//! PDF 集成测试（v1_plan T7.3/T13.1）：
//! 1. expected.json 交叉验证——pdf-rs 提取结果与 PyMuPDF 独立提取一致；
//! 2. PDF → mock LLM 翻译 → SQLite 缓存 → 二次全缓存命中 的完整管线。

use std::sync::Arc;

use axum::routing::post;
use axum::Router;
use geminus_core::cache::TranslationCache;
use geminus_core::model::{Block, InlineContent, ProgressStatus};
use geminus_core::translate::{translate_document, LlmClient};
use geminus_core::{parse_pdf, ApiConfig};
use serde::Deserialize;
use tokio::sync::mpsc;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/pdf")
}

#[derive(Deserialize)]
struct FixtureExpectation {
    pages: usize,
    images: usize,
    must_contain: Vec<String>,
}

/// 所有夹具与 PyMuPDF 验证脚本产出的 expected.json 一致（两独立实现互证）
#[test]
fn fixtures_match_pymupdf_expectations() {
    let expected: std::collections::HashMap<String, FixtureExpectation> =
        serde_json::from_str(&std::fs::read_to_string(fixtures_dir().join("expected.json")).unwrap())
            .expect("expected.json 应为合法 JSON（先运行 scripts/gen_pdf_fixtures.py + verify_fixtures.py）");

    assert!(!expected.is_empty());
    for (name, exp) in &expected {
        let path = fixtures_dir().join(name);
        assert!(path.exists(), "缺少夹具 {name}");
        let doc = parse_pdf(&path).unwrap_or_else(|e| panic!("{name} 解析失败: {e}"));

        // 图片数：资源池中的 image 资源数
        let images = doc
            .resources
            .values()
            .filter(|r| r.kind == geminus_core::model::ResourceKind::Image)
            .count();
        assert_eq!(images, exp.images, "{name}: 图片数");

        // 页数 → 以 large.pdf 的 LARGEPAGE 标记与块规模近似校验；
        // must_contain 是硬断言（PDF 文本流完整性）
        let joined: String = doc
            .blocks
            .iter()
            .filter_map(|b| b.paragraph_content())
            .flat_map(|c| c.iter())
            .filter_map(|c| match c {
                InlineContent::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for kw in &exp.must_contain {
            assert!(
                joined.contains(kw.as_str()),
                "{name}: 缺少关键词 {kw:?}\n前 200 字: {}",
                &joined[..joined.len().min(200)]
            );
        }

        // 页数：extractor 层再验一次（解析器可见页数）
        let ex = geminus_core::pdf::extract(&path).unwrap();
        assert_eq!(ex.page_count, exp.pages, "{name}: 页数");
    }
}

/// 回声 mock LLM（与 e2e_mock 相同语义，保留 {{Pn}} 令牌）
async fn spawn_mock_llm() -> String {
    let router = Router::new().route(
        "/chat/completions",
        post(|body: axum::Json<serde_json::Value>| async move {
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

/// T13.1：PDF 解析 → 翻译 → 缓存写入 → 二次翻译全缓存命中
#[tokio::test]
async fn pdf_translate_pipeline_with_cache() {
    let base = spawn_mock_llm().await;
    let doc = parse_pdf(&fixtures_dir().join("two_column.pdf")).unwrap();
    let n_para = doc.blocks.iter().filter(|b| !b.is_placeholder()).count();
    assert!(n_para >= 10, "two_column.pdf 段落数: {n_para}");

    let cfg = ApiConfig::new(base, "sk-test", "mock");
    let client = Arc::new(LlmClient::new(cfg));
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());

    // 第一次：全部 done（reader 并发排空，防 bounded channel 背压死锁）
    let (tx, mut rx) = mpsc::channel::<geminus_core::model::TranslateProgress>(64);
    let reader = tokio::spawn(async move {
        let mut statuses = Vec::new();
        while let Some(p) = rx.recv().await {
            statuses.push(p.status);
        }
        statuses
    });
    let r1 = translate_document(&doc, client.clone(), cache.clone(), tx, 3, "en", "zh").await;
    let statuses = reader.await.unwrap();
    assert_eq!(r1.pairs.len(), doc.blocks.len(), "译文块与原文块一一对应");
    assert_eq!(statuses.iter().filter(|s| **s == ProgressStatus::Done).count(), n_para);
    assert!(!statuses.contains(&ProgressStatus::Error));

    // 段落译文非空且占位块保持 null
    for pair in &r1.pairs {
        let orig = &pair.original;
        if orig.is_placeholder() {
            assert!(pair.translated.is_none(), "占位块 {} 不翻译", pair.block_id);
        } else {
            let t = pair.translated.as_ref().unwrap();
            let text: String = t
                .paragraph_content()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    InlineContent::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert!(!text.trim().is_empty(), "块 {} 译文为空", pair.block_id);
        }
    }

    // 第二次：全部 cached，0 次 LLM 调用
    let (tx2, mut rx2) = mpsc::channel::<geminus_core::model::TranslateProgress>(64);
    let reader2 = tokio::spawn(async move {
        let mut cached = 0;
        while let Some(p) = rx2.recv().await {
            if p.status == ProgressStatus::Cached {
                cached += 1;
            }
        }
        cached
    });
    let r2 = translate_document(&doc, client, cache, tx2, 3, "en", "zh").await;
    let cached = reader2.await.unwrap();
    assert_eq!(cached, n_para, "二次翻译应全缓存命中");
    assert_eq!(r2.pairs.len(), r1.pairs.len());
}

/// 真实论文 b2（13 页 TeX 双栏）：管线不崩溃 + 块对齐（质量抽查在单测层）
#[tokio::test]
async fn real_paper_pipeline_smoke() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/b2_Prototypical_Networks.pdf");
    if !path.exists() {
        return;
    }
    let base = spawn_mock_llm().await;
    let doc = parse_pdf(&path).unwrap();
    assert!(doc.blocks.len() > 400, "块数: {}", doc.blocks.len());

    let cfg = ApiConfig::new(base, "sk-test", "mock");
    let client = Arc::new(LlmClient::new(cfg));
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (tx, mut rx) = mpsc::channel::<geminus_core::model::TranslateProgress>(256);
    let reader = tokio::spawn(async move {
        let mut errors = 0;
        while let Some(p) = rx.recv().await {
            if p.status == ProgressStatus::Error {
                errors += 1;
            }
        }
        errors
    });
    let result = translate_document(&doc, client, cache, tx, 3, "en", "zh").await;
    let errors = reader.await.unwrap();
    assert_eq!(errors, 0, "真实论文翻译不应有失败块");
    assert_eq!(result.pairs.len(), doc.blocks.len());
    // 图片占位块翻译侧为 null 且资源池含图
    let img_blocks = doc.blocks.iter().filter(|b| matches!(b, Block::Image { .. })).count();
    assert!(img_blocks >= 1, "b2 应含图片块");
    for pair in &result.pairs {
        if matches!(pair.original, Block::Image { .. }) {
            assert!(pair.translated.is_none());
        }
    }
}
