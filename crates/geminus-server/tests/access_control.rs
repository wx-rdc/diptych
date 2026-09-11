//! v2.4 T1 权限解耦集成测试：匿名配额内流畅试用 + access_token 防盗链三选一。
//!
//! 覆盖：token 正确放行 / 错误 token 403 / 跨 IP 403 / 同 IP 放行 /
//! 登录所有者放行 / 资源与原始文件直链防盗链 / 翻译请求体 token 校验。
//! 直连本机（无代理头）放行口径与旧有测试一致，不在此重复。

use std::sync::Arc;

use axum::routing::post as route_post;
use axum::Router;
use geminus_core::cache::TranslationCache;
use geminus_core::ApiConfig;
use geminus_server::auth::{AuthService, UserStore};
use geminus_server::{build_app, AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn pdf_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/pdf").join(name)
}

/// 预设用户 alice/secret123（2 次迭代 pbkdf2，测试提速）
fn test_auth() -> AuthService {
    let dir = tempfile::tempdir().unwrap();
    let users = format!(
        "{{\"users\":[{{\"username\":\"alice\",\"password_hash\":\"{}\"}}]}}",
        geminus_server::auth::encode_password_hash("secret123", 2)
    );
    let path = dir.path().join("users.json");
    std::fs::write(&path, users).unwrap();
    let store = UserStore::load(&path).unwrap();
    drop(dir);
    AuthService::new(store, std::time::Duration::from_secs(3600), false)
}

fn multipart_body(boundary: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{b}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"{f}\"\r\ncontent-type: application/octet-stream\r\n\r\n",
            b = boundary,
            f = filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{b}--\r\n", b = boundary).as_bytes());
    body
}

type App = Router;

async fn upload_pdf(app: &App, ip: &str, file: &str) -> (axum::http::StatusCode, serde_json::Value) {
    let boundary = "----geminus-access-test";
    let body = multipart_body(boundary, file, &std::fs::read(pdf_fixture(file)).unwrap());
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse?parser=builtin")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .header("x-real-ip", ip)
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, v)
}

async fn get_document(app: &App, doc_id: &str, headers: &[(&str, &str)]) -> axum::http::StatusCode {
    let mut builder = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/api/documents/{doc_id}"));
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let resp = app.clone().oneshot(builder.body(axum::body::Body::empty()).unwrap()).await.unwrap();
    resp.status()
}

async fn login_cookie(app: &App) -> String {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "username": "alice", "password": "secret123" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

fn anon_app() -> App {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3))
}

fn auth_app() -> App {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
            .with_auth(test_auth()),
    )
}

/// T1.2 + T1.6：匿名上传下发 access_token；token 正确 → 200，错误 token / 跨 IP → 403，
/// 同 IP 无 token → 200（uploader_ip 匹配）
#[tokio::test]
async fn anon_upload_token_grants_access() {
    let app = anon_app();
    let (status, v) = upload_pdf(&app, "198.51.100.1", "simple.pdf").await;
    assert_eq!(status, 200, "配额内匿名上传应放行: {v}");
    let doc_id = v["id"].as_str().unwrap().to_string();
    let token = v["access_token"]
        .as_str()
        .expect("上传响应应下发 access_token")
        .to_string();
    assert!(!token.is_empty());

    // ② token 匹配：换一个 IP 但带正确 token → 200
    let with_token = format!("token={token}");
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/documents/{doc_id}?{with_token}"))
                .header("x-real-ip", "198.51.100.2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "正确 token 跨 IP 应放行");

    // 错误 token → 403
    assert_eq!(
        get_document(&app, &doc_id, &[("x-real-ip", "198.51.100.2")]).await,
        403,
        "无 token 跨 IP → 403"
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/documents/{doc_id}?token=00000000-0000-4000-8000-000000000000"))
                .header("x-real-ip", "198.51.100.2")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "错误 token → 403");

    // ③ 同 IP 无 token → 200
    assert_eq!(
        get_document(&app, &doc_id, &[("x-real-ip", "198.51.100.1")]).await,
        200,
        "上传者 IP 匹配应放行"
    );
}

/// T1.6：登录用户上传 → Session 放行；匿名无 token → 403
#[tokio::test]
async fn logged_in_owner_session_grants_access() {
    let app = auth_app();
    let cookie = login_cookie(&app).await;
    let (status, v) = upload_pdf(&app, "203.0.113.9", "simple.pdf").await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    assert!(v["access_token"].as_str().is_some(), "登录上传也下发 token（分享可选）");

    assert_eq!(
        get_document(&app, &doc_id, &[("cookie", &cookie)]).await,
        200,
        "登录所有者 Session 放行"
    );
    assert_eq!(
        get_document(&app, &doc_id, &[("x-real-ip", "203.0.113.99")]).await,
        403,
        "匿名跨 IP → 403"
    );
}

/// T1.3 扩展：资源 / 原始 PDF 直链同样三选一（?token=）
#[tokio::test]
async fn resource_and_original_pdf_token_gated() {
    let app = anon_app();
    let (status, v) = upload_pdf(&app, "198.51.100.7", "simple.pdf").await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let token = v["access_token"].as_str().unwrap().to_string();

    for uri in [
        format!("/api/original/{doc_id}"),
        format!("/api/resource/{doc_id}/whatever"),
    ] {
        let resp = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri(&uri)
                    .header("x-real-ip", "198.51.100.8")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 403, "无 token 跨 IP 直链 {uri} → 403");

        let resp = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri(format!("{uri}?token={token}"))
                    .header("x-real-ip", "198.51.100.8")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            resp.status() != 403,
            "带 token 的直链 {uri} 应过防盗链（404 = 资源本身不存在，可接受）"
        );
    }
}

