//! 匿名用户限流（部署安全加固）：
//! - 未登录用户按 IP 累计可「查看 + 翻译」的文档页数，上限默认 5 页；
//! - 同一文件（内容哈希）对同一 IP 只计一次——查看后翻译不重复扣减；
//! - 记录落 SQLite（`data/auth_quota.db`），服务重启不重置；
//! - 直连本机（无代理头）的请求视为本地开发流量，不限流。
//!
//! 计量口径：PDF 取真实页数；DOCX 按文本量估算（`Document::estimated_pages`）。

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct QuotaExceeded {
    pub used: i64,
    pub limit: i64,
}

impl QuotaExceeded {
    /// 前端可直接展示的文案
    pub fn message(&self) -> String {
        format!(
            "未登录用户最多查看并翻译 {} 页（当前已用 {} 页）。请登录后继续使用。",
            self.limit, self.used
        )
    }
}

/// 匿名页数配额仓（SQLite 持久）
pub struct QuotaStore {
    conn: Mutex<rusqlite::Connection>,
    limit: i64,
}

/// v2.4 T5：批级幂等键（同文档断点续传不重复扣费；旧 (ip,hash) 主键表自动迁移）
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS anon_page_usage (
    ip TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    batch INTEGER NOT NULL DEFAULT 0,
    pages INTEGER NOT NULL,
    charged_at INTEGER NOT NULL,
    PRIMARY KEY (ip, content_hash, batch)
)";

impl QuotaStore {
    fn open(conn: rusqlite::Connection, limit: i64) -> Self {
        // 旧表迁移：无 batch 列（v2.4 前的主键 (ip, content_hash)）→ 重建为批级主键
        let has_batch = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('anon_page_usage') WHERE name = 'batch'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        let table_exists = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'anon_page_usage'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if table_exists && !has_batch {
            conn.execute_batch(
                "ALTER TABLE anon_page_usage RENAME TO anon_page_usage_old;
                 CREATE TABLE anon_page_usage (
                    ip TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    batch INTEGER NOT NULL DEFAULT 0,
                    pages INTEGER NOT NULL,
                    charged_at INTEGER NOT NULL,
                    PRIMARY KEY (ip, content_hash, batch)
                 );
                 INSERT OR IGNORE INTO anon_page_usage (ip, content_hash, batch, pages, charged_at)
                    SELECT ip, content_hash, 0, pages, charged_at FROM anon_page_usage_old;
                 DROP TABLE anon_page_usage_old;",
            )
            .expect("配额表迁移失败");
        }
        conn.execute_batch(SCHEMA).expect("配额表创建失败");
        Self {
            conn: Mutex::new(conn),
            limit,
        }
    }

