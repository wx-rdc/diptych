//! HTTP 集成测试（任务 T8）：/api/parse、/api/translate SSE、/api/resource
//! LLM 上游用测试内 mock（axum 随机端口），通过 AppState 注入。

use std::sync::Arc;

use axum::routing::post as route_post;
use axum::Router;
use geminus_core::cache::TranslationCache;
use geminus_server::{build_app, AppState};
use geminus_core::ApiConfig;
use http_body_util::BodyExt;
use tower::ServiceExt;

type App = axum::Router;

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/rich.docx")
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
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn app_with_mock(mock_base: &str) -> App {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let cfg = ApiConfig::new(mock_base, "sk-test", "mock-model");
    build_app(AppState::new(cache, cfg, 3))
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn health_returns_ok() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    assert!(body.contains("\"ok\""), "body: {body}");
}

#[tokio::test]
async fn parse_returns_document() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": fixture_path() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["blocks"].as_array().map(|a| !a.is_empty()).unwrap_or(false));
    assert!(v["resources"].as_object().map(|o| !o.is_empty()).unwrap_or(false));
    assert!(v["id"].as_str().is_some());
}

#[tokio::test]
async fn parse_missing_file_is_4xx() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": "/nonexistent/x.docx" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body = body_string(resp).await;
    assert!(body.contains("error"), "body: {body}");
}

#[tokio::test]
async fn translate_streams_sse_progress_then_complete() {
    let mock = spawn_mock_llm().await;
    let app = app_with_mock(&mock);

    // 1. parse
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": fixture_path() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let doc_id = doc["id"].as_str().unwrap().to_string();
    let block_count = doc["blocks"].as_array().unwrap().len();

    // 2. translate → SSE
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "source_lang": "en", "target_lang": "zh" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false));

    let body = body_string(resp).await;
    assert!(body.contains("event: progress"), "SSE 应含 progress 事件");
    assert!(body.contains("event: complete"), "SSE 应以 complete 结束");

    // complete 事件的 data 是 TranslatedDocument JSON
    let complete_data = body
        .split("event: complete")
        .nth(1)
        .and_then(|tail| tail.split("data:").nth(1))
        .and_then(|d| d.split("\n\n").next())
        .expect("complete data 应存在");
    let td: serde_json::Value = serde_json::from_str(complete_data.trim()).unwrap();
    assert_eq!(td["pairs"].as_array().unwrap().len(), block_count);
    assert!(td["pairs"].as_array().unwrap().iter().any(|p| !p["translated"].is_null()));

    // progress 事件中应出现 done 状态
    assert!(body.contains("\"status\":\"done\""), "应有 done 状态");
}

#[tokio::test]
async fn translate_unknown_doc_is_404() {
    let mock = spawn_mock_llm().await;
    let app = app_with_mock(&mock);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": "nope" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn resource_returns_binary() {
    let mock = spawn_mock_llm().await;
    let app = app_with_mock(&mock);

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": fixture_path() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let doc: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let doc_id = doc["id"].as_str().unwrap().to_string();

    // 找一个 image 资源
    let rid = doc["resources"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, v)| v["kind"] == "image")
        .map(|(k, _)| k.clone())
        .expect("rich.docx 应有图片资源");

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/resource/{doc_id}/{rid}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap().to_str().unwrap(),
        "image/png"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..4], b"\x89PNG");

    // 未知资源 404
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/resource/{doc_id}/r99_image"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ────────────────────────── PDF 支持（v1_plan T8/T9）─────────────────────────

fn pdf_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/pdf")
        .join(name)
}

async fn parse_file(app: &App, path: &str) -> (axum::http::StatusCode, serde_json::Value) {
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": path }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap_or(serde_json::Value::Null);
    (status, v)
}

#[tokio::test]
async fn parse_pdf_returns_document() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let (status, v) = parse_file(&app, &pdf_fixture("simple.pdf").to_string_lossy()).await;
    assert_eq!(status, 200);
    assert_eq!(v["blocks"].as_array().map(|a| a.len()), Some(5), "simple.pdf 5 段");
    assert!(v["id"].as_str().is_some());
    assert_eq!(v["title"], "simple");
}

#[tokio::test]
async fn parse_pdf_with_images_serves_resources() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let (status, v) = parse_file(&app, &pdf_fixture("with_image.pdf").to_string_lossy()).await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    // 图片资源可经 /api/resource 取回 JPEG 原始字节
    let rid = v["resources"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, r)| r["kind"] == "image")
        .map(|(k, _)| k.clone())
        .expect("应有图片资源");
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/resource/{doc_id}/{rid}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers().get("content-type").unwrap().to_str().unwrap(), "image/jpeg");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..3], b"\xff\xd8\xff", "JPEG 魔数");
}

