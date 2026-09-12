//! 统一页数配额（v2.5 重构）：
//! - 计费主体（actor）统一：游客按 IP、登录用户按用户名，共用同一张表与同一套口径；
//! - 限额：游客默认 5 页 / 24 小时，登录用户默认 1000 页 / 24 小时
//!   （`GEMINUS_ANON_PAGE_LIMIT` / `GEMINUS_USER_PAGE_LIMIT`）；
//! - 24 小时滚动窗口：只统计窗口内扣减，历史用量自动「重置」；
//! - 计量单位：识别 + 翻译 1 页 = 1 个计数单位——同一 (actor, 文档哈希, 批) 幂等，
//!   断点续传/缓存秒回不重复扣；不存在「查看」「翻译」分开计费的路径；
//! - 记录落 SQLite（`data/auth_quota.db`），服务重启不重置；
//! - 直连本机（无代理头）的匿名请求视为本地开发流量，不限额。
//!
//! 计量口径：PDF 取真实页数；DOCX 按文本量估算（`Document::estimated_pages`）。

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 滚动窗口长度（秒）
pub const WINDOW_SECS: i64 = 24 * 3600;

#[derive(Debug)]
pub struct QuotaExceeded {
    pub used: i64,
    pub limit: i64,
    /// 窗口内最早一笔扣减到期时间（unix 秒）；额度由此逐页恢复
    pub reset_at: Option<i64>,
}

impl QuotaExceeded {
    /// 前端可直接展示的文案（游客 / 登录用户共用，由调用方选择）
    pub fn message_for(&self, is_user: bool) -> String {
        let who = if is_user { "24 小时内" } else { "游客 24 小时内" };
        match self.reset_at {
            Some(t) => format!(
                "{who}最多翻译 {} 页（当前已用 {} 页），额度从 {} 起逐页恢复。",
                self.limit,
                self.used,
                chrono_hhmm(t)
            ),
            None => format!("{who}最多翻译 {} 页（当前已用 {} 页）。", self.limit, self.used),
        }
    }
}

/// unix 秒 → 本地 HH:MM（仅提示用；无 chrono 依赖时退化为原值）
fn chrono_hhmm(ts: i64) -> String {
    let secs_of_day = ts.rem_euclid(86400);
    format!("{:02}:{:02}", secs_of_day / 3600, secs_of_day % 3600 / 60)
}

/// 计费主体：游客按 IP，登录用户按用户名（key 前缀隔离两类命名空间）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaActor {
    Guest(String),
    User(String),
}

impl QuotaActor {
    pub fn key(&self) -> String {
        match self {
            QuotaActor::Guest(ip) => format!("ip:{ip}"),
            QuotaActor::User(name) => format!("user:{name}"),
        }
    }

    pub fn is_user(&self) -> bool {
        matches!(self, QuotaActor::User(_))
    }
}

/// 统一页数配额仓（SQLite 持久，24h 滚动窗口）
pub struct QuotaStore {
    conn: Mutex<rusqlite::Connection>,
    guest_limit: i64,
    user_limit: i64,
}

/// 批级幂等键（同文档断点续传不重复扣费）
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS page_usage (
    actor TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    batch INTEGER NOT NULL DEFAULT 0,
    pages INTEGER NOT NULL,
    charged_at INTEGER NOT NULL,
    PRIMARY KEY (actor, content_hash, batch)
)";

