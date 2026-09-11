//! 译文回填：令牌切分重建 InlineContent 序列（docs/design.md §5.2 降级策略）

use crate::model::{Block, InlineContent};

/// 令牌正则的手工扫描：按 {{Pn}} 切分文本
/// 返回 (片段序列, 令牌出现记录)。片段为 Text 或令牌索引。
enum Segment {
    Text(String),
    Token(usize),
}

fn split_tokens(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut plain = String::new();
    while i < bytes.len() {
        // 查找 "{{P"
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{'
            && i + 2 < bytes.len() && bytes[i + 2] == b'P'
        {
            // 扫描数字与 "}}"
            let mut j = i + 3;
            let mut digits = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                digits.push(bytes[j] as char);
                j += 1;
            }
            if !digits.is_empty()
                && j + 1 < bytes.len()
                && bytes[j] == b'}'
                && bytes[j + 1] == b'}'
            {
                if !plain.is_empty() {
                    segments.push(Segment::Text(std::mem::take(&mut plain)));
                }
                segments.push(Segment::Token(digits.parse().unwrap_or(usize::MAX)));
                i = j + 2;
                continue;
            }
        }
        // 正常字符（UTF-8 安全：只按字节起点在多字节序列内部时不会命中 '{'）
        let ch_len = utf8_char_len(bytes[i]);
        plain.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    if !plain.is_empty() {
        segments.push(Segment::Text(plain));
    }
    segments
}

fn utf8_char_len(byte: u8) -> usize {
    match byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// 重建译文段落。
/// 返回 (新块, 是否降级)。降级 = 令牌被 LLM 破坏（缺失/多余/未知）或译文为空，
/// 此时译文 Text 在前、原行内资产按序追加在后（或整块回退原文）。
pub fn rebuild_paragraph(
    orig: &Block,
    translated_text: &str,
    refs: &[super::prompt::PlaceholderRef],
) -> (Block, bool) {
    let Block::Paragraph { id, style, content } = orig else {
        return (orig.clone(), true); // 非段落：原样返回
    };

    // 译文为空 → 整块回退原文
    if translated_text.trim().is_empty() {
        return (orig.clone(), true);
    }

    // 原 Text run 的格式（取首个 Text）
    let (bold, italic, underline) = content
        .iter()
        .find_map(|c| match c {
            InlineContent::Text { bold, italic, underline, .. } => Some((*bold, *italic, *underline)),
            _ => None,
        })
        .unwrap_or((false, false, false));

    let make_text = |t: &str| InlineContent::Text {
        text: t.to_string(),
        bold,
        italic,
        underline,
    };

    // 令牌校验：译文中出现的令牌集合必须与 refs 完全一致（无缺失/重复/未知）
    let segments = split_tokens(translated_text);
    let mut seen: Vec<usize> = Vec::new();
    let mut valid = true;
    for seg in &segments {
        if let Segment::Token(idx) = seg {
            if *idx >= refs.len() || seen.contains(idx) {
                valid = false;
                break;
            }
            seen.push(*idx);
        }
    }
    if seen.len() != refs.len() {
        valid = false;
    }

    let new_content: Vec<InlineContent> = if valid {
        segments
            .iter()
            .filter_map(|seg| match seg {
                Segment::Text(t) if !t.is_empty() => Some(make_text(t)),
                Segment::Token(idx) => Some(refs[*idx].inline.clone()),
                _ => None,
            })
            .collect()
    } else {
        // 降级：译文整段 Text + 资产按原序追加
        let mut items = vec![make_text(translated_text.trim())];
        items.extend(refs.iter().map(|r| r.inline.clone()));
        items
    };

    (
        Block::Paragraph {
            id: id.clone(),
            style: style.clone(),
            content: new_content,
        },
        !valid,
    )
}

#[cfg(test)]
mod tests {
    use super::super::prompt::PlaceholderRef;
    use super::*;
    use crate::model::{Alignment, ParagraphStyle};

    fn para(content: Vec<InlineContent>, style: ParagraphStyle) -> Block {
        Block::Paragraph { id: "b_0001".to_string(), style, content }
    }

    fn refs(n: usize) -> Vec<PlaceholderRef> {
        (0..n)
            .map(|i| PlaceholderRef {
                token: format!("{{{{P{i}}}}}"),
                inline: InlineContent::InlineImage { resource_id: format!("r{i}_image") },
            })
            .collect()
    }

    fn default_style() -> ParagraphStyle {
        ParagraphStyle { heading_level: Some(2), ..Default::default() }
    }

    #[test]
    fn rebuild_with_intact_tokens() {
        let orig = para(
            vec![
                InlineContent::Text { text: "before ".into(), bold: true, italic: false, underline: false },
                InlineContent::InlineImage { resource_id: "r0_image".into() },
                InlineContent::Text { text: " after".into(), bold: false, italic: false, underline: false },
            ],
            default_style(),
        );
        let (rebuilt, degraded) = rebuild_paragraph(&orig, "之前 {{P0}} 之后", &refs(1));
        assert!(!degraded);
        let Block::Paragraph { id, style, content } = rebuilt else { panic!() };
        assert_eq!(id, "b_0001"); // 锚点 ID 保持
        assert_eq!(style.heading_level, Some(2));
        assert_eq!(content.len(), 3);
        assert!(matches!(&content[0], InlineContent::Text { text, bold: true, .. } if text == "之前 "));
        assert!(matches!(&content[1], InlineContent::InlineImage { resource_id } if resource_id == "r0_image"));
        assert!(matches!(&content[2], InlineContent::Text { text, .. } if text == " 之后"));
    }

    #[test]
    fn degrade_when_token_missing() {
        let orig = para(
            vec![
                InlineContent::Text { text: "a ".into(), bold: false, italic: false, underline: false },
                InlineContent::InlineImage { resource_id: "r0_image".into() },
            ],
            ParagraphStyle::default(),
        );
        let (rebuilt, degraded) = rebuild_paragraph(&orig, "译文没有令牌", &refs(1));
        assert!(degraded);
        let Block::Paragraph { content, .. } = rebuilt else { panic!() };
        assert_eq!(content.len(), 2); // 译文 + 追加的资产
        assert!(matches!(&content[0], InlineContent::Text { text, .. } if text == "译文没有令牌"));
        assert!(matches!(&content[1], InlineContent::InlineImage { .. }));
    }

    #[test]
    fn degrade_when_token_duplicated() {
        let orig = para(
            vec![InlineContent::InlineImage { resource_id: "r0_image".into() }],
            ParagraphStyle::default(),
        );
        let (_, degraded) = rebuild_paragraph(&orig, "{{P0}} 重复 {{P0}}", &refs(1));
        assert!(degraded);
    }

    #[test]
    fn degrade_when_unknown_token_index() {
        let orig = para(
            vec![InlineContent::InlineImage { resource_id: "r0_image".into() }],
            ParagraphStyle::default(),
        );
        let (_, degraded) = rebuild_paragraph(&orig, "{{P5}} 越界", &refs(1));
        assert!(degraded);
    }

    #[test]
    fn empty_translation_falls_back_to_original() {
        let orig = para(
            vec![InlineContent::Text { text: "hello".into(), bold: false, italic: false, underline: false }],
            ParagraphStyle { alignment: Alignment::Center, ..Default::default() },
        );
        let (rebuilt, degraded) = rebuild_paragraph(&orig, "   ", &[]);
        assert!(degraded);
        assert_eq!(rebuilt, orig);
    }

    #[test]
    fn no_placeholder_paragraph_plain_rebuild() {
        let orig = para(
            vec![InlineContent::Text { text: "plain".into(), bold: false, italic: false, underline: false }],
            ParagraphStyle::default(),
        );
        let (rebuilt, degraded) = rebuild_paragraph(&orig, "朴素段落", &[]);
        assert!(!degraded);
        let Block::Paragraph { content, .. } = rebuilt else { panic!() };
        assert_eq!(content.len(), 1);
        assert!(matches!(&content[0], InlineContent::Text { text, .. } if text == "朴素段落"));
    }

    #[test]
    fn token_split_handles_multibyte() {
        let orig = para(
            vec![
                InlineContent::Text { text: "x".into(), bold: false, italic: false, underline: false },
                InlineContent::InlineFormula { resource_id: "r0_formula".into() },
            ],
            ParagraphStyle::default(),
        );
        // 中文 + 令牌混排
        let (rebuilt, degraded) = rebuild_paragraph(&orig, "公式{{P0}}结束", &[PlaceholderRef {
            token: "{{P0}}".into(),
            inline: InlineContent::InlineFormula { resource_id: "r0_formula".into() },
        }]);
        assert!(!degraded);
        let Block::Paragraph { content, .. } = rebuilt else { panic!() };
        assert_eq!(content.len(), 3);
        assert!(matches!(&content[0], InlineContent::Text { text, .. } if text == "公式"));
        assert!(matches!(&content[1], InlineContent::InlineFormula { .. }));
        assert!(matches!(&content[2], InlineContent::Text { text, .. } if text == "结束"));
    }
}
