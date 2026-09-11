//! SQLite 翻译缓存（plan.md §4.3）：SHA-256 内容哈希 + (hash, source_lang, target_lang) 唯一键。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("序列化错误: {0}")]
    Serde(String),
}

/// 原文内容的 SHA-256 十六进制哈希（缓存键组成部分）
pub fn content_hash(source_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS translations (
    id           INTEGER PRIMARY KEY,
    content_hash TEXT NOT NULL,
    source_lang  TEXT NOT NULL,
    target_lang  TEXT NOT NULL,
    source_text  TEXT NOT NULL,
    target_text  TEXT NOT NULL,
    model        TEXT NOT NULL,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(content_hash, source_lang, target_lang)
);
CREATE INDEX IF NOT EXISTS idx_hash ON translations(content_hash);
CREATE TABLE IF NOT EXISTS mineru_parses (
    file_hash       TEXT PRIMARY KEY,
    ir_json         TEXT NOT NULL,
    raw_root        TEXT,
    schema_version  TEXT,
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);
";

/// mineru_parses 的 IR 缓存版本（v2.2 T1.4）：适配器升级后 bump → 旧 IR 自动失效，
/// 有 raw_root 的条目可离线重适配（零 MinerU API）
pub const IR_SCHEMA_VERSION: &str = "v2.2";
/// 含行内公式段落的翻译缓存版本（v2.2 T5.3）：math_slot 令牌化上线，
/// 旧的被污染译文（公式被模型改写）自动失效重译；纯文本段落不受影响
pub const MATH_PIPELINE_VERSION: &str = "v2.2-math";

/// 翻译缓存（Connection 由 Mutex 保护，跨线程共享）
pub struct TranslationCache {
    conn: Mutex<Connection>,
}

impl TranslationCache {
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// 文件内容 SHA-256（流式读取，供 MinerU 结果缓存键）
    pub fn file_sha256(path: &Path) -> Result<String, std::io::Error> {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// MinerU 解析结果缓存：命中返回反序列化的 Document。
    /// 版本不匹配（schema_version 列为 v1/NULL 或不同值）→ None（上层走 raw 重适配或重解析）。
    pub fn get_mineru_parse(&self, file_hash: &str, expect_version: &str) -> Option<crate::model::Document> {
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.query_row(
            "SELECT ir_json, schema_version FROM mineru_parses WHERE file_hash = ?1",
            params![file_hash],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .unwrap_or(None)
        .filter(|(_, ver)| ver.as_deref() == Some(expect_version))
        .and_then(|(json, _)| serde_json::from_str(&json).ok())
    }

    /// 写入 MinerU 解析结果（同键覆盖；raw_root 供离线重放定位）
    pub fn put_mineru_parse(
        &self,
        file_hash: &str,
        doc: &crate::model::Document,
        raw_root: Option<&str>,
        version: &str,
    ) -> Result<(), CacheError> {
        let json = serde_json::to_string(doc)
            .map_err(|e| CacheError::Serde(e.to_string()))?;
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.execute(
            "INSERT INTO mineru_parses (file_hash, ir_json, raw_root, schema_version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(file_hash) DO UPDATE SET
                ir_json = excluded.ir_json,
                raw_root = excluded.raw_root,
                schema_version = excluded.schema_version",
            params![file_hash, json, raw_root, version],
        )?;
        Ok(())
    }

    /// raw 缓存目录路径（离线重放定位）
    pub fn mineru_raw_root(&self, file_hash: &str) -> Option<String> {
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.query_row(
            "SELECT raw_root FROM mineru_parses WHERE file_hash = ?1",
            params![file_hash],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .unwrap_or(None)
        .flatten()
    }

    /// 内存库（测试 / mock 场景）
    pub fn open_in_memory() -> Result<Self, CacheError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, CacheError> {
        // WAL + NORMAL：桌面应用典型配置，单条写入不再逐次 fsync（PoC-③ 性能验收）
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        // 存量库迁移：mineru_parses 补 raw_root/schema_version 列（v2.2 T1.16；
        // 列已存在时 ALTER 报 duplicate column → 忽略继续）
        for ddl in [
            "ALTER TABLE mineru_parses ADD COLUMN raw_root TEXT",
            "ALTER TABLE mineru_parses ADD COLUMN schema_version TEXT",
        ] {
            if let Err(e) = conn.execute(ddl, []) {
                if !e.to_string().contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 查缓存；命中返回译文
    pub fn get(&self, source_text: &str, source_lang: &str, target_lang: &str) -> Option<String> {
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.query_row(
            "SELECT target_text FROM translations
             WHERE content_hash = ?1 AND source_lang = ?2 AND target_lang = ?3",
            params![content_hash(source_text), source_lang, target_lang],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None)
    }

    /// 写缓存（同键覆盖）
    pub fn put(
        &self,
        source_text: &str,
        source_lang: &str,
        target_lang: &str,
        target_text: &str,
        model: &str,
    ) -> Result<(), CacheError> {
        self.put_impl(source_text, source_lang, target_lang, target_text, model, None)
    }

    /// 版本化读（v2.2 T5.3）：pipeline 版本混入哈希——仅含公式段落使用，
    /// 与旧键隔离，math_slot 上线后旧的被污染译文自动失效
    pub fn get_versioned(
        &self,
        source_text: &str,
        source_lang: &str,
        target_lang: &str,
        version: &str,
    ) -> Option<String> {
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.query_row(
            "SELECT target_text FROM translations
             WHERE content_hash = ?1 AND source_lang = ?2 AND target_lang = ?3",
            params![
                Self::hash_with_version(source_text, Some(version)),
                source_lang,
                target_lang
            ],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None)
    }

    /// 版本化写
    pub fn put_versioned(
        &self,
        source_text: &str,
        source_lang: &str,
        target_lang: &str,
        target_text: &str,
        model: &str,
        version: &str,
    ) -> Result<(), CacheError> {
        self.put_impl(source_text, source_lang, target_lang, target_text, model, Some(version))
    }

    fn hash_with_version(source_text: &str, version: Option<&str>) -> String {
        let mut hasher = Sha256::new();
        if let Some(v) = version {
            hasher.update(v.as_bytes());
            hasher.update(b"\x00");
        }
        hasher.update(source_text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn put_impl(
        &self,
        source_text: &str,
        source_lang: &str,
        target_lang: &str,
        target_text: &str,
        model: &str,
        version: Option<&str>,
    ) -> Result<(), CacheError> {
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.execute(
            "INSERT OR REPLACE INTO translations
             (content_hash, source_lang, target_lang, source_text, target_text, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Self::hash_with_version(source_text, version),
                source_lang,
                target_lang,
                source_text,
                target_text,
                model
            ],
        )?;
        Ok(())
    }

    /// 批量写（单事务）
    pub fn put_many(
        &self,
        entries: &[(String, String, String, String, String)], // (source_text, src, tgt, target_text, model)
    ) -> Result<(), CacheError> {
        let mut conn = self.conn.lock().expect("缓存锁中毒");
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO translations
                 (content_hash, source_lang, target_lang, source_text, target_text, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (source_text, src, tgt, target_text, model) in entries {
                stmt.execute(params![
                    content_hash(source_text),
                    src,
                    tgt,
                    source_text,
                    target_text,
                    model
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 缓存条目数（诊断/测试用）
    pub fn len(&self) -> usize {
        let conn = self.conn.lock().expect("缓存锁中毒");
        conn.query_row("SELECT COUNT(*) FROM translations", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, PdfMeta, RenderMeta};
    use std::collections::HashMap;

    #[test]
    fn mineru_parse_cache_roundtrip() {
        let cache = TranslationCache::open_in_memory().unwrap();
        let mut doc = Document {
            id: "d1".into(),
            title: "t".into(),
            blocks: vec![],
            resources: HashMap::new(),
            render_meta: HashMap::new(),
            pdf_meta: None,
        };
        assert!(cache.get_mineru_parse("hash1", IR_SCHEMA_VERSION).is_none());
        cache
            .put_mineru_parse("hash1", &doc, None, IR_SCHEMA_VERSION)
            .unwrap();
        let back = cache.get_mineru_parse("hash1", IR_SCHEMA_VERSION).unwrap();
        assert_eq!(back.id, "d1");
        // 覆盖写不报错，且 render_meta/pdf_meta 序列化无损
        doc.render_meta
            .insert("b_0000".into(), RenderMeta { page_idx: 2, bbox: [1.0, 2.0, 3.0, 4.0] });
        doc.pdf_meta = Some(PdfMeta { page_count: 3, page_sizes: vec![[612.0, 792.0]; 3] });
        cache
            .put_mineru_parse("hash1", &doc, Some("data/mineru_cache/hash1"), IR_SCHEMA_VERSION)
            .unwrap();
        let back = cache.get_mineru_parse("hash1", IR_SCHEMA_VERSION).unwrap();
        assert_eq!(back.pdf_meta.as_ref().unwrap().page_count, 3);
        assert_eq!(back.render_meta["b_0000"].page_idx, 2);
        assert_eq!(cache.mineru_raw_root("hash1").as_deref(), Some("data/mineru_cache/hash1"));
    }

    #[test]
    fn mineru_parse_version_and_raw_root_roundtrip() {
        let cache = TranslationCache::open_in_memory().unwrap();
        let doc = Document {
            id: "d".into(),
            title: "t".into(),
            blocks: vec![],
            resources: HashMap::new(),
            render_meta: HashMap::new(),
            pdf_meta: None,
        };
        // 无记录 → miss
        assert!(cache.get_mineru_parse(&doc.id, "v2.2").is_none());
        cache.put_mineru_parse(&doc.id, &doc, Some("raw/here"), "v2.2").unwrap();
        // 版本不匹配 → miss（旧 IR 失效）
        assert!(cache.get_mineru_parse(&doc.id, "v2.1").is_none());
        // 版本匹配 → 命中
        assert!(cache.get_mineru_parse(&doc.id, "v2.2").is_some());
        assert_eq!(cache.mineru_raw_root(&doc.id).as_deref(), Some("raw/here"));
        // raw_root 为 None 的条目（在线解析但未持久化）
        cache.put_mineru_parse(&doc.id, &doc, None, "v2.2").unwrap();
        assert!(cache.mineru_raw_root(&doc.id).is_none());
        assert!(cache.get_mineru_parse(&doc.id, "v2.2").is_some());
    }

    #[test]
    fn legacy_mineru_rows_without_version_miss_on_new_schema() {
        // 模拟 v2.1 存量库：直接 SQL 写无 schema_version 的行（init 迁移后列为 NULL）
        let cache = TranslationCache::open_in_memory().unwrap();
        let doc = Document {
            id: "d".into(),
            title: "t".into(),
            blocks: vec![],
            resources: HashMap::new(),
            render_meta: HashMap::new(),
            pdf_meta: None,
        };
        let json = serde_json::to_string(&doc).unwrap();
        {
            let conn = cache.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO mineru_parses (file_hash, ir_json) VALUES ('h', ?1)",
                params![json],
            )
            .unwrap();
        }
        assert!(cache.get_mineru_parse("h", IR_SCHEMA_VERSION).is_none(), "旧 IR 应失效");
        assert!(cache.mineru_raw_root("h").is_none());
    }

    #[test]
    fn math_versioned_key_isolated_from_legacy() {
        let cache = TranslationCache::open_in_memory().unwrap();
        cache.put("has $x$ math", "en", "zh", "旧污染译文", "m").unwrap();
        // 版本化键与旧键隔离：写入不覆盖、读取不串台
        cache
            .put_versioned("has $x$ math", "en", "zh", "新保真译文", "m", MATH_PIPELINE_VERSION)
            .unwrap();
        assert_eq!(cache.get("has $x$ math", "en", "zh"), Some("旧污染译文".into()));
        assert_eq!(
            cache.get_versioned("has $x$ math", "en", "zh", MATH_PIPELINE_VERSION),
            Some("新保真译文".into())
        );
        assert_eq!(cache.get_versioned("has $x$ math", "en", "zh", "v2.3-x"), None);
        assert_eq!(cache.len(), 2, "两套键共存");
    }

    #[test]
    fn file_sha256_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        std::fs::write(&p, b"hello geminus").unwrap();
        let h1 = TranslationCache::file_sha256(&p).unwrap();
        let h2 = TranslationCache::file_sha256(&p).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        std::fs::write(&p, b"hello geminus2").unwrap();
        assert_ne!(h1, TranslationCache::file_sha256(&p).unwrap());
    }

    #[test]
    fn hash_is_stable_and_distinct() {
        assert_eq!(content_hash("hello"), content_hash("hello"));
        assert_ne!(content_hash("hello"), content_hash("world"));
        assert_eq!(content_hash("hello").len(), 64);
    }

    #[test]
    fn miss_then_hit() {
        let cache = TranslationCache::open_in_memory().unwrap();
        assert_eq!(cache.get("Hello, world!", "en", "zh"), None);
        cache
            .put("Hello, world!", "en", "zh", "你好，世界！", "deepseek-v4-flash")
            .unwrap();
        assert_eq!(cache.get("Hello, world!", "en", "zh"), Some("你好，世界！".to_string()));
    }

    #[test]
    fn duplicate_put_replaces_without_error() {
        let cache = TranslationCache::open_in_memory().unwrap();
        cache.put("t", "en", "zh", "first", "m").unwrap();
        cache.put("t", "en", "zh", "second", "m").unwrap();
        assert_eq!(cache.get("t", "en", "zh"), Some("second".to_string()));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn langs_do_not_cross() {
        let cache = TranslationCache::open_in_memory().unwrap();
        cache.put("t", "en", "zh", "中文", "m").unwrap();
        assert_eq!(cache.get("t", "en", "zh"), Some("中文".to_string()));
        assert_eq!(cache.get("t", "en", "ja"), None);
        assert_eq!(cache.get("t", "fr", "zh"), None);
    }

    #[test]
    fn file_backed_cache_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("cache.db");
        {
            let cache = TranslationCache::open(&path).unwrap();
            cache.put("persist", "en", "zh", "持久", "m").unwrap();
        }
        let cache2 = TranslationCache::open(&path).unwrap();
        assert_eq!(cache2.get("persist", "en", "zh"), Some("持久".to_string()));
    }

    #[test]
    fn thousand_entries_performance() {
        let dir = tempfile::tempdir().unwrap();
        let cache = TranslationCache::open(&dir.path().join("perf.db")).unwrap();
        let start = std::time::Instant::now();
        for i in 0..1000 {
            cache.put(&format!("paragraph number {i}"), "en", "zh", &format!("第{i}段"), "m").unwrap();
        }
        for i in 0..1000 {
            assert!(cache.get(&format!("paragraph number {i}"), "en", "zh").is_some());
        }
        let elapsed = start.elapsed();
        assert_eq!(cache.len(), 1000);
        assert!(elapsed.as_secs() < 5, "1000 写+1000 读应 <5s，实际 {elapsed:?}");
    }
}
