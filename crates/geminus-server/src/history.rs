//! 用户历史记录（v2.4 T2）：登录用户上传/解析的文档关联表。
//! - 记录落 SQLite（默认 data/history.db，GEMINUS_HISTORY_DB 覆盖）；
//! - 单用户最多 100 条，超出按 uploaded_at FIFO 淘汰；
//! - find_doc 供 GET /api/documents 回源：服务重启后内存丢失 → 按 file_path
//!   重解析（调用方优先命中 MinerU 解析缓存，零云端调用）。

use std::path::Path;
use std::sync::Mutex;

/// 单用户历史上限（超出 FIFO 淘汰）
const MAX_PER_USER: i64 = 100;

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub doc_id: String,
    pub file_name: String,
    pub file_type: String,
    pub title: String,
    pub file_path: String,
    pub pages: i64,
    pub uploaded_at: i64,
    pub status: String,
}

impl HistoryEntry {
    /// handler 造行用便捷构造（uploaded_at 由 SQL 端 unixepoch() 填，此处占 0）
    pub fn new(doc_id: &str, file_name: &str, file_type: &str, title: &str, file_path: &str, pages: i64) -> Self {
        Self {
            doc_id: doc_id.to_string(),
            file_name: file_name.to_string(),
            file_type: file_type.to_string(),
            title: title.to_string(),
            file_path: file_path.to_string(),
            pages,
            uploaded_at: 0,
            status: "parsed".to_string(),
        }
    }
}

/// 用户文档历史仓（SQLite 持久）
pub struct HistoryStore {
    conn: Mutex<rusqlite::Connection>,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS user_documents (
    user_id TEXT NOT NULL,
    doc_id TEXT NOT NULL,
    file_name TEXT NOT NULL,
    file_type TEXT NOT NULL,
    title TEXT NOT NULL,
    file_path TEXT NOT NULL,
    pages INTEGER NOT NULL DEFAULT 0,
    uploaded_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'parsed',
    PRIMARY KEY (user_id, doc_id)
)";

impl HistoryStore {
    fn open(conn: rusqlite::Connection) -> Self {
        conn.execute_batch(SCHEMA).expect("历史表创建失败");
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

    /// 记录一条（INSERT OR IGNORE：回源重复注册不覆盖既有状态），随后 FIFO 淘汰
    pub fn insert(&self, user_id: &str, e: &HistoryEntry) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO user_documents
                (user_id, doc_id, file_name, file_type, title, file_path, pages, uploaded_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch(), ?8)",
            rusqlite::params![user_id, e.doc_id, e.file_name, e.file_type, e.title, e.file_path, e.pages, e.status],
        );
        let _ = conn.execute(
            "DELETE FROM user_documents WHERE user_id = ?1 AND doc_id NOT IN (
                SELECT doc_id FROM user_documents WHERE user_id = ?1
                ORDER BY uploaded_at DESC, rowid DESC LIMIT ?2)",
            rusqlite::params![user_id, MAX_PER_USER],
        );
    }

    /// 状态推进（parsed → translated）
    pub fn update_status(&self, user_id: &str, doc_id: &str, status: &str) {
        let _ = self.conn.lock().unwrap().execute(
            "UPDATE user_documents SET status = ?3 WHERE user_id = ?1 AND doc_id = ?2",
            rusqlite::params![user_id, doc_id, status],
        );
    }

    /// 用户历史（时间倒序，分页）
    pub fn list(&self, user_id: &str, limit: i64, offset: i64) -> Vec<HistoryEntry> {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare(
            "SELECT doc_id, file_name, file_type, title, file_path, pages, uploaded_at, status
             FROM user_documents WHERE user_id = ?1
             ORDER BY uploaded_at DESC, rowid DESC LIMIT ?2 OFFSET ?3") else { return Vec::new(); };
        let Ok(rows) = stmt.query_map(rusqlite::params![user_id, limit, offset], HistoryEntry::from_row)
        else { return Vec::new(); };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// doc_id → 落盘信息（GET /api/documents 回源用；同 doc 任意持有用户均可命中）
    pub fn find_doc(&self, doc_id: &str) -> Option<HistoryEntry> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT doc_id, file_name, file_type, title, file_path, pages, uploaded_at, status
                 FROM user_documents WHERE doc_id = ?1",
                [doc_id],
                HistoryEntry::from_row,
            )
            .ok()
    }
}