#[tokio::test]
async fn parse_unsupported_extension_is_400() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("note.txt");
    std::fs::write(&txt, b"plain text").unwrap();
    let (status, v) = parse_file(&app, &txt.to_string_lossy()).await;
    assert_eq!(status, 400, "不支持的扩展名应 400: {v}");
    assert!(v["error"].as_str().unwrap().contains("不支持"));
}

#[tokio::test]
async fn parse_mismatched_magic_bytes_is_400() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("fake.pdf");
    std::fs::write(&fake, b"PK\x03\x04 not a pdf").unwrap(); // ZIP 头伪装 .pdf
    let (status, v) = parse_file(&app, &fake.to_string_lossy()).await;
    assert_eq!(status, 400, "magic 不符应 400: {v}");
}

#[tokio::test]
async fn parse_empty_pdf_is_200_with_empty_blocks() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let (status, v) = parse_file(&app, &pdf_fixture("empty.pdf").to_string_lossy()).await;
    assert_eq!(status, 200);
    assert_eq!(v["blocks"].as_array().map(|a| a.len()), Some(0));
}

#[tokio::test]
async fn parse_corrupt_pdf_is_422_not_panic() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("broken.pdf");
    std::fs::write(&bad, b"%PDF-1.4 garbage garbage").unwrap();
    let (status, v) = parse_file(&app, &bad.to_string_lossy()).await;
    assert_eq!(status, 422, "损坏文件应 422: {v}");
}

#[tokio::test]
async fn translate_pdf_streams_sse_end_to_end() {
    // T8.3：/api/translate 对 PDF IR 无需修改即可工作
    let mock = spawn_mock_llm().await;
    let app = app_with_mock(&mock);
    let (status, v) = parse_file(&app, &pdf_fixture("two_column.pdf").to_string_lossy()).await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let block_count = v["blocks"].as_array().unwrap().len();
    assert!(block_count > 10, "two_column.pdf 块数: {block_count}");

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "source_lang": "en", "target_lang": "zh" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = body_string(resp).await;
    assert!(body.contains("event: progress"));
    assert!(body.contains("event: complete"));
    let complete_data = body
        .split("event: complete")
        .nth(1)
        .and_then(|t| t.split("data:").nth(1))
        .and_then(|d| d.split("\n\n").next())
        .unwrap();
    let td: serde_json::Value = serde_json::from_str(complete_data.trim()).unwrap();
    assert_eq!(td["pairs"].as_array().unwrap().len(), block_count, "PDF 块与译文对齐");
}

// ══════════════ v2：MinerU SSE parse / original / config（T8） ══════════════