/// T1.4：POST /api/translate 请求体 access_token 校验（跨 IP 无 token → 403）
#[tokio::test]
async fn translate_body_token_gated() {
    let app = anon_app();
    let (status, v) = upload_pdf(&app, "198.51.100.20", "simple.pdf").await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let token = v["access_token"].as_str().unwrap().to_string();

    let post = |body: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/translate")
            .header("content-type", "application/json")
            .header("x-real-ip", "198.51.100.21")
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    // 跨 IP 无 token → 403（SSE 之前）
    let resp = app
        .clone()
        .oneshot(post(serde_json::json!({ "doc_id": doc_id }).to_string()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    // 带 token → 放行（LLM 不可达也无妨：状态码 200 SSE 已开流）
    let resp = app
        .clone()
        .oneshot(post(
            serde_json::json!({ "doc_id": doc_id, "access_token": token }).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "正确 token 翻译请求应放行");
}

/// T1：require_login=1 旧部署模式行为不变（匿名仍 401 收口）——回归保护
#[tokio::test]
async fn require_login_mode_still_gates_anonymous() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
            .with_auth(test_auth())
            .with_require_login(true),
    );
    let cookie = login_cookie(&app).await;
    let (status, v) = upload_pdf(&app, "203.0.113.30", "simple.pdf").await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();

    assert_eq!(
        get_document(&app, &doc_id, &[("x-real-ip", "203.0.113.31")]).await,
        401,
        "require_login 部署：匿名即使带 uploader_ip 也 401"
    );
    assert_eq!(
        get_document(&app, &doc_id, &[("cookie", &cookie)]).await,
        200,
        "require_login 部署：登录放行"
    );
}

/// T1.5 前置：查看 IR（GET /api/documents）不扣匿名配额——
/// 上传 1 页后同 IP 反复查看 10 次，配额仍只占 1 页（无新增扣减）
#[tokio::test]
async fn viewing_ir_does_not_consume_quota() {
    use geminus_server::quota::QuotaStore;
    let quota = QuotaStore::open_in_memory(5).unwrap();
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3).with_quota(quota),
    );
    let (status, v) = upload_pdf(&app, "198.51.100.40", "simple.pdf").await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    for _ in 0..10 {
        assert_eq!(
            get_document(&app, &doc_id, &[("x-real-ip", "198.51.100.40")]).await,
            200,
            "查看原文 IR 免费且不限次"
        );
    }
    // 配额只含上传扣减的 1 页（simple.pdf 单页）——同 IP 再上传 simple.pdf 已计费幂等
    let (status, _) = upload_pdf(&app, "198.51.100.40", "simple.pdf").await;
    assert_eq!(status, 200, "重复上传同内容不重复扣费");
}

/// 辅助：mock LLM（断点续传/熔断测试用占位；T5 复用此模式）
#[allow(dead_code)]
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
