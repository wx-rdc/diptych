//! 调试工具：打印 PDF → IR 的块序列（类型 + 文本摘要），用于人工审查阅读顺序。
//!
//! cargo run -p geminus-core --example pdf_dump -- <file.pdf> [--full]
#![allow(missing_docs)]

use geminus_core::model::Block;
use geminus_core::parse_pdf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("用法: pdf_dump <file.pdf> [--full]");
    let full = args.iter().any(|a| a == "--full");
    let doc = parse_pdf(std::path::Path::new(path)).expect("解析失败");
    println!("标题: {}  块数: {}  资源: {}", doc.title, doc.blocks.len(), doc.resources.len());
    for b in &doc.blocks {
        let summary = match b {
            Block::Paragraph { style, content, .. } => {
                let text: String = content
                    .iter()
                    .filter_map(|c| match c {
                        geminus_core::model::InlineContent::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let style_tag = match style.heading_level {
                    Some(l) => format!("H{l}"),
                    None => {
                        if style.is_list_item { "LI".into() } else { "P".into() }
                    }
                };
                let shown = if full { text.clone() } else { text.chars().take(70).collect() };
                format!("[{style_tag}] {shown}")
            }
            Block::Image { width, height, .. } => format!("[IMG] {}x{}", width.unwrap_or(0), height.unwrap_or(0)),
            Block::Table { preview_text, .. } => format!("[TBL] {}", preview_text.clone().unwrap_or_default()),
            Block::Formula { .. } => "[FML]".to_string(),
            Block::Empty { .. } => "[EMPTY]".to_string(),
            Block::Skipped { source_type, .. } => format!("[SKIP:{source_type}]"),
            Block::Algorithm { lines, .. } => format!("[ALGO] {} 行", lines.len()),
        };
        println!("  {} {}", b.id(), summary);
    }
}