impl QuotaStore {
    fn open(conn: rusqlite::Connection, guest_limit: i64, user_limit: i64) -> Self {
        conn.execute_batch(SCHEMA).expect("配额表创建失败");
        // 旧表迁移（v2.4 前后两代）→ 统一 page_usage：
        // v2.4 及之前按 IP 全历史累计、无 24h 窗口——迁移后旧记录保留 charged_at，
        // 自然滑出 24h 窗口（等效于一次性重置，兑现「额度 24 小时重置」语义）。
        let legacy_has_rows = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'anon_page_usage'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if legacy_has_rows {
            // 极旧表（无 batch 列）先升为批级主键
            let has_batch = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('anon_page_usage') WHERE name = 'batch'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;
            if !has_batch {
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
                .expect("旧配额表迁移失败");
            }
            conn.execute_batch(
                "INSERT OR IGNORE INTO page_usage (actor, content_hash, batch, pages, charged_at)
                    SELECT 'ip:' || ip, content_hash, batch, pages, charged_at FROM anon_page_usage;
                 DROP TABLE anon_page_usage;",
            )
            .expect("配额表迁移失败");
        }
        // 窗口外数据只用于审计追溯意义有限，顺手清理 7 天前记录防表膨胀
        let _ = conn.execute(
            "DELETE FROM page_usage WHERE charged_at < unixepoch() - ?1",
            [WINDOW_SECS * 7],
        );
        Self {
            conn: Mutex::new(conn),
            guest_limit,
            user_limit,
        }
    }

    /// 打开（或创建）磁盘库
    pub fn open_at(path: &Path, guest_limit: i64, user_limit: i64) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        Ok(Self::open(rusqlite::Connection::open(path)?, guest_limit, user_limit))
    }

    /// 内存库（测试用）
    pub fn open_in_memory(guest_limit: i64, user_limit: i64) -> rusqlite::Result<Self> {
        Ok(Self::open(
            rusqlite::Connection::open_in_memory()?,
            guest_limit,
            user_limit,
        ))
    }

    pub fn limit_for(&self, actor: &QuotaActor) -> i64 {
        if actor.is_user() {
            self.user_limit
        } else {
            self.guest_limit
        }
    }

    /// 该主体 24h 窗口内已用页数
    pub fn used_pages(&self, actor: &QuotaActor) -> i64 {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COALESCE(SUM(pages), 0) FROM page_usage
                 WHERE actor = ?1 AND charged_at > unixepoch() - ?2",
                rusqlite::params![actor.key(), WINDOW_SECS],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// 剩余额度（limit - 窗口内已用）
    pub fn remaining(&self, actor: &QuotaActor) -> i64 {
        (self.limit_for(actor) - self.used_pages(actor)).max(0)
    }

    /// 最早一页恢复时间（窗口内最早扣减 + 24h）；未用为 None
    pub fn reset_at(&self, actor: &QuotaActor) -> Option<i64> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT MIN(charged_at) FROM page_usage
                 WHERE actor = ?1 AND charged_at > unixepoch() - ?2",
                rusqlite::params![actor.key(), WINDOW_SECS],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
            .map(|t| t + WINDOW_SECS)
    }

    /// 该 (actor, hash, batch) 是否在当前窗口内已扣过（断点续传不重复扣）。
    /// 窗口语义：扣减滑出 24h 窗口 = 新的计费周期，同批重新实扣（REPLACE 覆盖旧记录）。
    pub fn already_charged_batch(&self, actor: &QuotaActor, content_hash: &str, batch: i64) -> bool {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM page_usage
                 WHERE actor = ?1 AND content_hash = ?2 AND batch = ?3
                   AND charged_at > unixepoch() - ?4",
                rusqlite::params![actor.key(), content_hash, batch, WINDOW_SECS],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// 该 (actor, hash) 是否存在任意已扣批（回访已译文档放行的豁免判据）
    pub fn has_any_charged_batch(&self, actor: &QuotaActor, content_hash: &str) -> bool {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM page_usage WHERE actor = ?1 AND content_hash = ?2 LIMIT 1",
                rusqlite::params![actor.key(), content_hash],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// 翻译循环批级实扣（BEGIN IMMEDIATE 事务，读-检-写原子）：
    /// - 批级幂等：同 (actor, hash, batch) 已扣 → Ok 直接跳过；
    /// - 事务内复核窗口内 used + pages ≤ limit，防同主体并发超发；
    /// - Err = 熔断信号（本次不落账）；
    /// - 调用方规则：本批全部可译块缓存命中（零 LLM 调用）→ 不调用本方法（免费秒回）。
    pub fn commit_chunk(
        &self,
        actor: &QuotaActor,
        content_hash: &str,
        batch: i64,
        pages: i64,
    ) -> Result<(), QuotaExceeded> {
        if pages <= 0 {
            return Ok(());
        }
        if self.already_charged_batch(actor, content_hash, batch) {
            return Ok(()); // 断点续传：该批已付过
        }
        let limit = self.limit_for(actor);
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .expect("配额事务开启失败");
        let (used, reset_at): (i64, Option<i64>) = tx
            .query_row(
                "SELECT COALESCE(SUM(pages), 0), MIN(charged_at) FROM page_usage
                 WHERE actor = ?1 AND charged_at > unixepoch() - ?2",
                rusqlite::params![actor.key(), WINDOW_SECS],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, None));
        if used + pages > limit {
            // 超限熔断：直接返回（事务 drop = rollback，本次 chunk 不落账）
            return Err(QuotaExceeded {
                used,
                limit,
                reset_at: reset_at.map(|t| t + WINDOW_SECS),
            });
        }
        tx.execute(
            "INSERT OR REPLACE INTO page_usage (actor, content_hash, batch, pages, charged_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch())",
            rusqlite::params![actor.key(), content_hash, batch, pages],
        )
        .map_err(|_| QuotaExceeded { used, limit, reset_at })?;
        tx.commit().map_err(|_| QuotaExceeded { used, limit, reset_at })?;
        Ok(())
    }

    /// 测试辅助：把该主体全部扣减时间回拨 secs 秒（验证 24h 窗口滑出）
    pub fn rewind_for_test(&self, actor: &QuotaActor, secs: i64) {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE page_usage SET charged_at = charged_at - ?1 WHERE actor = ?2",
                rusqlite::params![secs, actor.key()],
            )
            .expect("回拨测试时间失败");
    }
}

