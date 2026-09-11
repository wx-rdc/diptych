//! 多片段 IR 合并（v2_plan §3.4）：块 id 重编号、资源键加前缀、render_meta 键迁移、
//! 页数累加；跨片段边界做保守段落合并（前末段无句末标点 + 后首段小写开头）。

use std::collections::HashMap;

use crate::model::{Block, Document, InlineContent, PdfMeta};

/// 合并单元：(片段标识, 该片段 IR, 该片段首页全局页号)
/// page 参数用于断言 page_idx 已在 adapter 层偏移（保留语义清晰）
pub fn merge_documents(parts: Vec<(String, Document, usize)>, title: &str) -> Document {
    let mut blocks: Vec<Block> = Vec::new();
    let mut resources: HashMap<String, crate::model::Resource> = HashMap::new();
    let mut render_meta: HashMap<String, crate::model::RenderMeta> = HashMap::new();
    let mut page_sizes: Vec<[f64; 2]> = Vec::new();

    let mut boundaries: Vec<usize> = Vec::new(); // 每片段首块的全局下标
    for (chunk_tag, doc, _start_page) in parts {
        boundaries.push(blocks.len());
        // 块 id 映射表：旧 id → 全局 id
        let mut id_map: HashMap<String, String> = HashMap::new();
        let old_ids: Vec<String> = doc.blocks.iter().map(|b| b.id().to_string()).collect();
        for old_id in old_ids {
            let new_id = format!("b_{:04}", blocks.len() + id_map.len());
            id_map.insert(old_id, new_id);
        }

        // 资源池加前缀（key 与块内引用同步改写）
        let mut res_remap: HashMap<String, String> = HashMap::new();
        let old_res: Vec<String> = doc.resources.keys().cloned().collect();
        for key in old_res {
            if let Some(res) = doc.resources.get(&key) {
                let new_key = format!("{chunk_tag}_{key}");
                resources.insert(new_key.clone(), res.clone());
                res_remap.insert(key, new_key);
            }
        }

        for block in doc.blocks {
            let new_id = id_map.get(block.id()).cloned().unwrap_or_else(|| format!("b_{:04}", blocks.len()));
            let renamed = renumber_block(block, &new_id, &res_remap);
            blocks.push(renamed);
        }
        for (old_id, meta) in doc.render_meta {
            if let Some(new_id) = id_map.get(&old_id) {
                render_meta.insert(new_id.clone(), meta);
            }
        }
        if let Some(pm) = &doc.pdf_meta {
            page_sizes.extend(pm.page_sizes.iter().copied());
        }
    }

    blocks = merge_cross_chunk_boundary(blocks, &boundaries, &mut render_meta);

    Document {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.to_string(),
        blocks,
        resources,
        render_meta,
        pdf_meta: if page_sizes.is_empty() {
            None
        } else {
            Some(PdfMeta { page_count: page_sizes.len(), page_sizes })
        },
    }
}

/// 块 id 重编号 + 资源引用改写
fn renumber_block(
    block: Block,
    new_id: &str,
    res_remap: &HashMap<String, String>,
) -> Block {
    let remap = |rid: &str| -> String {
        res_remap.get(rid).cloned().unwrap_or_else(|| rid.to_string())
    };
    match block {
        Block::Paragraph { id: _, style, content } => {
            let content = content
                .into_iter()
                .map(|c| match c {
                    InlineContent::InlineImage { resource_id } => InlineContent::InlineImage {
                        resource_id: remap(&resource_id),
                    },
                    InlineContent::InlineFormula { resource_id } => InlineContent::InlineFormula {
                        resource_id: remap(&resource_id),
                    },
                    other => other,
                })
                .collect();
            Block::Paragraph { id: new_id.to_string(), style, content }
        }
        Block::Image { id: _, resource_id, width, height, alt_text } => Block::Image {
            id: new_id.to_string(),
            resource_id: remap(&resource_id),
            width,
            height,
            alt_text,
        },
        Block::Table { id: _, resource_id, preview_text } => Block::Table {
            id: new_id.to_string(),
            resource_id: remap(&resource_id),
            preview_text,
        },
        Block::Formula { id: _, resource_id, omml } => Block::Formula {
            id: new_id.to_string(),
            resource_id: remap(&resource_id),
            omml,
        },
        Block::Empty { id: _ } => Block::Empty { id: new_id.to_string() },
        Block::Skipped { id: _, source_type, reason } => Block::Skipped {
            id: new_id.to_string(),
            source_type,
            reason,
        },
        Block::Algorithm { id: _, header, lines } => Block::Algorithm {
            id: new_id.to_string(),
            header,
            lines,
        },
    }
}

