//! v2.4 权限收口 + 历史记录 HTTP 集成测试：
//! require_login=1 时匿名可上传（外壳无 IR）但查看/翻译 401；登录后放行；
//! /api/history 按用户隔离；GET /api/documents 支持重启后回源重解析。

use std::sync::Arc;

use axum::routing::post as route_post;
use axum::Router;
use geminus_core::cache::TranslationCache;
use geminus_core::ApiConfig;
use geminus_server::auth::{AuthService, UserStore};
use geminus_server::history::HistoryStore;
use geminus_server::{build_app, AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/rich.docx")
}

async fn spawn_mock_llm() -> String {
    let router = Router::new().route(
        "/chat/completions",
        route_post(|body: axum::Json<serde_json::Value>| async move {
            let user = body.0["messages"]
                .as_array()
                .and_then(|ms| ms.last().cloned())
                .and_then(|m| m["content"].as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            axum::Json(serde_json::json!({
                "choices": [{"message": {"content": format!("【译】{user}")}, "finish_reason": "stop"}]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    format!("http://{addr}")
}

/// 预设用户 alice/secret123 的认证服务（2 次迭代 pbkdf2，测试提速）
fn test_auth() -> AuthService {
    let dir = tempfile::tempdir().unwrap();
    let users = format!(
        "{{\"users\":[{{\"username\":\"alice\",\"password_hash\":\"{}\"}}]}}",
        geminus_server::auth::encode_password_hash("secret123", 2)
    );
    let path = dir.path().join("users.json");
    std::fs::write(&path, users).unwrap();
    let store = UserStore::load(&path).unwrap();
    drop(dir); // UserStore 是进程内快照，文件用后即弃
    AuthService::new(store, std::time::Duration::from_secs(3600), false)
}

fn make_state(require_login: bool, history: Arc<HistoryStore>) -> AppState {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_auth(test_auth())
        .with_history(history)
        .with_require_login(require_login)
}

async fn login_cookie(app: &Router, username: &str, password: &str) -> String {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "username": username, "password": password }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

fn parse_req(cookie: Option<&str>) -> axum::http::Request<axum::body::Body> {
    let mut b = axum::http::Request::builder()
        .method("POST")
        .uri("/api/parse")
        .header("content-type", "application/json");
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    b.body(axum::body::Body::from(
        serde_json::json!({ "file_path": fixture_path() }).to_string(),
    ))
    .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn get_req(uri: String, cookie: Option<&str>) -> axum::http::Request<axum::body::Body> {
    let mut b = axum::http::Request::builder().uri(uri);
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    b.body(axum::body::Body::empty()).unwrap()
}

#[tokio::test]
async fn anonymous_parse_under_require_login_returns_shell_without_ir() {
    let history = Arc::new(HistoryStore::open_in_memory().unwrap());
    let app = build_app(make_state(true, history));
    let resp = app.oneshot(parse_req(None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let v = body_json(resp).await;
    assert!(v["id"].as_str().is_some(), "外壳仍返回 doc_id: {v}");
    assert!(
        v.get("blocks").is_none() || v["blocks"].is_null(),
        "匿名不得返回 IR blocks: {v}"
    );
    assert!(v.get("resources").is_none(), "匿名不得返回 resources");
}

#[tokio::test]
async fn anonymous_document_and_translate_are_401_but_logged_in_ok() {
    let mock = spawn_mock_llm().await;
    let history = Arc::new(HistoryStore::open_in_memory().unwrap());
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state = AppState::new(cache, ApiConfig::new(&mock, "k", "m"), 3)
        .with_auth(test_auth())
        .with_history(history)
        .with_require_login(true);
    let app = build_app(state);
    let cookie = login_cookie(&app, "alice", "secret123").await;

    // 登录解析 → 拿 doc_id（完整 IR）
    let resp = app.clone().oneshot(parse_req(Some(&cookie))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let doc = body_json(resp).await;
    assert!(!doc["blocks"].as_array().unwrap().is_empty());
    let doc_id = doc["id"].as_str().unwrap().to_string();

    // 匿名查看原文档 → 401；登录查看 → 200
    let resp = app
        .clone()
        .oneshot(get_req(format!("/api/documents/{doc_id}"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let resp = app
        .clone()
        .oneshot(get_req(format!("/api/documents/{doc_id}"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 匿名翻译 → 401
    let translate = |cookie: Option<&str>| {
        let mut b = axum::http::Request::builder()
            .method("POST")
            .uri("/api/translate")
            .header("content-type", "application/json");
        if let Some(c) = cookie {
            b = b.header("cookie", c);
        }
        b.body(axum::body::Body::from(
            serde_json::json!({ "doc_id": doc_id }).to_string(),
        ))
        .unwrap()
    };
    let resp = app.clone().oneshot(translate(None)).await.unwrap();
    assert_eq!(resp.status(), 401);
    let resp = app.clone().oneshot(translate(Some(&cookie))).await.unwrap();
    assert_eq!(resp.status(), 200, "登录翻译 SSE 正常建立");

    // 匿名单段重译 → 401（门禁先于块存在性检查）
    let para = axum::http::Request::builder()
        .method("POST")
        .uri("/api/translate/paragraph")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "doc_id": doc_id, "block_id": "nope" }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(para).await.unwrap();
    assert_eq!(resp.status(), 401);

    // 匿名取原始 PDF → 401
    let resp = app
        .clone()
        .oneshot(get_req(format!("/api/original/{doc_id}"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn require_login_off_keeps_legacy_anonymous_behavior() {
    let history = Arc::new(HistoryStore::open_in_memory().unwrap());
    let app = build_app(make_state(false, history));
    let resp = app.oneshot(parse_req(None)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let v = body_json(resp).await;
    assert!(
        v["blocks"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "require_login=0 匿名仍得全量 IR"
    );
}

#[tokio::test]
async fn history_records_and_isolation_and_translate_status() {
    let mock = spawn_mock_llm().await;
    let history = Arc::new(HistoryStore::open_in_memory().unwrap());
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state = AppState::new(cache, ApiConfig::new(&mock, "k", "m"), 3)
        .with_auth(test_auth())
        .with_history(history.clone())
        .with_require_login(false);
    let app = build_app(state);
    let cookie = login_cookie(&app, "alice", "secret123").await;

    // 未登录 → /api/history 401（与 require_login 无关：历史是用户数据）
    let resp = app
        .clone()
        .oneshot(get_req("/api/history".to_string(), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 登录解析（rich.docx）→ 历史落一条 parsed
    let resp = app.clone().oneshot(parse_req(Some(&cookie))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let doc = body_json(resp).await;
    let doc_id = doc["id"].as_str().unwrap().to_string();

    let resp = app
        .clone()
        .oneshot(get_req("/api/history".to_string(), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let h = body_json(resp).await;
    let items = h["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["doc_id"].as_str().unwrap(), doc_id);
    assert_eq!(items[0]["file_type"].as_str().unwrap(), "docx");
    assert_eq!(items[0]["status"].as_str().unwrap(), "parsed");
    assert!(
        items[0]["file_name"].as_str().unwrap().contains("rich"),
        "上传文件名入历史: {}",
        items[0]["file_name"]
    );

    // 翻译完成 → 状态推进 translated（SSE 流读到 complete）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("event: complete"), "SSE: {body}");

    let resp = app
        .clone()
        .oneshot(get_req("/api/history".to_string(), Some(&cookie)))
        .await
        .unwrap();
    let h = body_json(resp).await;
    assert_eq!(h["items"][0]["status"].as_str().unwrap(), "translated");
}

#[tokio::test]
async fn document_reload_from_disk_after_memory_loss() {
    // 同一磁盘 history db + 同一 uploads 文件，两个 AppState 模拟「服务重启」
    let dir = tempfile::tempdir().unwrap();
    let history_db = dir.path().join("history.db");
    let history = Arc::new(HistoryStore::open_at(&history_db).unwrap());
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state1 = AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_auth(test_auth())
        .with_history(history.clone())
        .with_require_login(false);
    let app1 = build_app(state1);
    let cookie = login_cookie(&app1, "alice", "secret123").await;
    let resp = app1.oneshot(parse_req(Some(&cookie))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let doc = body_json(resp).await;
    let doc_id = doc["id"].as_str().unwrap().to_string();

    // 「重启」：全新 AppState（内存 docs 为空），history db 指向同一磁盘文件
    let history2 = Arc::new(HistoryStore::open_at(&history_db).unwrap());
    let cache2 = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state2 = AppState::new(cache2, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_auth(test_auth())
        .with_history(history2)
        .with_require_login(false);
    let app2 = build_app(state2);
    let resp = app2
        .oneshot(get_req(format!("/api/documents/{doc_id}"), Some(&cookie)))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "回源重解析成功");
    let v = body_json(resp).await;
    assert!(v["blocks"].as_array().map(|a| !a.is_empty()).unwrap_or(false));
}