// ────────────────────────── 请求频次滑动窗口（内存） ──────────────────────────

/// 单位时间请求次数上限（单段重译等轻量端点的二级防线）
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

    fn guest() -> QuotaActor {
        QuotaActor::Guest("1.1.1.1".into())
    }

    #[test]
    fn commit_chunk_accumulates_and_blocks_at_limit() {
        let q = QuotaStore::open_in_memory(5, 1000).unwrap();
        assert_eq!(q.remaining(&guest()), 5);
        // 逐 chunk 扣 2+2 → 剩 1
        q.commit_chunk(&guest(), "docA", 0, 2).unwrap();
        q.commit_chunk(&guest(), "docA", 1, 2).unwrap();
        assert_eq!(q.remaining(&guest()), 1);
        // 检查点：剩余 1 < 需要 2 → 熔断（不落账）
        let err = q.commit_chunk(&guest(), "docA", 2, 2).unwrap_err();
        assert_eq!((err.used, err.limit), (4, 5));
        assert!(err.reset_at.is_some(), "熔断响应带恢复时间");
        assert_eq!(q.remaining(&guest()), 1, "熔断 chunk 不扣");
        // 恰好 1 页仍可扣
        q.commit_chunk(&guest(), "docA", 2, 1).unwrap();
        assert_eq!(q.remaining(&guest()), 0);
        // 彻底用尽后任何 chunk 熔断
        assert!(q.commit_chunk(&guest(), "docB", 0, 1).is_err());
    }

    #[test]
    fn batch_idempotent_for_resume() {
        let q = QuotaStore::open_in_memory(5, 1000).unwrap();
        q.commit_chunk(&guest(), "docA", 0, 1).unwrap();
        q.commit_chunk(&guest(), "docA", 1, 1).unwrap();
        assert_eq!(q.remaining(&guest()), 3);
        // 断点续传：已付批重发 → 幂等跳过，不重复扣
        q.commit_chunk(&guest(), "docA", 0, 1).unwrap();
        q.commit_chunk(&guest(), "docA", 1, 1).unwrap();
        assert_eq!(q.remaining(&guest()), 3, "重译已付批零重复扣费");
        // 新批继续扣
        q.commit_chunk(&guest(), "docA", 2, 1).unwrap();
        assert_eq!(q.remaining(&guest()), 2);
        assert!(q.already_charged_batch(&guest(), "docA", 0));
        assert!(!q.already_charged_batch(&guest(), "docA", 9));
        assert!(q.has_any_charged_batch(&guest(), "docA"));
        assert!(!q.has_any_charged_batch(&guest(), "docB"));
    }

    /// v2.5 核心：24h 滚动窗口——扣减滑出窗口后额度恢复
    #[test]
    fn window_sliding_resets_quota_after_24h() {
        let q = QuotaStore::open_in_memory(5, 1000).unwrap();
        q.commit_chunk(&guest(), "docA", 0, 5).unwrap();
        assert_eq!(q.remaining(&guest()), 0, "用满");
        assert!(q.reset_at(&guest()).is_some());
        // 23h 后：仍在窗口内
        q.rewind_for_test(&guest(), WINDOW_SECS - 3600);
        assert_eq!(q.used_pages(&guest()), 5, "23h：窗口内不恢复");
        // 25h 后：滑出窗口，额度全额恢复
        q.rewind_for_test(&guest(), 2 * 3600);
        assert_eq!(q.used_pages(&guest()), 0, "25h：滑出窗口");
        assert_eq!(q.remaining(&guest()), 5);
        assert!(q.reset_at(&guest()).is_none(), "无在窗扣减则无恢复时间");
    }

    /// 滑出窗口的旧批键残留 vs 重新扣同批：幂等读会误命中旧记录。
    /// 生产语义：额度恢复后重译同一文档是「重新计费」，不应被 24h 前的旧批键豁免。
    /// 处理：commit_chunk 幂等读只看窗口内记录（旧批键滑出窗口即失效，重新实扣）。
    #[test]
    fn stale_batch_keys_outside_window_are_recharged() {
        let q = QuotaStore::open_in_memory(5, 1000).unwrap();
        q.commit_chunk(&guest(), "docA", 0, 5).unwrap();
        q.rewind_for_test(&guest(), WINDOW_SECS + 1);
        // 旧批键滑出窗口 → 不再视为已付费
        assert!(!q.already_charged_batch(&guest(), "docA", 0), "窗口外批键失效");
        // 重新扣同批 → 覆盖为新的在窗记录（REPLACE），额度重新计
        q.commit_chunk(&guest(), "docA", 0, 3).unwrap();
        assert_eq!(q.used_pages(&guest()), 3, "只计新记录");
    }

    #[test]
    fn actor_isolation_guest_vs_user() {
        let q = QuotaStore::open_in_memory(5, 1000).unwrap();
        let guest2 = QuotaActor::Guest("2.2.2.2".into());
        let user = QuotaActor::User("alice".into());
        // 同 IP 的游客与登录用户是两个主体（登录即切换到 1000 页额度）
        assert_eq!(q.limit_for(&guest()), 5);
        assert_eq!(q.limit_for(&user), 1000);
        q.commit_chunk(&guest(), "docA", 0, 5).unwrap();
        assert_eq!(q.remaining(&guest()), 0);
        assert_eq!(q.remaining(&user), 1000, "登录主体独立额度");
        assert_eq!(q.remaining(&guest2), 5, "不同 IP 独立");
        // 用户名与 IP 字符串同值也不冲突（key 前缀隔离）
        let user_named_like_ip = QuotaActor::User("1.1.1.1".into());
        assert_ne!(user_named_like_ip.key(), guest().key());
        assert_eq!(q.remaining(&user_named_like_ip), 1000);
    }

    /// 登录用户 1000 页 / 24h 同样受限
    #[test]
    fn user_limit_enforced_after_1000_pages() {
        let q = QuotaStore::open_in_memory(5, 1000).unwrap();
        let user = QuotaActor::User("bob".into());
        for b in 0..1000 {
            q.commit_chunk(&user, "big.pdf", b, 1).unwrap();
        }
        assert_eq!(q.remaining(&user), 0);
        let err = q.commit_chunk(&user, "next.pdf", 0, 1).unwrap_err();
        assert_eq!((err.used, err.limit), (1000, 1000));
        // 24h 后恢复
        q.rewind_for_test(&user, WINDOW_SECS + 1);
        assert_eq!(q.remaining(&user), 1000);
    }

    /// v2.5 迁移：v2.4 anon_page_usage（批级主键）→ page_usage，旧记录滑出窗口等效重置
    #[test]
    fn migrates_v24_table_into_page_usage() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("quota.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE anon_page_usage (
                    ip TEXT NOT NULL, content_hash TEXT NOT NULL, batch INTEGER NOT NULL DEFAULT 0,
                    pages INTEGER NOT NULL, charged_at INTEGER NOT NULL,
                    PRIMARY KEY (ip, content_hash, batch));
                 INSERT INTO anon_page_usage VALUES ('1.1.1.1', 'h1', 0, 3, unixepoch());
                 INSERT INTO anon_page_usage VALUES ('1.1.1.1', 'h1', 1, 2, unixepoch() - 90000);
                 INSERT INTO anon_page_usage VALUES ('9.9.9.9', 'h2', 0, 5, 0);",
            )
            .unwrap();
        }
        let q = QuotaStore::open_at(&db, 5, 1000).unwrap();
        let guest = QuotaActor::Guest("1.1.1.1".into());
        // 在窗 3 页计入；90000s（>24h）前的 2 页已滑出 → 旧全历史「永不重置」问题一次性修复
        assert_eq!(q.used_pages(&guest), 3);
        assert_eq!(q.remaining(&guest), 2);
        // 极旧（epoch 0）记录同样滑出
        assert_eq!(q.remaining(&QuotaActor::Guest("9.9.9.9".into())), 5);
        // 旧表已删，幂等重开不报错
        drop(q);
        let q2 = QuotaStore::open_at(&db, 5, 1000).unwrap();
        assert_eq!(q2.used_pages(&guest), 3);
    }

    /// 极旧表（v2.4 前无 batch 列）→ 直接迁移
    #[test]
    fn migrates_legacy_no_batch_table() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("quota.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE anon_page_usage (
                    ip TEXT NOT NULL, content_hash TEXT NOT NULL,
                    pages INTEGER NOT NULL, charged_at INTEGER NOT NULL,
                    PRIMARY KEY (ip, content_hash));
                 INSERT INTO anon_page_usage VALUES ('1.1.1.1', 'h1', 3, unixepoch());",
            )
            .unwrap();
        }
        let q = QuotaStore::open_at(&db, 5, 1000).unwrap();
        let guest = QuotaActor::Guest("1.1.1.1".into());
        assert_eq!(q.used_pages(&guest), 3, "旧数据保留（batch=0）");
        assert!(q.already_charged_batch(&guest, "h1", 0));
        assert_eq!(q.remaining(&guest), 2);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("quota.db");
        let guest = QuotaActor::Guest("1.1.1.1".into());
        QuotaStore::open_at(&db, 5, 1000)
            .unwrap()
            .commit_chunk(&guest, "h", 0, 4)
            .unwrap();
        let reopened = QuotaStore::open_at(&db, 5, 1000).unwrap();
        assert_eq!(reopened.used_pages(&guest), 4);
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

    /// 并发超发：同主体 10 线程并发各扣 1 页，上限 5 → 恰好成功 5 次总扣 5
    #[test]
    fn commit_chunk_no_oversell_under_concurrency() {
        let q = std::sync::Arc::new(QuotaStore::open_in_memory(5, 1000).unwrap());
        let ok = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut handles = Vec::new();
        for i in 0..10 {
            let (q, ok) = (q.clone(), ok.clone());
            let actor = QuotaActor::Guest("9.9.9.9".into());
            handles.push(std::thread::spawn(move || {
                if q.commit_chunk(&actor, &format!("doc{i}"), 0, 1).is_ok() {
                    ok.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(ok.load(std::sync::atomic::Ordering::SeqCst), 5, "恰好放行 5 次");
        assert_eq!(q.used_pages(&QuotaActor::Guest("9.9.9.9".into())), 5, "总扣减 == 限额，零超发");
    }
}