/// 跨片段边界的保守段落合并：仅当边界两侧均为正文段、前段文本不以句末标点结束、
/// 且后段以 ASCII 小写开头时合并为一段（render_meta 取前者）。
/// 只作用于片段边界（boundaries 为各片段首块全局下标），片段内部段落不动。
fn merge_cross_chunk_boundary(
    mut blocks: Vec<Block>,
    boundaries: &[usize],
    render_meta: &mut HashMap<String, crate::model::RenderMeta>,
) -> Vec<Block> {
    // 从后往前处理，合并删除块不影响前面边界的下标
    for &b in boundaries.iter().rev() {
        if b == 0 || b >= blocks.len() {
            continue;
        }
        let (prev_plain, next_plain) =
            (is_plain_paragraph(&blocks[b - 1]), is_plain_paragraph(&blocks[b]));
        if prev_plain && next_plain {
            let prev_text = paragraph_text(&blocks[b - 1]).unwrap_or_default();
            let next_text = paragraph_text(&blocks[b]).unwrap_or_default();
            if should_merge_adjacent(&prev_text, &next_text) {
                let (merged, removed_id) = merge_two(&blocks[b - 1], &blocks[b]);
                render_meta.remove(&removed_id);
                blocks.splice(b - 1..b + 1, std::iter::once(merged));
            }
        }
    }
    blocks
}

/// 正文段（非标题、非列表的普通段落；占位/空块天然不是 Paragraph）
fn is_plain_paragraph(b: &Block) -> bool {
    matches!(b, Block::Paragraph { style, .. }
        if style.heading_level.is_none() && !style.is_list_item && style.indent_level == 0)
}

/// 保守合并判定：前段不以句末标点结束 + 后段以小写字母开头
fn should_merge_adjacent(prev: &str, next: &str) -> bool {
    let prev = prev.trim_end();
    let next = next.trim_start();
    if prev.is_empty() || next.is_empty() {
        return false;
    }
    let ends_sentence = prev.ends_with(['.', '!', '?', ':', ';', '。', '！', '？', '：', '；', ')', '）', '"', '”']);
    let starts_lower = next.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    !ends_sentence && starts_lower
}

