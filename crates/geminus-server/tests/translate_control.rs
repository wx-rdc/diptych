//! v2.4 T5 任务控制流集成测试（HTTP 级）：
//! - 匿名熔断：配额内逐批实扣，用尽 → limit_reached 事件（已译部分保留）
//! - 登录取消：POST /api/translate/:doc_id/cancel → cancelled 事件 + registry 清空
//! - SSE 断开自动止损：客户端 drop → 任务取消（registry 清空）
//! - 断点续传：取消后重发 → 已译段落缓存命中秒回（LLM 调用数下降）+ complete
//! - cancel 端点防盗链：跨 IP 无 token → 403

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::routing::post as route_post;
use axum::Router;
use futures::StreamExt;
use geminus_core::cache::TranslationCache;
use geminus_core::ApiConfig;
use geminus_server::quota::QuotaStore;
use geminus_server::{build_app, AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn pdf_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/pdf").join(name)
}

type App = Router;

fn make_app(llm_base: &str, quota: QuotaStore) -> App {
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    build_app(
        AppState::new(cache, ApiConfig::new(llm_base, "k", "m"), 3).with_quota(quota),
    )
}

/// mock LLM：可注入的调用计数器（断点续传断言「缓存命中不再调 LLM」用）
async fn spawn_counting_mock_llm() -> (String, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let c2 = count.clone();
    let router = Router::new().route(
        "/chat/completions",
        route_post(move |body: axum::Json<serde_json::Value>| {
            let c = c2.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                let user = body.0["messages"]
                    .as_array()
                    .and_then(|ms| ms.last().cloned())
                    .and_then(|m| m["content"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                // 带 sleep 制造窗口（cancel 测试需要翻译进行中的时刻）
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                axum::Json(serde_json::json!({
                    "choices": [{"message": {"content": format!("【译】{user}")}, "finish_reason": "stop"}]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    (format!("http://{addr}"), count)
}

#[derive(Debug, Clone)]
struct SseEv {
    event: String,
    data: String,
}

/// 逐帧读 SSE（HTTP body → 事件流）。
/// `pred` = **继续读取条件**（返回 false 的那一帧是最后一帧；流被 drop = 客户端断开）。
async fn read_sse_while(
    resp: axum::response::Response,
    mut pred: impl FnMut(&SseEv) -> bool,
) -> Vec<SseEv> {
    let mut stream = resp.into_body().into_data_stream();
    let mut buf = String::new();
    let mut events = Vec::new();
    'outer: while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find("\n\n") {
            let frame: String = buf.drain(..pos + 2).collect();
            let mut ev = SseEv { event: "message".into(), data: String::new() };
            for line in frame.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    ev.event = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    ev.data.push_str(v.trim_start());
                }
            }
            let keep = pred(&ev);
            events.push(ev);
            if !keep {
                break 'outer;
            }
        }
    }
    events
}

async fn post_translate(app: &App, body: serde_json::Value, headers: &[(&str, &str)]) -> axum::response::Response {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri("/api/translate")
        .header("content-type", "application/json");
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    app.clone()
        .oneshot(builder.body(axum::body::Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

async fn parse_pdf(app: &App, name: &str, headers: &[(&str, &str)]) -> String {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri("/api/parse")
        .header("content-type", "application/json");
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let resp = app
        .clone()
        .oneshot(
            builder
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": pdf_fixture(name).to_string_lossy(), "parser": "builtin" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["id"].as_str().unwrap().to_string()
}

/// T5 熔断：解析免费、翻译扣费 → 常规漏斗下第二篇文档中途断粮。
/// 限额 2：解析 A(1 页) + B(2 页) 均通过预检（解析不扣）；译完 A 剩 1 → 译 B
/// 批 1 扣满 → 批 2 熔断（limit_reached，B 的第 2 页未译）。
#[tokio::test]
async fn anon_translate_limit_reached_after_quota() {
    let (llm, _count) = spawn_counting_mock_llm().await;
    let app = make_app(&llm, QuotaStore::open_in_memory(2).unwrap());
    let ip: &[(&str, &str)] = &[("x-real-ip", "203.0.113.50")];
    let doc_a = parse_pdf(&app, "simple.pdf", ip).await; // 真实 1 页
    let doc_b = parse_pdf(&app, "two_column.pdf", ip).await; // 真实 2 页（预检过）；估算 2 批

    // 译 A：1 页全扣 → 剩 1
    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_a }), ip).await;
    assert_eq!(resp.status(), 200);
    let events = read_sse_while(resp, |ev| ev.event != "complete").await;
    assert!(events.iter().any(|e| e.event == "complete"), "A 配额内完整译完");

    // 译 B：批 1 扣 1 → 剩 0 → 批 2 熔断
    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_b }), ip).await;
    assert_eq!(resp.status(), 200);
    let events = read_sse_while(resp, |ev| ev.event != "limit_reached").await;
    let limit_ev = events.iter().find(|e| e.event == "limit_reached").expect("应收到 limit_reached");
    let v: serde_json::Value = serde_json::from_str(&limit_ev.data).unwrap();
    assert_eq!(v["used"].as_i64(), Some(2), "恰好译满 2 页后熔断");
    assert_eq!(v["limit"].as_i64(), Some(2));
    assert_eq!(v["required"].as_i64(), Some(1));
    assert!(events.iter().all(|e| e.event != "complete"), "熔断后无 complete");
    let progress_n = events.iter().filter(|e| e.event == "progress").count();
    assert!(progress_n >= 1, "已译部分 progress 保留（实际 {progress_n}）");
}

/// T5 取消：翻译进行中 POST cancel → cancelled 事件（translated_pages ≥ 0）+ registry 清空
#[tokio::test]
async fn cancel_midway_emits_cancelled_and_clears_registry() {
    let (llm, _count) = spawn_counting_mock_llm().await;
    let app = make_app(&llm, QuotaStore::open_in_memory(5).unwrap());
    // 本地直连（无代理头）：无配额干扰，纯控制流
    let doc_id = parse_pdf(&app, "large.pdf", &[]).await;

    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_id }), &[]).await;
    assert_eq!(resp.status(), 200);

    // 收到首个 progress 后发起 cancel
    let app2 = app.clone();
    let doc2 = doc_id.clone();
    let reader = tokio::spawn(async move {
        read_sse_while(resp, |ev| ev.event == "progress").await
    });
    // 等 reader 抓到首个 progress（确保任务已注册并运行）
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let cancel_resp = app2
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/translate/{doc2}/cancel"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancel_resp.status(), 200);
    let v: serde_json::Value =
        serde_json::from_slice(&cancel_resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["cancelled"].as_bool(), Some(true));

    let events = reader.await.unwrap();
    let cancelled = events.iter().find(|e| e.event == "cancelled").expect("应收到 cancelled");
    let v: serde_json::Value = serde_json::from_str(&cancelled.data).unwrap();
    assert!(v["translated_pages"].as_i64().is_some(), "cancelled 携带 translated_pages");
    assert!(events.iter().all(|e| e.event != "complete"), "取消后无 complete");

    // registry 已清空：再次 cancel → 404
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let resp = app2
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/translate/{doc_id}/cancel"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "任务结束后 registry 清空（无泄漏）");
}

