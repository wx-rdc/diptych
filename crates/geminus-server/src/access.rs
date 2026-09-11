//! 文档访问凭证（v2.4 T1）：匿名防盗链。
//! - 上传/解析成功 → 下发 `access_token`（UUID v4，写 `doc_access` 表）；
//! - `GET /api/documents/:id` 等端点按「三选一」校验：
//!   ① 登录 Session（所有者/历史关联）② token 匹配 ③ uploader_ip 匹配；
//! - 匿名文档 24h TTL（`purge_expired` 供后台清理，T6 cron）；
//! - 直连本机（无代理头）= 本地开发流量，放行（与匿名配额口径一致）。
//!
//! 计划偏差说明：原计划 `ALTER TABLE documents ADD COLUMN access_token`——
//! 本仓 `documents` 是内存 HashMap（无 SQLite 表），故独立建 `doc_access` 表。

use std::path::Path;
use std::sync::Mutex;

/// 匿名文档访问 token 的生存期（24h）
pub const ANON_TOKEN_TTL_SECS: i64 = 24 * 3600;

/// 文档访问仓（SQLite 持久）
pub struct DocAccessStore {
    conn: Mutex<rusqlite::Connection>,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS doc_access (
    doc_id TEXT PRIMARY KEY,
    access_token TEXT NOT NULL,
    uploader_ip TEXT,
    is_anonymous INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
)";

impl DocAccessStore {
    fn open(conn: rusqlite::Connection) -> Self {
        conn.execute_batch(SCHEMA).expect("doc_access 表创建失败");
        Self { conn: Mutex::new(conn) }
    }

    /// 打开（或创建）磁盘库
    pub fn open_at(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        Ok(Self::open(rusqlite::Connection::open(path)?))
    }

    /// 内存库（测试 / AppState::new 默认）
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Ok(Self::open(rusqlite::Connection::open_in_memory()?))
    }

    /// 登记文档访问凭证，返回该文档的 access_token。
    /// token 仅首次生成（重传/回源不轮换，持有者不失效）；uploader_ip 每次更新。
    pub fn issue(&self, doc_id: &str, uploader_ip: Option<&str>, is_anonymous: bool) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO doc_access (doc_id, access_token, uploader_ip, is_anonymous, created_at)
             VALUES (?1, ?2, ?3, ?4, unixepoch())
             ON CONFLICT(doc_id) DO UPDATE SET
                uploader_ip = excluded.uploader_ip,
                is_anonymous = excluded.is_anonymous",
            rusqlite::params![doc_id, token, uploader_ip, is_anonymous as i64],
        );
        conn.query_row(
            "SELECT access_token FROM doc_access WHERE doc_id = ?1",
            [doc_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or(token)
    }

    /// 三选一之 ②：token 匹配
    pub fn verify_token(&self, doc_id: &str, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM doc_access WHERE doc_id = ?1 AND access_token = ?2",
                [doc_id, token],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// 三选一之 ③：上传者 IP 匹配
    pub fn uploader_ip(&self, doc_id: &str) -> Option<String> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT uploader_ip FROM doc_access WHERE doc_id = ?1",
                [doc_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
    }

    /// 文档是否匿名上传（TTL 清理范围判定）
    pub fn is_anonymous(&self, doc_id: &str) -> bool {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT is_anonymous FROM doc_access WHERE doc_id = ?1",
                [doc_id],
                |r| r.get::<_, i64>(0),
            )
            .is_ok_and(|v| v != 0)
    }

    /// 匿名 token TTL 过期清理：返回过期 doc_id 列表（调用方据此清 IR/缓存/uploads）
    pub fn purge_expired(&self, ttl_secs: i64) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) =
            conn.prepare("SELECT doc_id FROM doc_access WHERE is_anonymous = 1 AND created_at < unixepoch() - ?1")
        else {
            return Vec::new();
        };
        let expired: Vec<String> = stmt
            .query_map([ttl_secs], |r| r.get::<_, String>(0))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        for doc in &expired {
            let _ = conn.execute("DELETE FROM doc_access WHERE doc_id = ?1", [doc]);
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_then_verify_token() {
        let s = DocAccessStore::open_in_memory().unwrap();
        let t1 = s.issue("d1", Some("1.2.3.4"), true);
        let t2 = s.issue("d2", None, false);
        assert!(!t1.is_empty() && !t2.is_empty() && t1 != t2, "token 唯一");
        assert!(s.verify_token("d1", &t1));
        assert!(s.verify_token("d2", &t2));
        assert!(!s.verify_token("d1", &t2), "错 token 不放行");
        assert!(!s.verify_token("d1", ""), "空 token 不放行");
        assert!(!s.verify_token("nope", &t1), "未知 doc 不放行");
    }

    #[test]
    fn reissue_keeps_token_but_refreshes_ip() {
        let s = DocAccessStore::open_in_memory().unwrap();
        let t1 = s.issue("d1", Some("1.1.1.1"), true);
        let t2 = s.issue("d1", Some("2.2.2.2"), true);
        assert_eq!(t1, t2, "重传不轮换 token（持有者不失效）");
        assert_eq!(s.uploader_ip("d1").as_deref(), Some("2.2.2.2"), "IP 更新为新上传者");
    }

    #[test]
    fn uploader_ip_roundtrip() {
        let s = DocAccessStore::open_in_memory().unwrap();
        s.issue("d1", Some("9.9.9.9"), true);
        s.issue("d2", None, false);
        assert_eq!(s.uploader_ip("d1").as_deref(), Some("9.9.9.9"));
        assert_eq!(s.uploader_ip("d2"), None);
        assert!(!s.is_anonymous("d2"));
        assert!(s.is_anonymous("d1"));
    }

    #[test]
    fn purge_expired_only_touches_anonymous_and_old() {
        let s = DocAccessStore::open_in_memory().unwrap();
        s.issue("anon_fresh", Some("1.1.1.1"), true);
        s.issue("login_old", None, false);
        // 手工把一条匿名记录回拨 25h
        s.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE doc_access SET created_at = unixepoch() - ?1 WHERE doc_id = 'anon_old'",
                [ANON_TOKEN_TTL_SECS + 3600],
            )
            .unwrap();
        // anon_old 未登记过——先补一条再回拨
        s.issue("anon_old", Some("1.1.1.1"), true);
        s.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE doc_access SET created_at = unixepoch() - ?1 WHERE doc_id = 'anon_old'",
                [ANON_TOKEN_TTL_SECS + 3600],
            )
            .unwrap();
        let expired = s.purge_expired(ANON_TOKEN_TTL_SECS);
        assert_eq!(expired, vec!["anon_old".to_string()]);
        assert!(!s.verify_token("anon_old", ""), "过期记录已删");
        assert!(s.uploader_ip("anon_fresh").is_some(), "新匿名记录保留");
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("access.db");
        let s = DocAccessStore::open_at(&db).unwrap();
        let t = s.issue("d1", Some("1.2.3.4"), true);
        drop(s);
        let reopened = DocAccessStore::open_at(&db).unwrap();
        assert!(reopened.verify_token("d1", &t), "重启后 token 仍有效");
        assert_eq!(reopened.uploader_ip("d1").as_deref(), Some("1.2.3.4"));
    }
}