/// MinerU 上游 mock：签名上传 → PUT → 轮询 done → ZIP 下载（全部地址可达）。
/// 先绑定端口取得自身地址，再构造返回该地址的路由。
async fn spawn_mock_mineru() -> geminus_core::mineru::MineruClient {
    use axum::routing::{get, put};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let polled = Arc::new(AtomicBool::new(false));
    let zip_bytes = {
        use std::io::Write as _;
        let buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(buf);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/mineru/simple");
        w.start_file("content_list.json", opts).unwrap();
        w.write_all(std::fs::read(base.join("content_list.json")).unwrap().as_slice()).unwrap();
        w.start_file("layout.json", opts).unwrap();
        w.write_all(std::fs::read(base.join("layout.json")).unwrap().as_slice()).unwrap();
        w.finish().unwrap().into_inner()
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_addr = format!("http://{}", listener.local_addr().unwrap());
    let oss_url = format!("{base_addr}/oss.pdf");
    let zip_url = format!("{base_addr}/result.zip");

    let p = polled.clone();
    let router = Router::new()
        .route(
            "/api/v4/file-urls/batch",
            route_post(move || {
                let oss_url = oss_url.clone();
                async move {
                    axum::Json(serde_json::json!({"code": 0, "data": {"batch_id": "b1",
                        "file_urls": [oss_url]}}))
                }
            }),
        )
        .route("/oss.pdf", put(|| async { "ok" }))
        .route(
            "/api/v4/extract-results/batch/:batch_id",
            get(move || {
                let p = p.clone();
                let zip_url = zip_url.clone();
                async move {
                    let done = p.swap(true, Ordering::SeqCst);
                    if done {
                        axum::Json(serde_json::json!({"code": 0, "data": {"extract_result": [
                            {"file_name": "simple_chunk_00.pdf", "state": "done",
                             "full_zip_url": zip_url}]}}))
                    } else {
                        axum::Json(serde_json::json!({"code": 0, "data": {"extract_result": [
                            {"file_name": "simple_chunk_00.pdf", "state": "running",
                             "extract_progress": {"extracted_pages": 0, "total_pages": 1}}]}}))
                    }
                }
            }),
        )
        .route(
            "/result.zip",
            get(move || {
                let z = zip_bytes.clone();
                async move { ([(axum::http::header::CONTENT_TYPE, "application/zip")], z) }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    geminus_core::mineru::MineruClient::new(format!("{base_addr}/api/v4"), "tok".into())
}

/// SSE 解析辅助：收集全部事件 (event, data)
async fn collect_sse_events(resp: axum::response::Response) -> Vec<(String, String)> {
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    let mut events = Vec::new();
    for block in text.split("\n\n") {
        let mut name = None;
        let mut data = String::new();
        for line in block.lines() {
            if let Some(v) = line.strip_prefix("event: ") {
                name = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("data: ") {
                data.push_str(v);
            }
        }
        if let Some(name) = name {
            events.push((name, data));
        }
    }
    events
}

#[tokio::test]
async fn parse_builtin_keeps_v1_sync_json() {
    // parser=builtin + PDF → 同步 JSON（v1 行为）
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "builtin"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().get("content-type").unwrap().to_str().unwrap().contains("application/json"));
    let doc: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(doc["blocks"].as_array().unwrap().len() >= 3);
    assert!(doc.get("render_meta").is_some());
}

#[tokio::test]
async fn parse_mineru_streams_progress_and_done() {
    let client = spawn_mock_mineru().await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state = AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_mineru(Some(client));
    let app = build_app(state);

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "mineru"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let events = collect_sse_events(resp).await;
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"progress"), "{names:?}");
    assert!(names.contains(&"done"), "{names:?}");
    // done 事件是完整 IR：pdf_meta + render_meta 齐备
    let done = events.iter().find(|(n, _)| n == "done").unwrap().1.clone();
    let doc: serde_json::Value = serde_json::from_str(&done).unwrap();
    assert!(doc["pdf_meta"]["page_count"].as_u64().unwrap() >= 1, "{done}");
    assert!(doc["render_meta"].as_object().unwrap().len() >= 3);
    // 进度含阶段事件
    let stages: Vec<String> = events
        .iter()
        .filter(|(n, _)| n == "progress")
        .filter_map(|(_, d)| serde_json::from_str::<serde_json::Value>(d).ok())
        .filter_map(|v| v["stage"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(stages.iter().any(|s| s == "splitting"), "{stages:?}");
    assert!(stages.iter().any(|s| s == "parsing"), "{stages:?}");
    assert!(stages.iter().any(|s| s == "merging"), "{stages:?}");
    // 文档已注册：doc_id 可用于 /api/translate
    let doc_id = doc["id"].as_str().unwrap().to_string();
    assert!(!doc_id.is_empty());
}

#[tokio::test]
async fn parse_mineru_cached_second_time() {
    let client = spawn_mock_mineru().await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state = AppState::new(cache.clone(), ApiConfig::new("http://unused", "k", "m"), 3)
        .with_mineru(Some(client));
    let app = build_app(state);
    let req = || {
        axum::http::Request::builder()
            .method("POST")
            .uri("/api/parse")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "mineru"}).to_string(),
            ))
            .unwrap()
    };
    // 两次：第二次应命中缓存（stage=cached，且无 splitting——不走网络）
    let resp = app.clone().oneshot(req()).await.unwrap();
    let events1 = collect_sse_events(resp).await;
    assert!(events1.iter().any(|(n, d)| n == "done" && !d.is_empty()));
    // 在同一个 app（同 state）上再请求
    let resp = app.clone().oneshot(req()).await.unwrap();
    let events2 = collect_sse_events(resp).await;
    let stages: Vec<String> = events2
        .iter()
        .filter(|(n, _)| n == "progress")
        .filter_map(|(_, d)| serde_json::from_str::<serde_json::Value>(d).ok())
        .filter_map(|v| v["stage"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(stages.iter().any(|s| s == "cached"), "{stages:?}");
    assert!(!stages.iter().any(|s| s == "splitting"), "缓存命中不应再拆分上传: {stages:?}");
}

#[tokio::test]
async fn coverage_endpoint_reports_after_mineru_parse() {
    // v2.2 T1：MinerU 解析 → raw 持久化 → /api/debug/coverage 返回三层度量
    let client = spawn_mock_mineru().await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let raw_root = tempfile::tempdir().unwrap();
    let state = AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_mineru(Some(client))
        .with_mineru_cache_root(raw_root.path().to_path_buf());
    let app = build_app(state);

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "mineru"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let events = collect_sse_events(resp).await;
    let done = events.iter().find(|(n, _)| n == "done").unwrap().1.clone();
    let doc: serde_json::Value = serde_json::from_str(&done).unwrap();
    let doc_id = doc["id"].as_str().unwrap().to_string();

    // raw 已持久化（mock 输出为 v1 格式 → 全 text 条目，审计零违规）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/api/debug/coverage?doc_id={doc_id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(v["per_type"].as_array().unwrap().iter()
        .any(|t| t["kind"] == "text" && t["mineru_count"].as_u64().unwrap() >= 3), "{v}");
    assert_eq!(v["violations"].as_array().unwrap().len(), 0, "{v}");
    assert!(v["char_coverage"].as_f64().unwrap() >= 0.98, "{v}");

    // 未知 doc_id → 404；缺参数 → 400
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/api/debug/coverage?doc_id=nope")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/api/debug/coverage")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn parse_auto_falls_back_to_builtin_on_mineru_auth_failure() {
    // mock 上游一律 401 A0202 → auto 模式回退 builtin → done（无 render_meta）
    use axum::routing::post;
    let router = Router::new().route(
        "/api/v4/file-urls/batch",
        post(|| async {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({"msgCode": "A0202", "msg": "auth failed"})),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    let client = geminus_core::mineru::MineruClient::new(format!("http://{addr}/api/v4"), "bad".into());

    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state = AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_mineru(Some(client));
    let app = build_app(state);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "auto"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let events = collect_sse_events(resp).await;
    let stages: Vec<String> = events
        .iter()
        .filter(|(n, _)| n == "progress")
        .filter_map(|(_, d)| serde_json::from_str::<serde_json::Value>(d).ok())
        .filter_map(|v| v["stage"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(stages.iter().any(|s| s == "fallback"), "{stages:?}");
    let done = events.iter().find(|(n, _)| n == "done").expect("回退后应 done");
    let doc: serde_json::Value = serde_json::from_str(&done.1).unwrap();
    assert!(doc["blocks"].as_array().unwrap().len() >= 3, "builtin 解析结果");
}

#[tokio::test]
async fn parse_mineru_error_when_no_fallback() {
    use axum::routing::post;
    let router = Router::new().route(
        "/api/v4/file-urls/batch",
        post(|| async {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({"msgCode": "A0202", "msg": "auth failed"})),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    let client = geminus_core::mineru::MineruClient::new(format!("http://{addr}/api/v4"), "bad".into());

    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let state = AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
        .with_mineru(Some(client));
    let app = build_app(state);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "mineru"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let events = collect_sse_events(resp).await;
    let err = events.iter().find(|(n, _)| n == "error").expect("mineru 模式失败应 error 事件");
    let payload: serde_json::Value = serde_json::from_str(&err.1).unwrap();
    assert_eq!(payload["code"], "MINERU_AUTH");
}

#[tokio::test]
async fn original_pdf_served_after_parse() {
    // builtin 解析（PDF）→ /api/original/{doc_id} 返回 %PDF 字节
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"file_path": pdf_fixture("simple.pdf"), "parser": "builtin"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let doc: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let doc_id = doc["id"].as_str().unwrap().to_string();

    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/original/{doc_id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers().get("content-type").unwrap(), "application/pdf");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"%PDF"), "应为 PDF 魔数");

    // 未知 doc_id → 404
    let app2 = build_app(AppState::new(
        Arc::new(TranslationCache::open_in_memory().unwrap()),
        ApiConfig::new("http://unused", "k", "m"),
        3,
    ));
    let resp = app2
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/original/nonexistent")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn mineru_config_endpoints() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
            .with_mineru(None),
    );
    // 未配置
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/config/mineru")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(v["configured"], false);

    // 未登录 POST token → 401（运行时配置属管理操作）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/config/mineru")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"token": "sk-new"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // 登录后 POST → configured
    let (app, cookie) = app_with_users().await;
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/config/mineru")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .body(axum::body::Body::from(r#"{"token": "sk-new"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(v["configured"], true);
}

// ══════════════ 部署安全加固：认证 / 匿名限流 / multipart 上传 ══════════════

/// 构造带预设用户（alice / secret123）的应用，返回 (app, 登录 Cookie)
async fn app_with_users() -> (App, String) {
    let dir = tempfile::tempdir().unwrap();
    let users_json = dir.path().join("users.json");
    std::fs::write(
        &users_json,
        serde_json::json!({
            "users": [
                { "username": "alice", "password_hash": geminus_server::auth::encode_password_hash("secret123", 2) },
                { "username": "bob",   "password_hash": geminus_server::auth::encode_password_hash("bob-pw", 2) }
            ]
        })
        .to_string(),
    )
    .unwrap();
    let users = geminus_server::auth::UserStore::load(&users_json).unwrap();
    let auth = geminus_server::auth::AuthService::new(
        users,
        std::time::Duration::from_secs(3600),
        false,
    );
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3).with_auth(auth),
    );
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
    let cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    (app, cookie)
}

async fn post_json(
    app: &App,
    uri: &str,
    body: serde_json::Value,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    app.clone()
        .oneshot(builder.body(axum::body::Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn auth_login_me_logout_flow() {
    let (app, cookie) = app_with_users().await;

    // me（带会话）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/auth/me")
                .header("cookie", &cookie)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(v["authenticated"], true);
    assert_eq!(v["username"], "alice");

    // me（匿名）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/auth/me")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(v["authenticated"], false);

    // 错误口令 → 401
    let resp = post_json(
        &app,
        "/api/auth/login",
        serde_json::json!({ "username": "alice", "password": "wrong" }),
        &[("x-real-ip", "203.0.113.9")],
    )
    .await;
    assert_eq!(resp.status(), 401);

    // logout → 会话失效
    let resp = post_json(&app, "/api/auth/logout", serde_json::json!({}), &[("cookie", &cookie)]).await;
    assert_eq!(resp.status(), 200);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/auth/me")
                .header("cookie", &cookie)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(v["authenticated"], false);
}

#[tokio::test]
async fn login_bruteforce_throttled() {
    let (app, _) = app_with_users().await;
    for i in 0..5 {
        let resp = post_json(
            &app,
            "/api/auth/login",
            serde_json::json!({ "username": "bob", "password": format!("guess{i}") }),
            &[("x-real-ip", "198.51.100.7")],
        )
        .await;
        assert_eq!(resp.status(), 401);
    }
    // 第 6 次即使口令正确也被限速 → 429
    let resp = post_json(
        &app,
        "/api/auth/login",
        serde_json::json!({ "username": "bob", "password": "bob-pw" }),
        &[("x-real-ip", "198.51.100.7")],
    )
    .await;
    assert_eq!(resp.status(), 429);
}

#[tokio::test]
async fn anon_quota_charges_pages_and_blocks() {
    use geminus_server::quota::QuotaStore;
    // 限额 1：simple.pdf（1 页）解析后即满——后续 large/two_column 预检 429 的前提
    let quota = QuotaStore::open_in_memory(1).unwrap();
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3).with_quota(quota),
    );

    // 匿名（X-Real-IP）解析 1 页 simple.pdf → 计 1 页
    let (status, v) = parse_file(&app, &pdf_fixture("simple.pdf").to_string_lossy()).await;
    assert_eq!(status, 200, "{v}");
    // 注意：parse_file 不带代理头（本地直连）→ 不计费；改用带 X-Real-IP 的请求
    let resp = post_json(
        &app,
        "/api/parse",
        serde_json::json!({ "file_path": pdf_fixture("simple.pdf").to_string_lossy(), "parser": "builtin" }),
        &[("x-real-ip", "203.0.113.1")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let doc_id = doc["id"].as_str().unwrap().to_string();

    // 同 IP 翻译同一文档 → 幂等不再扣页（若重复扣会 429）
    let resp = post_json(
        &app,
        "/api/translate",
        serde_json::json!({ "doc_id": doc_id }),
        &[("x-real-ip", "203.0.113.1")],
    )
    .await;
    assert_eq!(resp.status(), 200, "同文档翻译不应重复计费");
    drop(body_string(resp).await);

    // 再解析 100 页 large.pdf → 预检超限 → 429（不消耗 MinerU/解析资源）
    let resp = post_json(
        &app,
        "/api/parse",
        serde_json::json!({ "file_path": pdf_fixture("large.pdf").to_string_lossy(), "parser": "builtin" }),
        &[("x-real-ip", "203.0.113.1")],
    )
    .await;
    assert_eq!(resp.status(), 429);
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(v["error"].as_str().unwrap().contains("请登录"));

    // 已用满额度后再请求新文档 → 429
    let resp = post_json(
        &app,
        "/api/parse",
        serde_json::json!({ "file_path": pdf_fixture("two_column.pdf").to_string_lossy(), "parser": "builtin" }),
        &[("x-real-ip", "203.0.113.1")],
    )
    .await;
    assert_eq!(resp.status(), 429);

    // 其他 IP 不受影响（fresh 额度解析 1 页 simple.pdf）；登录用户不受限（见 logged_in_user_unlimited）
    let resp = post_json(
        &app,
        "/api/parse",
        serde_json::json!({ "file_path": pdf_fixture("simple.pdf").to_string_lossy(), "parser": "builtin" }),
        &[("x-real-ip", "203.0.113.2")],
    )
    .await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn logged_in_user_unlimited() {
    use geminus_server::quota::QuotaStore;
    let quota = QuotaStore::open_in_memory(2).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let users_json = dir.path().join("users.json");
    std::fs::write(
        &users_json,
        serde_json::json!({ "users": [
            { "username": "alice", "password_hash": geminus_server::auth::encode_password_hash("pw", 2) }
        ]})
        .to_string(),
    )
    .unwrap();
    let auth = geminus_server::auth::AuthService::new(
        geminus_server::auth::UserStore::load(&users_json).unwrap(),
        std::time::Duration::from_secs(3600),
        false,
    );
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
            .with_quota(quota)
            .with_auth(auth),
    );

    // 登录
    let resp = post_json(
        &app,
        "/api/auth/login",
        serde_json::json!({ "username": "alice", "password": "pw" }),
        &[("x-real-ip", "203.0.113.5")],
    )
    .await;
    assert_eq!(resp.status(), 200);
    let cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    // 额度只有 2 页，但登录用户连解析 3+2+100 页都放行
    for f in ["simple.pdf", "headings.pdf", "large.pdf"] {
        let resp = post_json(
            &app,
            "/api/parse",
            serde_json::json!({ "file_path": pdf_fixture(f).to_string_lossy(), "parser": "builtin" }),
            &[("x-real-ip", "203.0.113.5"), ("cookie", &cookie)],
        )
        .await;
        assert_eq!(resp.status(), 200, "登录用户 {f} 不应受限");
        drop(body_string(resp).await);
    }
}

#[tokio::test]
async fn anon_paragraph_retranslate_gated() {
    use geminus_server::quota::QuotaStore;
    let quota = QuotaStore::open_in_memory(5).unwrap();
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3).with_quota(quota),
    );

    // 匿名解析 rich.docx（本地直连，不计费）→ 文档不在该 IP 额度内。
    // v2.4 T1：防盗链三选一优先于配额门控——陌生 IP 遍历 doc_id → 403（旧断言 429）
    let (status, v) = parse_file(&app, &fixture_path().to_string_lossy()).await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let block_id = v["blocks"][0]["id"].as_str().unwrap().to_string();

    let resp = post_json(
        &app,
        "/api/translate/paragraph",
        serde_json::json!({ "doc_id": doc_id, "block_id": block_id }),
        &[("x-real-ip", "203.0.113.77")],
    )
    .await;
    assert_eq!(resp.status(), 403, "陌生 IP 无 token 访问他人文档 → 防盗链 403");
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(body["error"].as_str().unwrap().contains("无权访问"));

    // 无代理头（本地开发）→ 不受限（过门控即验证目的；LLM 指向不可达地址，结果码不限定）
    let resp = post_json(
        &app,
        "/api/translate/paragraph",
        serde_json::json!({ "doc_id": doc_id, "block_id": block_id }),
        &[],
    )
    .await;
    assert_ne!(resp.status(), 429, "本地直连不应被限流");
}

/// 构造 multipart 上传请求体（file 字段 + 可选 parser 字段）
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

#[tokio::test]
async fn multipart_upload_anon_quota_precheck() {
    // 公网语义：匿名（X-Real-IP）上传超限大文件 → 预检 429；小文件 → 200 计费
    use geminus_server::quota::QuotaStore;
    let quota = QuotaStore::open_in_memory(5).unwrap();
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3).with_quota(quota),
    );
    let boundary = "----geminus-test-boundary";

    // 100 页 large.pdf 匿名上传 → 429（预检发生在解析与 MinerU 调用之前）
    let body = multipart_body(
        boundary,
        "large.pdf",
        &std::fs::read(pdf_fixture("large.pdf")).unwrap(),
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .header("x-real-ip", "203.0.113.55")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 429, "匿名上传超限 PDF 应预检 429");
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(v["error"].as_str().unwrap().contains("请登录"));

    // 1 页 simple.pdf 匿名上传 → 200
    let body = multipart_body(
        boundary,
        "simple.pdf",
        &std::fs::read(pdf_fixture("simple.pdf")).unwrap(),
    );
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .header("x-real-ip", "203.0.113.55")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "1 页匿名上传应放行");

    // 同 IP 再次上传同一文件（同哈希）→ 幂等 200，不重复扣页
    let body = multipart_body(
        boundary,
        "simple.pdf",
        &std::fs::read(pdf_fixture("simple.pdf")).unwrap(),
    );
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .header("x-real-ip", "203.0.113.55")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "同内容重复上传不应叠加计费");
}