/// T5 断开止损：客户端读几帧后 drop → 服务端自动取消，registry 清空
#[tokio::test]
async fn sse_disconnect_auto_cancels() {
    let (llm, _count) = spawn_counting_mock_llm().await;
    let app = make_app(&llm, QuotaStore::open_in_memory(5).unwrap());
    let doc_id = parse_pdf(&app, "large.pdf", &[]).await;

    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_id }), &[]).await;
    // 第一次：读到首个 progress 即断流（模拟弱网闪断/用户关页）
    let events = read_sse_while(resp, |ev| ev.event != "progress").await;
    assert!(!events.is_empty());

    // 服务端检测到断开 → cancel + registry 清理（给控制点轮询留时间）
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/translate/{doc_id}/cancel"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "断开后任务应已自动取消并移除");
}

/// T5 断点续传：中途 cancel → 重发 translate → 已译段缓存命中（LLM 调用数显著下降）+ complete
#[tokio::test]
async fn resume_after_cancel_reuses_cache() {
    let (llm, count) = spawn_counting_mock_llm().await;
    let app = make_app(&llm, QuotaStore::open_in_memory(5).unwrap());
    let doc_id = parse_pdf(&app, "large.pdf", &[]).await;

    // 第一次：读到首个真实翻译完成（done）的块后断流（模拟用户中途停止）
    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_id }), &[]).await;
    let events = read_sse_while(resp, |ev| {
        !(ev.event == "progress" && ev.data.contains("\"status\":\"done\""))
    })
    .await;
    let done_first = events
        .iter()
        .filter(|e| e.event == "progress")
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
        .filter(|p| p["status"] == "done")
        .count();
    assert!(done_first >= 1, "取消前已有真实翻译块");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await; // 等断开止损落地
    let llm_first = count.load(Ordering::SeqCst);

    // 第二次：完整翻译（读到 complete 为止；同 app → 同缓存）
    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_id }), &[]).await;
    let events = read_sse_while(resp, |ev| ev.event != "complete").await;
    let mut summary = std::collections::BTreeMap::new();
    for e in &events {
        *summary.entry(e.event.clone()).or_insert(0) += 1;
    }
    eprintln!("[debug] t2 events: {summary:?}, llm_first={llm_first}, now={}", count.load(Ordering::SeqCst));
    assert!(events.iter().any(|e| e.event == "complete"), "续译完成");
    let cached_n = events
        .iter()
        .filter(|e| e.event == "progress")
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
        .filter(|p| p["status"] == "cached")
        .count();
    let t2_done_n = events
        .iter()
        .filter(|e| e.event == "progress")
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
        .filter(|p| p["status"] == "done")
        .count();
    let llm_second_delta = count.load(Ordering::SeqCst) - llm_first;
    assert!(
        cached_n >= done_first,
        "已译段落命中缓存（cached {cached_n} ≥ 首轮已译 {done_first}）"
    );
    // 每个真实翻译块恰好一次 LLM 调用；缓存命中块零调用 → 增量调用 == t2 真实翻译块数
    assert_eq!(
        llm_second_delta, t2_done_n,
        "LLM 增量调用与 t2 真实翻译块一一对应（缓存块零调用）"
    );
    let _ = summary;
}