impl HistoryEntry {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            doc_id: r.get(0)?,
            file_name: r.get(1)?,
            file_type: r.get(2)?,
            title: r.get(3)?,
            file_path: r.get(4)?,
            pages: r.get(5)?,
            uploaded_at: r.get(6)?,
            status: r.get(7)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_list_desc_and_isolated_per_user() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert("alice", &HistoryEntry::new("d1", "a.pdf", "pdf", "A", "/tmp/a.pdf", 3));
        s.insert("alice", &HistoryEntry::new("d2", "b.docx", "docx", "B", "/tmp/b.docx", 5));
        s.insert("bob", &HistoryEntry::new("d3", "c.pdf", "pdf", "C", "/tmp/c.pdf", 2));
        let alice: Vec<String> = s.list("alice", 100, 0).iter().map(|e| e.doc_id.clone()).collect();
        assert_eq!(alice.len(), 2);
        assert_eq!(alice[0], "d2", "时间倒序（后插在前）");
        assert_eq!(s.list("bob", 100, 0).len(), 1, "用户隔离");
    }

    #[test]
    fn insert_is_idempotent_and_keeps_first_status() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert("alice", &HistoryEntry::new("d1", "a.pdf", "pdf", "A", "/tmp/a.pdf", 3));
        s.update_status("alice", "d1", "translated");
        // 回源场景：重新 register 不应把 status 打回 parsed
        s.insert("alice", &HistoryEntry::new("d1", "a.pdf", "pdf", "A", "/tmp/a.pdf", 3));
        let list = s.list("alice", 100, 0);
        assert_eq!(list.len(), 1, "(user,doc) 主键去重");
        assert_eq!(list[0].status, "translated");
    }

    #[test]
    fn fifo_eviction_keeps_latest_100() {
        let s = HistoryStore::open_in_memory().unwrap();
        for i in 0..105 {
            s.insert("alice", &HistoryEntry::new(&format!("d{i:03}"), "f.pdf", "pdf", "T", "/tmp/f.pdf", 1));
            std::thread::sleep(std::time::Duration::from_millis(2)); // uploaded_at 秒级粒度 → 靠 rowid 决胜
        }
        let list = s.list("alice", 200, 0);
        assert_eq!(list.len(), 100, "上限 100 条");
        assert_eq!(list[0].doc_id, "d104", "最新在前");
        assert_eq!(list[99].doc_id, "d005", "最老的 d000..d004 被淘汰");
    }

    #[test]
    fn update_status_only_touches_target_row() {
        let s = HistoryStore::open_in_memory().unwrap();
        s.insert("alice", &HistoryEntry::new("d1", "a.pdf", "pdf", "A", "/tmp/a.pdf", 3));
        s.insert("alice", &HistoryEntry::new("d2", "b.pdf", "pdf", "B", "/tmp/b.pdf", 3));
        s.update_status("alice", "d2", "translated");
        let m: Vec<(String, String)> = s.list("alice", 100, 0).into_iter().map(|e| (e.doc_id, e.status)).collect();
        assert!(m.contains(&("d2".to_string(), "translated".to_string())));
        assert!(m.contains(&("d1".to_string(), "parsed".to_string())));
    }

    #[test]
    fn find_doc_any_user_and_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.db");
        let s = HistoryStore::open_at(&db).unwrap();
        s.insert("alice", &HistoryEntry::new("d1", "a.pdf", "pdf", "A", "/tmp/a.pdf", 3));
        drop(s);
        let reopened = HistoryStore::open_at(&db).unwrap();
        let e = reopened.find_doc("d1").expect("doc_id 回源命中");
        assert_eq!((e.file_path.as_str(), e.title.as_str()), ("/tmp/a.pdf", "A"));
        assert!(reopened.find_doc("nope").is_none());
    }
}