#[tokio::test]
async fn multipart_upload_parses_and_rejects_bad_ext() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3));

    // multipart 上传 simple.pdf → 200（builtin 同步 JSON）
    let boundary = "----geminus-test-boundary";
    let pdf_bytes = std::fs::read(pdf_fixture("simple.pdf")).unwrap();
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{b}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"simple.pdf\"\r\ncontent-type: application/pdf\r\n\r\n",
            b = boundary
        )
        .as_bytes(),
    );
    body.extend_from_slice(&pdf_bytes);
    body.extend_from_slice(format!("\r\n--{b}--\r\n", b = boundary).as_bytes());

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "multipart 上传应可解析");
    let doc: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(doc["blocks"].as_array().unwrap().len() >= 3);

    // 非 .docx/.pdf 扩展名 → 400
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{b}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"evil.exe\"\r\n\r\nMZ",
            b = boundary
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("\r\n--{b}--\r\n", b = boundary).as_bytes());
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    // 伪装扩展名（.pdf 实为文本）→ 400（magic bytes 校验）
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(
        format!(
            "--{b}\r\ncontent-disposition: form-data; name=\"file\"; filename=\"fake.pdf\"\r\n\r\nplain text not a pdf",
            b = boundary
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("\r\n--{b}--\r\n", b = boundary).as_bytes());
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn local_path_denied_when_disabled() {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let app = build_app(
        AppState::new(cache, ApiConfig::new("http://unused", "k", "m"), 3)
            .with_allow_local_path(false),
    );
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": "/etc/passwd.pdf" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "禁用本地路径后 JSON 分支应 403");
    let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert!(v["error"].as_str().unwrap().contains("上传"));
}

// ══════════════ v2.1：对齐审计端点（T1.6） ══════════════

#[tokio::test]
async fn debug_audit_clean_after_translate_and_404_cases() {
    let mock = spawn_mock_llm().await;
    let app = app_with_mock(&mock);

    // 1. parse → doc_id
    let (status, v) = parse_file(&app, &fixture_path().to_string_lossy()).await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let block_count = v["blocks"].as_array().unwrap().len();

    // 2. 翻译完成前审计 → 404（无译文）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/debug/audit?doc_id={doc_id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // 3. 翻译（消费完整 SSE 体，complete 后译文已入内存仓）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "source_lang": "en", "target_lang": "zh" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(body_string(resp).await.contains("event: complete"));

    // 4. 审计：干净（mock 全成功）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/debug/audit?doc_id={doc_id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let r: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(r["missing"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["extra"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["empty_translation"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["original_blocks"].as_u64().unwrap(), block_count as u64);
    assert_eq!(r["translated_blocks"].as_u64().unwrap(), block_count as u64);

    // 5. 缺 doc_id 参数 → 400；未知 doc_id → 404
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/debug/audit")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/debug/audit?doc_id=ghost")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ══════════════ v2.1：单段重译端点（T7.2） ══════════════

/// mock LLM：按 user 内容关键字返回不同文案（两次重译结果不同以验证绕过读缓存）
async fn spawn_mock_llm_variant() -> String {
    use std::collections::HashMap;
    // 按 user 内容计次：同一段第 N 次请求返回第 N 版译文（验证绕过读缓存）
    let seen: Arc<std::sync::Mutex<HashMap<String, usize>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let seen2 = seen.clone();
    let router = Router::new().route(
        "/chat/completions",
        route_post(move |body: axum::Json<serde_json::Value>| {
            let seen = seen2.clone();
            async move {
                let user = body.0["messages"]
                    .as_array()
                    .and_then(|ms| ms.last().cloned())
                    .and_then(|m| m["content"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let n = {
                    let mut m = seen.lock().unwrap();
                    let e = m.entry(user.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                // rich.docx 首个标题为 "Chapter One: Introduction"
                let content = if user.contains("Chapter One") {
                    if n == 1 { "第一版标题译文".to_string() } else { format!("第{n}次重译标题") }
                } else {
                    format!("【译】{user}")
                };
                axum::Json(serde_json::json!({
                    "choices": [{"message": {"content": content}, "finish_reason": "stop"}]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    format!("http://{addr}")
}

#[tokio::test]
async fn translate_paragraph_preserves_inline_math_via_slots() {
    // v2.3 B4b：单段重译必须走 math_slot（extract→verify→强制回填）——
    // 公式不经过模型。mock 对令牌化请求一律吞掉 {{Mn}}（持续损坏）：
    // 未接防线时 $...$ 原文直发被模型改写；接防线后公式强制回填保全。
    let router = Router::new().route(
        "/chat/completions",
        route_post(|body: axum::Json<serde_json::Value>| async move {
            let user = body.0["messages"]
                .as_array()
                .and_then(|ms| ms.last().cloned())
                .and_then(|m| m["content"].as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            // 模拟模型破坏：令牌化请求吞 {{Mn}}；原始 $...$ 直发时改写公式
            let content = if user.contains("{{M") {
                format!("【坏译】{}", user.replace("{{M0}}", "").replace("{{M1}}", ""))
            } else {
                format!(
                    "【坏译】{}",
                    user.replace("$J(\\phi)$", "J(φ)").replace("$\\eta = 0.1$", "η")
                )
            };
            axum::Json(serde_json::json!({
                "choices": [{"message": {"content": content}, "finish_reason": "stop"}]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    let mock = format!("http://{addr}");

    let app = app_with_mock(&mock);
    let math_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/math_para.docx");
    let (status, v) = parse_file(&app, &math_fixture.to_string_lossy()).await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let math_block = v["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| {
            b["content"]
                .as_array()
                .map(|cs| {
                    cs.iter().any(|c| c["text"].as_str().is_some_and(|t| t.contains("$J(")))
                })
                .unwrap_or(false)
        })
        .map(|b| b["id"].as_str().unwrap().to_string())
        .expect("应存在含 $J(\\phi)$ 的段落");

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate/paragraph")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "doc_id": doc_id,
                        "block_id": math_block,
                        "source_lang": "en",
                        "target_lang": "zh"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let pair: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(pair["fallback"], false);
    let text = pair["translated"]["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("{{M"), "令牌必须回填: {text}");
    assert!(text.contains("$J(\\phi)$"), "公式源码逐位还原: {text}");
    assert!(text.contains("$\\eta = 0.1$"), "第二公式同样保全: {text}");
}

#[tokio::test]
async fn translate_paragraph_retranslates_and_overrides_cache() {
    let mock = spawn_mock_llm_variant().await;
    let app = app_with_mock(&mock);

    // 1. parse + 全文翻译（标题进缓存）
    let (status, v) = parse_file(&app, &fixture_path().to_string_lossy()).await;
    assert_eq!(status, 200);
    let doc_id = v["id"].as_str().unwrap().to_string();
    let first_heading = v["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| {
            b["style"]["heading_level"].as_u64().is_some()
                || b["content"][0]["text"].as_str().is_some_and(|t| t.to_lowercase().contains("geminus"))
        })
        .map(|b| b["id"].as_str().unwrap().to_string())
        .expect("应找到标题块");

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "source_lang": "en", "target_lang": "zh" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(body_string(resp).await.contains("event: complete"));

    // 2. 单段重译 → 200，返回新译文（绕过读缓存）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate/paragraph")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "block_id": first_heading, "source_lang": "en", "target_lang": "zh" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let pair: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(pair["block_id"], first_heading);
    assert_eq!(pair["fallback"], false);
    let t = pair["translated"]["content"][0]["text"].as_str().unwrap();
    assert!(t.contains("重译"), "应为重译新文案: {t}");

    // 3. 二次重译 → 文案再变（读缓存被绕过、覆盖写生效）
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate/paragraph")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "block_id": first_heading }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let pair2: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let t2 = pair2["translated"]["content"][0]["text"].as_str().unwrap();
    assert_ne!(t, t2, "两次重译应产出不同文案");

    // 4. 错误路径：未知块 404；占位块 400
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/translate/paragraph")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({ "doc_id": doc_id, "block_id": "b_ghost" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let placeholder_id = v["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] != "paragraph" && b["type"] != "empty")
        .map(|b| b["id"].as_str().unwrap().to_string());
    if let Some(pid) = placeholder_id {
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/translate/paragraph")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({ "doc_id": doc_id, "block_id": pid }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "占位块不可重译");
    }
}
