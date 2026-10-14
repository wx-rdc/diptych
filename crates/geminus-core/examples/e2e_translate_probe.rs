//! 实验：目标段落走完整 translate_document 路径（分块合并→LLM→[N]拆分→回填→缓存），
//! 检查产物中是否出现 U+FFFD。只取目标句所在块窗口，控制 API 消耗。
//!
//! cargo run -p geminus-core --example e2e_translate_probe -- test/_local/14_EffTrans_Mamba.pdf
#![allow(missing_docs)]

use geminus_core::model::{Block, InlineContent};
use geminus_core::translate::{translate_document, LlmClient};
use geminus_core::{parse_pdf, ApiConfig};
use std::sync::Arc;
use tokio::sync::mpsc;

fn stats(tag: &str, s: &str) {
    let fffd = s.chars().filter(|&c| c == '\u{FFFD}').count();
    let astral = s.chars().filter(|c| (*c as u32) > 0xFFFF).count();
    println!("[{tag}] U+FFFD={fffd} 星面字符={astral}: {s}");
}

#[tokio::main]
async fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test/_local/14_EffTrans_Mamba.pdf".to_string());
    let mut doc = parse_pdf(std::path::Path::new(&path)).expect("解析失败");

    // 截取目标句所在块窗口（b_0064..=b_0068 一带），其余丢弃
    let anchor = doc
        .blocks
        .iter()
        .position(|b| {
            let Block::Paragraph { content, .. } = b else { return false };
            let t: String = content
                .iter()
                .filter_map(|c| match c {
                    InlineContent::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            t.contains("represented by")
        })
        .expect("未找到目标块");
    let lo = anchor.saturating_sub(4);
    let hi = (anchor + 3).min(doc.blocks.len());
    doc.blocks = doc.blocks[lo..hi].to_vec();
    println!("窗口块数: {} (锚点 {})", doc.blocks.len(), doc.blocks[anchor - lo].id());

    let client = Arc::new(LlmClient::new(ApiConfig::from_env()));
    let cache = Arc::new(geminus_core::cache::TranslationCache::open_in_memory().unwrap());
    let (tx, mut rx) = mpsc::channel(256);
    let reader = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let result = translate_document(&doc, client, cache, tx, 2, "en", "zh").await;
    reader.await.unwrap();

    println!("\n=== 全链路译文对 ===");
    let mut total_fffd = 0usize;
    for pair in &result.pairs {
        let Some(Block::Paragraph { content, .. }) = &pair.translated else { continue };
        let t: String = content
            .iter()
            .filter_map(|c| match c {
                InlineContent::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let fffd = t.chars().filter(|&c| c == '\u{FFFD}').count();
        total_fffd += fffd;
        if fffd > 0 || t.chars().any(|c| c as u32 > 0xFFFF) {
            stats(&format!("块 {}", pair.block_id), &t);
        }
    }
    println!("\n全链路 U+FFFD 总数: {total_fffd}");
}
