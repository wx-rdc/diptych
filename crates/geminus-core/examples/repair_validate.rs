//! 真实数据验证：MinerU content_list.json（含 FFFD）+ 同 PDF 内置解析 → 回填。
//! 用法：cargo run -p geminus-core --example repair_validate -- <content_list.json> <origin.pdf>
#![allow(missing_docs)]

use geminus_core::{parse_pdf, mineru::repair};
use geminus_core::model::{Block, Document, InlineContent};
use std::path::Path;

fn main() {
    let cl_path = std::env::args().nth(1).expect("用法: repair_validate <content_list.json> <origin.pdf>");
    let pdf_path = std::env::args().nth(2).expect("缺少 origin.pdf");

    // 1. 用 content_list 的 text 项构造待修复 Document（模拟 adapter 产物）
    let raw = std::fs::read_to_string(&cl_path).expect("读取 content_list");
    let items: serde_json::Value = serde_json::from_str(&raw).expect("解析 content_list");
    let mut doc = Document {
        id: "validate".into(),
        title: "validate".into(),
        blocks: Vec::new(),
        resources: Default::default(),
        render_meta: Default::default(),
        pdf_meta: None,
    };
    for item in items.as_array().expect("content_list 应为数组") {
        if item["type"] == "text" {
            if let Some(text) = item["text"].as_str() {
                doc.blocks.push(Block::Paragraph {
                    id: format!("b_{:04}", doc.blocks.len()),
                    style: Default::default(),
                    content: vec![InlineContent::Text {
                        text: text.to_string(),
                        bold: false,
                        italic: false,
                        underline: false,
                    }],
                });
            }
        }
    }
    let before = repair::count_fffd(&doc);
    println!("修复前 FFFD 字符数: {before}");

    // 2. 内置解析参考 + 修复
    let reference = parse_pdf(Path::new(&pdf_path)).expect("内置解析失败");
    let stats = repair::repair_document(&mut doc, &reference);
    let after = repair::count_fffd(&doc);
    println!(
        "修复后 FFFD 字符数: {after}  (回填 {} 段 / {} 轮, 残余 {} 段, 歧义 {}, 异常 {}, 未锚定 {})",
        stats.filled_runs, stats.passes, stats.residual_runs, stats.ambiguous, stats.bad_capture, stats.unanchored
    );

    // 3. 打印关键句核对
    for block in &doc.blocks {
        let Block::Paragraph { content, .. } = block else { continue };
        for item in content {
            let InlineContent::Text { text, .. } = item else { continue };
            if text.contains("represented") || text.contains("In other words") || text.contains("structure on") {
                println!("-- {text}");
            }
        }
    }
}