/// T5 cancel 端点防盗链：跨 IP 无 token → 403（有任务与否不影响拦截顺序）
#[tokio::test]
async fn cancel_endpoint_gated_by_access() {
    let (llm, _count) = spawn_counting_mock_llm().await;
    // 大限额：本测试只验证 cancel 端点权限，不做配额路径
    let app = make_app(&llm, QuotaStore::open_in_memory(1000).unwrap());
    let ip: &[(&str, &str)] = &[("x-real-ip", "203.0.113.60")];
    let doc_id = parse_pdf(&app, "large.pdf", ip).await;

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/translate/{doc_id}/cancel"))
                .header("x-real-ip", "203.0.113.99")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "陌生 IP 不能取消他人任务");
}

/// v2.4.1 修复回归：额度耗尽后回访已译文档 → 已付费批豁免熔断 → 缓存秒回（零新增 LLM 调用）。
/// 限额 2 = two_column.pdf 全文；译完额度满 → 回访时批 1（已付）缓存重放、批 2（已付）同样豁免
/// → 全文秒回 complete；随后验证新文档（未付费）仍正常熔断。
#[tokio::test]
async fn revisit_after_quota_exhausted_replays_from_cache() {
    let (llm, count) = spawn_counting_mock_llm().await;
    let app = make_app(&llm, QuotaStore::open_in_memory(2).unwrap());
    let ip: &[(&str, &str)] = &[("x-real-ip", "203.0.113.70")];
    let doc_id = parse_pdf(&app, "two_column.pdf", ip).await; // 2 页（2 批）

    // 第一轮：完整译完（扣满 2/2）
    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_id }), ip).await;
    let events = read_sse_while(resp, |ev| ev.event != "complete").await;
    assert!(events.iter().any(|e| e.event == "complete"), "第一轮完整译完");
    let llm_first = count.load(Ordering::SeqCst);
    assert!(llm_first > 0);

    // 第二轮回访（额度 0/2 剩 0）：两批均已付费 → 全部豁免熔断 → 缓存秒回 complete
    let resp = post_translate(&app, serde_json::json!({ "doc_id": doc_id }), ip).await;
    let events = read_sse_while(resp, |ev| ev.event != "complete").await;
    assert!(
        events.iter().any(|e| e.event == "complete"),
        "已付费批豁免熔断：额度耗尽也能全文缓存秒回"
    );
    let cached_n = events
        .iter()
        .filter(|e| e.event == "progress")
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
        .filter(|p| p["status"] == "cached")
        .count();
    assert!(cached_n > 0, "回访以缓存事件秒回（可读）");
    assert_eq!(
        count.load(Ordering::SeqCst),
        llm_first,
        "回访零新增 LLM 调用（已付费批缓存重放）"
    );

    // 对照：同一 IP 再译新文档（未付费批）→ 预检 402（2 页 > 剩 0），熔断防线仍在
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/parse")
                .header("content-type", "application/json")
                .header("x-real-ip", "203.0.113.70")
                .body(axum::body::Body::from(
                    serde_json::json!({ "file_path": pdf_fixture("mixed_layout.pdf").to_string_lossy(), "parser": "builtin" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 402, "未付费新文档仍被预检拦截");
}
