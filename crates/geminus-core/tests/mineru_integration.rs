//! MinerU 集成测试（v2 T11）：
//! 1. 真实 MinerU 夹具目录 → adapter → merge → 与翻译管线兼容（mock LLM）
//! 2. 真实 API 端到端（#[ignore]，需要 MINERU_API_KEY + 网络）

use std::sync::Arc;

use axum::routing::post;
use axum::Router;
use geminus_core::cache::TranslationCache;
use geminus_core::mineru::{adapt_dir, merge_documents};
use geminus_core::translate::{translate_document, LlmClient};
use geminus_core::ApiConfig;
use tokio::sync::mpsc;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/mineru")
        .join(name)
}

/// 回声 mock LLM（对合并请求返回规范编号）
async fn spawn_numbered_mock_llm() -> String {
    let router = Router::new().route(
        "/chat/completions",
        post(|body: axum::Json<serde_json::Value>| async move {
            let user = body.0["messages"]
                .as_array()
                .and_then(|ms| ms.last().cloned())
                .and_then(|m| m["content"].as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            let content = if user.contains("保持段落编号") {
                let mut out = String::from("译文如下：\n");
                for line in user.lines() {
                    if let Some(rest) = line.strip_prefix('[') {
                        if let Some((num, text)) = rest.split_once("] ") {
                            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                                out.push_str(&format!("[{num}] 【合并】{text}\n"));
                            }
                        }
                    }
                }
                out
            } else {
                format!("【译】{user}")
            };
            axum::Json(serde_json::json!({
                "choices": [{"message": {"content": content}, "finish_reason": "stop"}],
                "model": "mock"
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
    format!("http://{addr}")
}

#[test]
fn merged_mineru_fixtures_flow_through_translate_pipeline() {
    // 两个真实 MinerU 输出目录 → IR → 合并 → mock 翻译（运行时内联 block_on）
    let simple = adapt_dir(&fixture_dir("simple"), 0, "simple").unwrap();
    let headings = adapt_dir(&fixture_dir("headings"), 1, "headings").unwrap();
    let total_blocks = simple.blocks.len() + headings.blocks.len() - 1; // 边界可能合并一段
    let merged = merge_documents(
        vec![
            ("c0".to_string(), simple, 0usize),
            ("c1".to_string(), headings, 1usize),
        ],
        "merged",
    );
    assert!(merged.blocks.len() >= total_blocks - 1);
    assert!(merged.pdf_meta.is_some());
    assert!(!merged.render_meta.is_empty());
    // 块 id 全局连续唯一
    let ids: Vec<&str> = merged.blocks.iter().map(|b| b.id()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "块 id 全局唯一");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let base = spawn_numbered_mock_llm().await;
        let client = Arc::new(LlmClient::new(ApiConfig::new(base, "sk-test", "mock")));
        let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
        let (tx, mut rx) = mpsc::channel::<geminus_core::model::TranslateProgress>(1024);
        let reader = tokio::spawn(async move {
            let mut errors = 0usize;
            let mut done = 0usize;
            while let Some(ev) = rx.recv().await {
                match ev.status {
                    geminus_core::model::ProgressStatus::Error => errors += 1,
                    geminus_core::model::ProgressStatus::Done => done += 1,
                    _ => {}
                }
            }
            (done, errors)
        });
        let result = translate_document(&merged, client, cache, tx, 3, "en", "zh").await;
        let (done, errors) = reader.await.unwrap();
        assert_eq!(errors, 0, "mock LLM 下不应有失败段");
        assert_eq!(done, result.pairs.iter().filter(|p| p.translated.is_some()).count());
        assert_eq!(result.pairs.len(), merged.blocks.len(), "译文与原文块对齐");
    });
}

#[test]
fn builtin_pdf_regression_unaffected() {
    // v1 内置解析回归： MinerU 代码路径不影响 builtin
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/pdf/two_column.pdf");
    let doc = geminus_core::parse_pdf(&path).unwrap();
    assert!(doc.blocks.len() > 10);
    assert!(doc.render_meta.is_empty(), "builtin 路径不带 render_meta");
    assert!(doc.pdf_meta.is_none());
}

/// 真实 MinerU API 端到端：simple.pdf → 拆分(单片) → 上传 → 轮询 → 下载 → 适配 → 合并。
/// 运行：cargo test -p geminus-core --test mineru_integration -- --ignored --nocapture
#[tokio::test]
#[ignore = "需要 MINERU_API_KEY 与外网"]
async fn real_mineru_parse_simple_pdf() {
    let Some(client) = geminus_core::mineru::MineruClient::from_env() else {
        panic!("MINERU_API_KEY 未配置");
    };
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/pdf/simple.pdf");
    let work = tempfile::tempdir().unwrap();
    let stages = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let s2 = stages.clone();
    let doc = geminus_core::mineru::parse_pdf_mineru(
        &file,
        &client,
        &geminus_core::mineru::MineruConfig::default(),
        &geminus_core::mineru::SplitConfig::default(),
        work.path(),
        move |stage| {
            let payload = serde_json::to_value(stage).unwrap_or_default();
            s2.lock().unwrap().push(payload["stage"].as_str().unwrap_or("?").to_string());
        },
    )
    .await
    .expect("真实 MinerU 解析应成功");
    assert_eq!(doc.pdf_meta.as_ref().unwrap().page_count, 1);
    assert!(!doc.blocks.is_empty());
    assert!(!doc.render_meta.is_empty());
    let full_text: String = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            geminus_core::model::Block::Paragraph { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        geminus_core::model::InlineContent::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert!(full_text.contains("Machine learning"), "VLM 应识别正文：{full_text:.200}");
    println!("[real] 块数 {} / 阶段 {:?}", doc.blocks.len(), stages.lock().unwrap());
}
