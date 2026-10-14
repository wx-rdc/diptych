//! 实验：数学字母数字区星面字符（𝑁 U+1D441 等）定点归一化为 ASCII/BMP 后，
//! LLM 译文乱码（U+FFFD）是否消除。对照组用未归一化原文。
//!
//! cargo run -p geminus-core --example math_norm_probe -- test/_local/14_EffTrans_Mamba.pdf
#![allow(missing_docs)]

use geminus_core::model::{Block, InlineContent};
use geminus_core::translate::LlmClient;
use geminus_core::{parse_pdf, ApiConfig};

include!("math_map_table.inc");

/// 星面字符定点映射；返回 (归一化文本, 未覆盖的星面字符数)
fn normalize(s: &str) -> (String, usize) {
    let mut out = String::with_capacity(s.len());
    let mut unmapped = 0usize;
    for c in s.chars() {
        let cp = c as u32;
        if cp > 0xFFFF {
            if let Some(&(_, to)) = MATH_MAP.iter().find(|(from, _)| *from == cp) {
                out.push(char::from_u32(to).unwrap());
                continue;
            }
            unmapped += 1;
        }
        out.push(c);
    }
    (out, unmapped)
}

fn stats(tag: &str, s: &str) {
    let fffd = s.chars().filter(|&c| c == '\u{FFFD}').count();
    let astral = s.chars().filter(|c| (*c as u32) > 0xFFFF).count();
    println!("[{tag}] 长度={} U+FFFD={fffd} 星面字符={astral}", s.chars().count());
}

#[tokio::main]
async fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test/_local/14_EffTrans_Mamba.pdf".to_string());
    let doc = parse_pdf(std::path::Path::new(&path)).expect("解析失败");

    let paras: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph { content, .. } => {
                let t: String = content
                    .iter()
                    .filter_map(|c| match c {
                        InlineContent::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                (!t.trim().is_empty()).then_some(t)
            }
            _ => None,
        })
        .collect();

    // 截图对应的句子：两块拼合（行片段按空格连接，与原文连续语义一致）
    let frag1 = paras
        .iter()
        .find(|t| t.contains("represented by"))
        .expect("未找到目标片段1");
    let frag2 = paras
        .iter()
        .find(|t| t.contains("applied independently to each channel"))
        .expect("未找到目标片段2");
    let raw = format!("{frag1} {frag2}");
    let (norm, unmapped) = normalize(&raw);
    assert_eq!(unmapped, 0, "映射表未覆盖 {unmapped} 个星面字符，请扩充 math_map_table.rs");

    println!("=== 对照组源文（未归一化）===\n{raw}\n");
    println!("=== 实验组源文（归一化后）===\n{norm}\n");
    stats("RAW 源文", &raw);
    stats("NORM 源文", &norm);

    let client = LlmClient::new(ApiConfig::from_env());
    let t_raw = client
        .translate_text(&raw, "en", "zh")
        .await
        .expect("RAW 翻译失败");
    let t_norm = client
        .translate_text(&norm, "en", "zh")
        .await
        .expect("NORM 翻译失败");

    println!("\n=== 对照组译文 ===\n{t_raw}");
    println!("\n=== 实验组译文 ===\n{t_norm}");
    stats("RAW 译文", &t_raw);
    stats("NORM 译文", &t_norm);
}
