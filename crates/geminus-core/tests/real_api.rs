//! 真实 API 集成测试（任务 T12）：deepseek-v4-flash 实际调用。
//! 默认 #[ignore]，显式运行：
//!   cargo test -p geminus-core --test real_api -- --ignored --nocapture

use std::path::Path;
use std::sync::Arc;

use geminus_core::cache::TranslationCache;
use geminus_core::model::{Block, ProgressStatus};
use geminus_core::translate::{translate_document, LlmClient};
use geminus_core::{parse_docx, ApiConfig};
use tokio::sync::mpsc;

fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/simple.docx")
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

#[tokio::test]
#[ignore = "需真实 API key（项目根 .env.local）与网络"]
async fn translates_document_with_real_deepseek() {
    // 加载项目根 .env.local（测试 cwd 在 crates/geminus-core）
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.local");
    let _ = dotenvy::from_filename(&env_path);
    let cfg = ApiConfig::from_env();
    assert!(!cfg.api_key.is_empty(), "DEEPSEEK_API_KEY 未配置（.env.local）");
    assert_eq!(cfg.model, "deepseek-v4-flash");

    let doc = parse_docx(&fixture_path()).expect("simple.docx 应可解析");
    let paragraph_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph { .. }))
        .count();
    assert!(paragraph_count >= 5, "夹具段落数异常：{paragraph_count}");

    let client = Arc::new(LlmClient::new(cfg));
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());

    let (tx, mut rx) = mpsc::channel(256);
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events2 = events.clone();
    let reader = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            events2.lock().unwrap().push(ev);
        }
    });

    let result = translate_document(&doc, client, cache.clone(), tx, 2, "en", "zh").await;
    reader.await.unwrap();
    let events = events.lock().unwrap().clone();

    // 对齐
    assert_eq!(result.pairs.len(), doc.blocks.len());

    // 进度事件：Done+Cached+Error 覆盖全部可翻译段
    let settled = events
        .iter()
        .filter(|e| matches!(e.status, ProgressStatus::Done | ProgressStatus::Cached | ProgressStatus::Error))
        .count();
    assert_eq!(settled, paragraph_count);

    // 至少两个正文段落译文含中文
    let cjk_translated = result
        .pairs
        .iter()
        .filter(|p| {
            p.translated
                .as_ref()
                .and_then(|b| b.paragraph_content().map(|c| {
                    c.iter()
                        .filter_map(|i| match i {
                            geminus_core::model::InlineContent::Text { text, .. } => {
                                Some(text.as_str())
                            }
                            _ => None,
                        })
                        .collect::<String>()
                }))
                .map(|s| contains_cjk(&s))
                .unwrap_or(false)
        })
        .count();
    assert!(
        cjk_translated >= 2,
        "至少 2 段译文应含中文，实际 {cjk_translated}/{paragraph_count}"
    );

    // 缓存应写入与成功翻译数一致的条目
    assert!(
        cache.len() >= cjk_translated && cache.len() <= paragraph_count,
        "缓存条目 {} 应在 [{cjk_translated}, {paragraph_count}] 内",
        cache.len()
    );

    // 占位符/空段不翻译
    for pair in &result.pairs {
        if !matches!(pair.original, Block::Paragraph { .. }) {
            assert!(pair.translated.is_none());
        }
    }

    eprintln!("✅ 真实 API：{}/{} 段含中文译文，缓存 {} 条", cjk_translated, paragraph_count, cache.len());
}

/// v1_plan T13.4：真实 API 翻译 simple.pdf（PDF 管线实证）
#[tokio::test]
#[ignore = "需真实 API key（项目根 .env.local）与网络"]
async fn translates_pdf_with_real_deepseek() {
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.local");
    let _ = dotenvy::from_filename(&env_path);
    let cfg = ApiConfig::from_env();
    assert!(!cfg.api_key.is_empty(), "DEEPSEEK_API_KEY 未配置（.env.local）");

    let pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/pdf/simple.pdf");
    let doc = geminus_core::parse_pdf(&pdf).expect("simple.pdf 应可解析");
    let paragraph_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph { .. }))
        .count();
    assert_eq!(paragraph_count, 5, "simple.pdf 应为 5 段");

    let client = Arc::new(LlmClient::new(cfg));
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());

    let (tx, mut rx) = mpsc::channel(256);
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events2 = events.clone();
    let reader = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            events2.lock().unwrap().push(ev);
        }
    });
    let result = translate_document(&doc, client, cache, tx, 2, "en", "zh").await;
    reader.await.unwrap();
    let events = events.lock().unwrap().clone();

    assert_eq!(result.pairs.len(), doc.blocks.len(), "PDF 块与译文对齐");
    let settled = events
        .iter()
        .filter(|e| matches!(e.status, ProgressStatus::Done | ProgressStatus::Cached | ProgressStatus::Error))
        .count();
    assert_eq!(settled, paragraph_count);

    let cjk_translated = result
        .pairs
        .iter()
        .filter(|p| {
            p.translated
                .as_ref()
                .and_then(|b| b.paragraph_content().map(|c| {
                    c.iter()
                        .filter_map(|i| match i {
                            geminus_core::model::InlineContent::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<String>()
                }))
                .map(|s| contains_cjk(&s))
                .unwrap_or(false)
        })
        .count();
    assert!(cjk_translated >= 4, "至少 4/5 段含中文译文，实际 {cjk_translated}");
    eprintln!("✅ 真实 API PDF：{cjk_translated}/{paragraph_count} 段含中文译文");
}
