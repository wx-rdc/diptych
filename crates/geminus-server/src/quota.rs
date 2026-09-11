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

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS anon_page_usage (
    ip TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    pages INTEGER NOT NULL,
    charged_at INTEGER NOT NULL,
    PRIMARY KEY (ip, content_hash)
)";

impl QuotaStore {
    fn open(conn: rusqlite::Connection, limit: i64) -> Self {
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
}