fn paragraph_text(b: &Block) -> Option<String> {
    match b {
        Block::Paragraph { content, .. } => Some(
            content
                .iter()
                .filter_map(|c| match c {
                    InlineContent::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// 合并两个相邻段（文本以空格连接；样式取前者）
fn merge_two(a: &Block, b: &Block) -> (Block, String) {
    let (id, style, mut content) = match a {
        Block::Paragraph { id, style, content, .. } => (id.clone(), style.clone(), content.clone()),
        _ => unreachable!("调用前已判定为段落"),
    };
    if let Block::Paragraph { content: bc, .. } = b {
        // 前后均为字母/数字结尾/开头时补空格，避免英文单词粘连
        let needs_space = matches!(content.last(), Some(InlineContent::Text { text, .. })
                if text.chars().last().is_some_and(|c| c.is_alphanumeric()))
            && matches!(bc.first(), Some(InlineContent::Text { text, .. })
                if text.chars().next().is_some_and(|c| c.is_alphanumeric()));
        if needs_space {
            content.push(InlineContent::Text {
                text: " ".to_string(),
                bold: false,
                italic: false,
                underline: false,
            });
        }
        content.extend(bc.iter().cloned());
    }
    let removed_id = b.id().to_string();
    (Block::Paragraph { id, style, content }, removed_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InlineContent, ParagraphStyle, RenderMeta, Resource};

    fn text_para(id: &str, text: &str) -> Block {
        Block::Paragraph {
            id: id.to_string(),
            style: ParagraphStyle::default(),
            content: vec![InlineContent::Text {
                text: text.to_string(),
                bold: false,
                italic: false,
                underline: false,
            }],
        }
    }

    fn doc_with_texts(texts: &[&str]) -> Document {
        Document {
            id: "d".into(),
            title: "t".into(),
            blocks: texts.iter().enumerate().map(|(i, t)| text_para(&format!("b_{i:04}"), t)).collect(),
            resources: HashMap::new(),
            render_meta: HashMap::new(),
            pdf_meta: Some(PdfMeta { page_count: texts.len().max(1), page_sizes: vec![[612.0, 792.0]; texts.len().max(1)] }),
        }
    }

    fn text_of(b: &Block) -> String {
        paragraph_text(b).unwrap_or_default()
    }

    #[test]
    fn merges_two_chunks_renumbering_and_resource_prefix() {
        let mut d1 = doc_with_texts(&["One", "Two"]);
        d1.resources.insert("img_0000".into(), Resource::image("image/png", "eHg="));
        let mut d2 = doc_with_texts(&["Three.", "Four."]);
        d2.render_meta.insert("b_0001".into(), RenderMeta { page_idx: 5, bbox: [1.0, 2.0, 3.0, 4.0] });

        let merged = merge_documents(
            vec![("c0".into(), d1, 0), ("c1".into(), d2, 4)],
            "title.pdf",
        );
        assert_eq!(
            merged.blocks.iter().map(|b| b.id().to_string()).collect::<Vec<_>>(),
            ["b_0000", "b_0001", "b_0002", "b_0003"]
        );
        assert!(merged.resources.contains_key("c0_img_0000"));
        // render_meta 键随重编号迁移
        assert_eq!(merged.render_meta["b_0003"].page_idx, 5);
        assert_eq!(merged.pdf_meta.as_ref().unwrap().page_count, 4);
        assert_eq!(merged.title, "title.pdf");
    }

    #[test]
    fn cross_chunk_short_paragraph_merged_conservatively() {
        // 边界：A 末段 "The model achieves"（无句号）+ B 首段 "on three benchmarks"（小写）→ 合并
        // 片段内部同样形态（"First ends." 与 "The model achieves" 之间）不受影响
        let a = doc_with_texts(&["First ends.", "The model achieves"]);
        let b = doc_with_texts(&["on three benchmarks", "Tail sentence."]);
        let merged = merge_documents(vec![("a".into(), a, 0), ("b".into(), b, 1)], "t");
        let texts: Vec<String> = merged.blocks.iter().map(text_of).collect();
        assert_eq!(
            texts,
            vec![
                "First ends.".to_string(),
                "The model achieves on three benchmarks".to_string(),
                "Tail sentence.".to_string(),
            ],
            "texts={texts:?}"
        );
    }

    #[test]
    fn intra_chunk_unfinished_paragraph_not_merged() {
        // 同一片段内部的相邻段即使形态相符也不合并（只处理片段边界）
        let a = doc_with_texts(&["The model achieves", "state of the art results."]);
        let merged = merge_documents(vec![("a".into(), a, 0)], "t");
        assert_eq!(merged.blocks.len(), 2);
    }

    #[test]
    fn cross_chunk_not_merged_when_sentence_complete() {
        let a = doc_with_texts(&["First paragraph ends here."]);
        let b = doc_with_texts(&["Second starts fresh."]);
        let merged = merge_documents(vec![("a".into(), a, 0), ("b".into(), b, 1)], "t");
        assert_eq!(merged.blocks.iter().filter(|b| matches!(b, Block::Paragraph { .. })).count(), 2);
    }

    #[test]
    fn heading_boundary_not_merged() {
        // 后块是标题（heading_level）→ 不合并
        let mut a = doc_with_texts(&["The model achieves"]);
        let b = Document {
            id: "d2".into(),
            title: "t".into(),
            blocks: vec![Block::Paragraph {
                id: "b_0000".into(),
                style: ParagraphStyle { heading_level: Some(1), ..Default::default() },
                content: vec![InlineContent::Text {
                    text: "intro starts".into(),
                    bold: false,
                    italic: false,
                    underline: false,
                }],
            }],
            resources: HashMap::new(),
            render_meta: HashMap::new(),
            pdf_meta: None,
        };
        a.pdf_meta = None;
        let merged = merge_documents(vec![("a".into(), a, 0), ("b".into(), b, 1)], "t");
        assert_eq!(merged.blocks.len(), 2);
    }

    #[test]
    fn render_meta_of_removed_block_dropped() {
        let mut a = doc_with_texts(&["The model achieves"]);
        a.render_meta.insert("b_0000".into(), RenderMeta { page_idx: 0, bbox: [0.0; 4] });
        let mut b = doc_with_texts(&["on benchmarks."]);
        b.render_meta.insert("b_0000".into(), RenderMeta { page_idx: 1, bbox: [9.0; 4] });
        let merged = merge_documents(vec![("a".into(), a, 0), ("b".into(), b, 1)], "t");
        assert_eq!(merged.blocks.len(), 1);
        // 保留前者坐标，后者键移除
        assert!(merged.render_meta.contains_key("b_0000"));
        assert_eq!(merged.render_meta.len(), 1);
        assert_eq!(merged.render_meta["b_0000"].page_idx, 0);
    }

    #[test]
    fn should_merge_adjacent_rules() {
        assert!(should_merge_adjacent("the end", "continues here"));
        assert!(!should_merge_adjacent("the end.", "Continues"));
        assert!(!should_merge_adjacent("the end.", "continues"));
        assert!(!should_merge_adjacent("the end", "Continues"));
        assert!(!should_merge_adjacent("", "continues"));
    }
}