    /// 打开（或创建）磁盘库
    pub fn open_at(path: &Path, limit: i64) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        Ok(Self::open(rusqlite::Connection::open(path)?, limit))
    }

    /// 内存库（测试用）
    pub fn open_in_memory(limit: i64) -> rusqlite::Result<Self> {
        Ok(Self::open(rusqlite::Connection::open_in_memory()?, limit))
    }

    pub fn limit(&self) -> i64 {
        self.limit
    }

    /// 该 IP 已累计页数
    pub fn used_pages(&self, ip: &str) -> i64 {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COALESCE(SUM(pages), 0) FROM anon_page_usage WHERE ip = ?1",
                [ip],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// 该内容是否已对该 IP 计过费（查看后再翻译不重复扣）
    pub fn already_charged(&self, ip: &str, content_hash: &str) -> bool {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM anon_page_usage WHERE ip = ?1 AND content_hash = ?2",
                [ip, content_hash],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// 预检（不落账）：再消耗 pages 页是否会超限
    pub fn would_exceed(&self, ip: &str, pages: i64) -> bool {
        self.used_pages(ip) + pages > self.limit
    }

    /// 计费（幂等：同 IP 同内容只扣一次）。
    /// Ok = 放行；Err = 超限（本次不落账）。
    pub fn charge(&self, ip: &str, content_hash: &str, pages: i64) -> Result<(), QuotaExceeded> {
        let conn = self.conn.lock().unwrap();
        let charged: bool = conn
            .query_row(
                "SELECT 1 FROM anon_page_usage WHERE ip = ?1 AND content_hash = ?2",
                [ip, content_hash],
                |_| Ok(()),
            )
            .is_ok();
        if charged {
            return Ok(());
        }
        let used: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(pages), 0) FROM anon_page_usage WHERE ip = ?1",
                [ip],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if used + pages > self.limit {
            return Err(QuotaExceeded { used, limit: self.limit });
        }
        conn.execute(
            "INSERT OR IGNORE INTO anon_page_usage (ip, content_hash, pages, charged_at) VALUES (?1, ?2, ?3, unixepoch())",
            rusqlite::params![ip, content_hash, pages],
        )
        .map_err(|_| QuotaExceeded { used, limit: self.limit })?;
        Ok(())
    }

    /// 剩余额度（limit - used）
    pub fn remaining(&self, ip: &str) -> i64 {
        (self.limit - self.used_pages(ip)).max(0)
    }

    /// 该 (ip, hash, batch) 是否已付费（批级幂等——断点续传不重复扣）
    pub fn already_charged_batch(&self, ip: &str, content_hash: &str, batch: i64) -> bool {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM anon_page_usage WHERE ip = ?1 AND content_hash = ?2 AND batch = ?3",
                rusqlite::params![ip, content_hash, batch],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// v2.4 T5：翻译循环 chunk 级实扣（BEGIN IMMEDIATE 事务，读-检-写原子）。
    /// - 批级幂等：同 (ip, hash, batch) 已扣 → Ok 直接跳过；
    /// - 事务内复核 used + pages ≤ limit，防同 IP 并发超发；
    /// - Err = 熔断信号（本次不落账）；
    /// - 调用方规则：本批全部可译块缓存命中（零 LLM 调用）→ 不调用本方法（免费秒回）。
    pub fn commit_chunk(
        &self,
        ip: &str,
        content_hash: &str,
        batch: i64,
        pages: i64,
    ) -> Result<(), QuotaExceeded> {
        if self.already_charged_batch(ip, content_hash, batch) {
            return Ok(()); // 断点续传：该批已付过
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .expect("配额事务开启失败");
        let used: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(pages), 0) FROM anon_page_usage WHERE ip = ?1",
                [ip],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if used + pages > self.limit {
            // 超限熔断：直接返回（事务 drop = rollback，本次 chunk 不落账）
            return Err(QuotaExceeded { used, limit: self.limit });
        }
        tx.execute(
            "INSERT INTO anon_page_usage (ip, content_hash, batch, pages, charged_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch())",
            rusqlite::params![ip, content_hash, batch, pages],
        )
        .map_err(|_| QuotaExceeded { used, limit: self.limit })?;
        tx.commit().map_err(|_| QuotaExceeded { used, limit: self.limit })?;
        Ok(())
    }
}

// ────────────────────────── 请求频次滑动窗口（内存） ──────────────────────────

/// 单位时间请求次数上限（匿名单段重译等轻量端点的二级防线）
#[derive(Clone, Default)]
pub struct RateWindow {
    max: usize,
    window: Duration,
    events: std::sync::Arc<Mutex<Vec<(String, Instant)>>>,
}

impl RateWindow {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            events: std::sync::Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 记录一次并判断是否放行（窗口内第 1..=max 次 → true）
    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut events = self.events.lock().unwrap();
        events.retain(|(_, t)| now.duration_since(*t) < self.window);
        let count = events.iter().filter(|(k, _)| k == key).count();
        events.push((key.to_string(), now));
        count < self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_accumulates_and_blocks() {
        let q = QuotaStore::open_in_memory(5).unwrap();
        assert!(q.charge("1.1.1.1", "hashA", 3).is_ok());
        assert_eq!(q.used_pages("1.1.1.1"), 3);
        // 同内容重复计费幂等（查看后翻译不重复扣）
        assert!(q.charge("1.1.1.1", "hashA", 3).is_ok());
        assert_eq!(q.used_pages("1.1.1.1"), 3);
        // 再来 3 页超限
        let err = q.charge("1.1.1.1", "hashB", 3).unwrap_err();
        assert_eq!((err.used, err.limit), (3, 5));
        assert!(err.message().contains("请登录"));
        // 超限的 B 未落账，恰好 2 页可放行
        assert!(q.charge("1.1.1.1", "hashB", 2).is_ok());
        // 其他 IP 不受影响
        assert!(q.charge("2.2.2.2", "hashB", 3).is_ok());
    }

    #[test]
    fn would_exceed_peek_does_not_write() {
        let q = QuotaStore::open_in_memory(5).unwrap();
        assert!(!q.would_exceed("3.3.3.3", 5), "恰好用满额度应放行");
        assert!(q.would_exceed("3.3.3.3", 6));
        assert!(!q.would_exceed("3.3.3.3", 4));
        assert_eq!(q.used_pages("3.3.3.3"), 0, "预检不应落账");
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("quota.db");
        QuotaStore::open_at(&db, 5).unwrap().charge("1.1.1.1", "h", 4).unwrap();
        let reopened = QuotaStore::open_at(&db, 5).unwrap();
        assert_eq!(reopened.used_pages("1.1.1.1"), 4);
    }

    #[test]
    fn rate_window_caps_burst() {
        let w = RateWindow::new(3, Duration::from_secs(60));
        for _ in 0..3 {
            assert!(w.allow("ip"));
        }
        assert!(!w.allow("ip"));
        assert!(w.allow("other"));
    }

    // ────────────────── v2.4 T5：chunk 实扣 ──────────────────

    #[test]
    fn commit_chunk_accumulates_and_blocks_at_limit() {
        let q = QuotaStore::open_in_memory(5).unwrap();
        assert_eq!(q.remaining("1.1.1.1"), 5);
        // 逐 chunk 扣 2+2 → 剩 1
        q.commit_chunk("1.1.1.1", "docA", 0, 2).unwrap();
        q.commit_chunk("1.1.1.1", "docA", 1, 2).unwrap();
        assert_eq!(q.remaining("1.1.1.1"), 1);
        // 检查点：剩余 1 < 需要 2 → 熔断（不落账）
        let err = q.commit_chunk("1.1.1.1", "docA", 2, 2).unwrap_err();
        assert_eq!((err.used, err.limit), (4, 5));
        assert_eq!(q.remaining("1.1.1.1"), 1, "熔断 chunk 不扣");
        // 恰好 1 页仍可扣
        q.commit_chunk("1.1.1.1", "docA", 2, 1).unwrap();
        assert_eq!(q.remaining("1.1.1.1"), 0);
        // 彻底用尽后任何 chunk 熔断
        assert!(q.commit_chunk("1.1.1.1", "docB", 0, 1).is_err());
    }

    #[test]
    fn commit_chunk_batch_idempotent_for_resume() {
        let q = QuotaStore::open_in_memory(5).unwrap();
        q.commit_chunk("1.1.1.1", "docA", 0, 1).unwrap();
        q.commit_chunk("1.1.1.1", "docA", 1, 1).unwrap();
        assert_eq!(q.remaining("1.1.1.1"), 3);
        // 断点续传：已付批重发 → 幂等跳过，不重复扣
        q.commit_chunk("1.1.1.1", "docA", 0, 1).unwrap();
        q.commit_chunk("1.1.1.1", "docA", 1, 1).unwrap();
        assert_eq!(q.remaining("1.1.1.1"), 3, "重译已付批零重复扣费");
        // 新批继续扣
        q.commit_chunk("1.1.1.1", "docA", 2, 1).unwrap();
        assert_eq!(q.remaining("1.1.1.1"), 2);
        assert!(q.already_charged_batch("1.1.1.1", "docA", 0));
        assert!(!q.already_charged_batch("1.1.1.1", "docA", 9));
    }

    #[test]
    fn commit_chunk_isolated_per_ip_and_hash() {
        let q = QuotaStore::open_in_memory(5).unwrap();
        q.commit_chunk("1.1.1.1", "docA", 0, 3).unwrap();
        // 其他 IP / 其他文档独立
        assert!(q.commit_chunk("2.2.2.2", "docA", 0, 3).is_ok());
        assert!(q.commit_chunk("1.1.1.1", "docB", 0, 3).is_err(), "同 IP 跨文档共享额度");
    }

    /// T5.5 并发超发：同 IP 10 线程并发各扣 1 页，上限 5 → 恰好成功 5 次总扣 5
    #[test]
    fn commit_chunk_no_oversell_under_concurrency() {
        let q = std::sync::Arc::new(QuotaStore::open_in_memory(5).unwrap());
        let ok = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut handles = Vec::new();
        for i in 0..10 {
            let (q, ok) = (q.clone(), ok.clone());
            handles.push(std::thread::spawn(move || {
                if q.commit_chunk("9.9.9.9", &format!("doc{i}"), 0, 1).is_ok() {
                    ok.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(ok.load(std::sync::atomic::Ordering::SeqCst), 5, "恰好放行 5 次");
        assert_eq!(q.used_pages("9.9.9.9"), 5, "总扣减 == 限额，零超发");
    }

    /// v2.4 迁移：旧主键 (ip, hash) 表 → 批级主键，数据保留
    #[test]
    fn migrates_legacy_table_and_keeps_usage() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("quota.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE anon_page_usage (
                    ip TEXT NOT NULL, content_hash TEXT NOT NULL,
                    pages INTEGER NOT NULL, charged_at INTEGER NOT NULL,
                    PRIMARY KEY (ip, content_hash));
                 INSERT INTO anon_page_usage VALUES ('1.1.1.1', 'h1', 3, 0);",
            )
            .unwrap();
        }
        let reopened = QuotaStore::open_at(&db, 5).unwrap();
        assert_eq!(reopened.used_pages("1.1.1.1"), 3, "旧数据保留（batch=0）");
        assert!(reopened.already_charged("1.1.1.1", "h1"), "already_charged 语义兼容");
        assert_eq!(reopened.remaining("1.1.1.1"), 2);
        // 迁移后新批可继续扣
        reopened.commit_chunk("1.1.1.1", "h1", 1, 2).unwrap();
        assert_eq!(reopened.remaining("1.1.1.1"), 0);
    }
}
