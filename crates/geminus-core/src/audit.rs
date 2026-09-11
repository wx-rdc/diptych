//! 对齐审计（v2.1_plan T1）：比对原文 IR 与译文 Document，找出管线静默丢失/多出/空译的块。
//! 纯函数，供 /api/debug/audit 与测试使用。

use std::collections::HashMap;

use crate::model::{Block, Document, InlineContent, TranslatedBlock, TranslatedDocument};

/// 审计报告
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct AuditReport {
    /// 原文可翻译段在译文中缺失（block_id）
    pub missing: Vec<String>,
    /// 译文有、原文无的 block_id
    pub extra: Vec<String>,
    /// 译文可见文本为空的可翻译段
    pub empty_translation: Vec<String>,
    /// 回退原文显示的段（翻译失败/配平失败，前端打「未翻译」徽标）
    pub fallbacks: Vec<String>,
    /// 原文块数
    pub original_blocks: usize,
    /// 译文 pairs 数
    pub translated_blocks: usize,
}

/// 段落可见文本（Text 拼接，忽略占位令牌）
fn visible_text(block: &Block) -> String {
    match block.paragraph_content() {
        Some(content) => content
            .iter()
            .filter_map(|c| match c {
                InlineContent::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect(),
        None => String::new(),
    }
}

/// 可翻译段 = 有可见文本的 Paragraph
fn is_translatable(block: &Block) -> bool {
    !visible_text(block).trim().is_empty()
}

/// 按块 id 比对原文与译文（v2.1_plan T1.1 规则）：
/// - 原文可翻译段在译文中无对应 pair，或 Paragraph 的 translated=None 且未标 fallback → missing
/// - 译文 Paragraph 降级为占位符块 → missing（结构性丢失）
/// - fallback=true → fallbacks（不算 missing，但可视化）
/// - 译文可见文本为空 → empty_translation
/// - 译文有原文无 → extra
pub fn alignment_audit(orig: &Document, trans: &TranslatedDocument) -> AuditReport {
    let mut report = AuditReport {
        original_blocks: orig.blocks.len(),
        translated_blocks: trans.pairs.len(),
        ..Default::default()
    };
    let orig_map: HashMap<&str, &Block> = orig.blocks.iter().map(|b| (b.id(), b)).collect();
    let trans_map: HashMap<&str, &TranslatedBlock> =
        trans.pairs.iter().map(|p| (p.block_id.as_str(), p)).collect();

    for (id, block) in &orig_map {
        match trans_map.get(id) {
            None => {
                if is_translatable(block) {
                    report.missing.push((*id).to_string());
                }
            }
            Some(pair) => {
                if pair.fallback {
                    report.fallbacks.push((*id).to_string());
                }
                if let Some(tb) = &pair.translated {
                    if tb.is_placeholder() && !block.is_placeholder() {
                        // 译文降级为占位符 = 结构性丢失
                        report.missing.push((*id).to_string());
                    } else if visible_text(tb).trim().is_empty() && is_translatable(block) {
                        report.empty_translation.push((*id).to_string());
                    }
                } else if is_translatable(block) {
                    // Paragraph 的 translated=None 且未标 fallback = 静默缺失
                    report.missing.push((*id).to_string());
                }
            }
        }
    }
    for id in trans_map.keys() {
        if !orig_map.contains_key(id) {
            report.extra.push((*id).to_string());
        }
    }
    report
}

// ────────────────────────── v2.2 T1：解析覆盖度审计 ──────────────────────────
// 度量「MinerU 给了什么 vs IR 留下了什么」——对齐审计（上文）只能发现翻译层丢失，
// 解析层的静默丢弃两侧同缺、恒 missing=0；覆盖度审计补齐这道盲区。

use crate::mineru::raw::RawItem;

/// 类型级账目（逐逻辑类型）
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct TypeCoverage {
    pub kind: String,
    pub mineru_count: usize,
    pub ir_count: usize,
    pub skipped_count: usize,
}

/// 覆盖度违规项（内容型类型的 ir+skipped 账目对不上原始计数）
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CoverageViolation {
    pub kind: String,
    pub dropped: usize,
    pub sample_bbox: Option<[f64; 4]>,
}

/// 覆盖度报告（三层度量）
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct CoverageReport {
    pub per_type: Vec<TypeCoverage>,
    /// Σ IR 文本字符 / Σ MinerU 内容字符（扣除装饰类型）——门禁 ≥0.98
    pub char_coverage: f64,
    /// Σ 已映射块 bbox 面积 / Σ 内容块 bbox 面积——门禁 ≥0.95
    pub area_coverage: f64,
    pub violations: Vec<CoverageViolation>,
}

/// 装饰类型（不计入内容度量；与 adapter::DECOR_TYPES 一致，v2.3 补 v2 管线同义词）
const DECOR: &[&str] = &[
    "header", "footer", "page_number", "page_footnote", "page_aside_text", "aside_text", "index",
    "page_header", "page_footer",
];

/// IR 侧块的来源类型推导：
/// Paragraph{is_caption}→figure_caption / heading→title / Algorithm→algorithm /
/// Skipped→source_type / 其余按枚举名小写
fn ir_kind_of(block: &Block) -> String {
    match block {
        Block::Paragraph { style, .. } => {
            if style.is_caption {
                "figure_caption".to_string()
            } else if style.heading_level.is_some() {
                "title".to_string()
            } else {
                "text".to_string()
            }
        }
        Block::Image { .. } => "image".into(),
        Block::Table { .. } => "table".into(),
        Block::Formula { .. } => "equation".into(),
        Block::Empty { .. } => "empty".into(),
        Block::Skipped { source_type, .. } => source_type.clone(),
        Block::Algorithm { .. } => "algorithm".into(),
    }
}

fn ir_text_len(block: &Block) -> usize {
    match block {
        Block::Paragraph { content, .. } => content
            .iter()
            .filter_map(|c| if let InlineContent::Text { text, .. } = c {
                Some(text.chars().count())
            } else {
                None
            })
            .sum(),
        Block::Algorithm { header, lines, .. } => {
            header.chars().count()
                + lines
                    .iter()
                    .map(|l| {
                        l.segments
                            .iter()
                            .filter_map(|c| if let InlineContent::Text { text, .. } = c {
                                Some(text.chars().count())
                            } else {
                                None
                            })
                            .sum::<usize>()
                    })
                    .sum::<usize>()
        }
        _ => 0,
    }
}

/// 解析覆盖度审计（纯函数）：IR vs MinerU 原始条目。
/// violations = 内容型类型上 ir_count + skipped_count < mineru_count（含完全未出现的类型）。
pub fn coverage_audit(ir: &Document, raw: &[RawItem]) -> CoverageReport {
    let mut per_type: Vec<TypeCoverage> = Vec::new();
    fn entry<'a>(per_type: &'a mut Vec<TypeCoverage>, kind: &str) -> &'a mut TypeCoverage {
        if let Some(i) = per_type.iter().position(|t| t.kind == kind) {
            &mut per_type[i]
        } else {
            per_type.push(TypeCoverage { kind: kind.into(), ..Default::default() });
            per_type.last_mut().unwrap()
        }
    }
    // 归类口径：raw 侧 chart 归并 image 桶（chart 在 IR 即 Block::Image）；
    // IR 侧 is_caption 段按前驱块区分 table_caption（紧随 Table）/figure_caption（其余）
    fn raw_kind(k: &str) -> &str {
        if k == "chart" {
            "image"
        } else {
            k
        }
    }

    for r in raw {
        entry(&mut per_type, raw_kind(&r.kind)).mineru_count += 1;
    }
    let mut ir_area = 0.0f64;
    let mut prev: Option<&Block> = None;
    // 内容型类型上出现 Unmapped 跳过 = 显式丢弃，本身就是违规（如 algorithm 待实装占位）
    let mut unmapped_kinds: Vec<String> = Vec::new();
    for b in &ir.blocks {
        let kind = match b {
            Block::Paragraph { style, .. } if style.is_caption => {
                if matches!(prev, Some(Block::Table { .. })) {
                    "table_caption".to_string()
                } else {
                    "figure_caption".to_string()
                }
            }
            other => ir_kind_of(other),
        };
        let e = entry(&mut per_type, &kind);
        if matches!(b, Block::Skipped { .. }) {
            e.skipped_count += 1;
            if matches!(
                b,
                Block::Skipped { reason: crate::model::SkipReason::Unmapped { .. }, .. }
            ) && !unmapped_kinds.iter().any(|k| k == &kind)
            {
                unmapped_kinds.push(kind.clone());
            }
        } else if !matches!(b, Block::Empty { .. }) {
            e.ir_count += 1;
        }
        if let Some(m) = ir.render_meta.get(b.id()) {
            if !matches!(b, Block::Skipped { .. }) {
                ir_area += (m.bbox[2] - m.bbox[0]).max(0.0) * (m.bbox[3] - m.bbox[1]).max(0.0);
            }
        }
        prev = Some(b);
    }

    let raw_chars: usize = raw
        .iter()
        .filter(|r| !DECOR.contains(&r.kind.as_str()))
        .map(|r| r.text.chars().count())
        .sum();
    let ir_chars: usize = ir.blocks.iter().map(ir_text_len).sum();
    let raw_area: f64 = raw
        .iter()
        .filter(|r| !DECOR.contains(&r.kind.as_str()))
        .map(|r| r.bbox.map(|b| (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0)).unwrap_or(0.0))
        .sum();

    let mut violations = Vec::new();
    for t in &per_type {
        if DECOR.contains(&t.kind.as_str()) {
            continue;
        }
        if t.ir_count + t.skipped_count < t.mineru_count {
            violations.push(CoverageViolation {
                kind: t.kind.clone(),
                dropped: t.mineru_count - t.ir_count - t.skipped_count,
                sample_bbox: raw.iter().find(|r| r.kind == t.kind).and_then(|r| r.bbox),
            });
        } else if unmapped_kinds.iter().any(|k| k == &t.kind) {
            // 账目平但走了 Unmapped 跳过（如 algorithm 占位）→ 显式丢弃仍算违规
            violations.push(CoverageViolation {
                kind: t.kind.clone(),
                dropped: t.skipped_count,
                sample_bbox: raw.iter().find(|r| r.kind == t.kind).and_then(|r| r.bbox),
            });
        }
    }

    CoverageReport {
        per_type,
        char_coverage: if raw_chars == 0 { 1.0 } else { ir_chars as f64 / raw_chars as f64 },
        area_coverage: if raw_area == 0.0 { 1.0 } else { ir_area / raw_area },
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParagraphStyle;

    fn para(id: &str, text: &str) -> Block {
        Block::Paragraph {
            id: id.into(),
            style: ParagraphStyle::default(),
            content: vec![InlineContent::Text {
                text: text.into(),
                bold: false,
                italic: false,
                underline: false,
            }],
        }
    }

    fn doc(blocks: Vec<Block>) -> Document {
        Document {
            id: "d1".into(),
            title: "t".into(),
            blocks,
            resources: Default::default(),
            render_meta: Default::default(),
            pdf_meta: None,
        }
    }

    fn pair(block: &Block, translated: Option<Block>, fallback: bool) -> TranslatedBlock {
        TranslatedBlock {
            block_id: block.id().to_string(),
            original: block.clone(),
            translated,
            fallback,
        }
    }

    #[test]
    fn clean_translation_reports_nothing() {
        let orig = doc(vec![para("b_0", "hello"), para("b_1", "world")]);
        let trans = TranslatedDocument {
            doc_id: "d1".into(),
            pairs: vec![
                pair(&orig.blocks[0], Some(para("b_0", "你好")), false),
                pair(&orig.blocks[1], Some(para("b_1", "世界")), false),
            ],
        };
        let r = alignment_audit(&orig, &trans);
        assert!(r.missing.is_empty() && r.extra.is_empty() && r.empty_translation.is_empty());
        assert_eq!(r.original_blocks, 2);
        assert_eq!(r.translated_blocks, 2);
    }

    #[test]
    fn missing_paragraph_detected() {
        let orig = doc(vec![para("b_0", "hello"), para("b_1", "world")]);
        let trans = TranslatedDocument {
            doc_id: "d1".into(),
            pairs: vec![pair(&orig.blocks[0], Some(para("b_0", "你好")), false)], // b_1 丢失
        };
        let r = alignment_audit(&orig, &trans);
        assert_eq!(r.missing, vec!["b_1".to_string()]);
    }

    #[test]
    fn placeholder_none_is_not_missing_but_fallback_listed() {
        let orig = doc(vec![para("b_0", "hello"), Block::Empty { id: "b_1".into() }]);
        // 占位块 translated=None 合法；b_0 fallback=true 进 fallbacks 不进 missing
        let trans = TranslatedDocument {
            doc_id: "d1".into(),
            pairs: vec![
                pair(&orig.blocks[0], Some(para("b_0", "hello")), true),
                pair(&orig.blocks[1], None, false),
            ],
        };
        let r = alignment_audit(&orig, &trans);
        assert!(r.missing.is_empty(), "{r:?}");
        assert_eq!(r.fallbacks, vec!["b_0".to_string()]);
    }

    #[test]
    fn empty_translation_and_extra_detected() {
        let orig = doc(vec![para("b_0", "hello")]);
        let ghost = para("b_9", "幽灵");
        let trans = TranslatedDocument {
            doc_id: "d1".into(),
            pairs: vec![
                pair(&orig.blocks[0], Some(para("b_0", "  ")), false), // 空译文
                pair(&ghost, Some(ghost.clone()), false),               // 多出
            ],
        };
        let r = alignment_audit(&orig, &trans);
        assert_eq!(r.empty_translation, vec!["b_0".to_string()]);
        assert_eq!(r.extra, vec!["b_9".to_string()]);
    }

    #[test]
    fn paragraph_translated_none_without_fallback_is_missing() {
        let orig = doc(vec![para("b_0", "hello")]);
        let trans = TranslatedDocument {
            doc_id: "d1".into(),
            pairs: vec![pair(&orig.blocks[0], None, false)],
        };
        assert_eq!(alignment_audit(&orig, &trans).missing, vec!["b_0".to_string()]);
    }

    #[test]
    fn degraded_to_placeholder_is_missing() {
        let orig = doc(vec![para("b_0", "hello")]);
        let degraded = Block::Empty { id: "b_0".into() };
        let trans = TranslatedDocument {
            doc_id: "d1".into(),
            pairs: vec![pair(&orig.blocks[0], Some(degraded), false)],
        };
        assert_eq!(alignment_audit(&orig, &trans).missing, vec!["b_0".to_string()]);
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use crate::mineru::raw::RawItem;
    use crate::model::{ParagraphStyle, PdfMeta, RenderMeta};

    fn item(kind: &str, text: &str, bbox: Option<[f64; 4]>) -> RawItem {
        RawItem { kind: kind.into(), text: text.into(), page_idx: 0, bbox }
    }

    fn para_block(id: &str, text: &str) -> Block {
        Block::Paragraph {
            id: id.into(),
            style: ParagraphStyle::default(),
            content: vec![InlineContent::Text {
                text: text.into(),
                bold: false,
                italic: false,
                underline: false,
            }],
        }
    }

    fn doc_with(blocks: Vec<Block>, metas: Vec<(&str, [f64; 4])>) -> Document {
        let mut render_meta = HashMap::new();
        for (id, bbox) in metas {
            render_meta.insert(id.to_string(), RenderMeta { page_idx: 0, bbox });
        }
        Document {
            id: "d".into(),
            title: "t".into(),
            blocks,
            resources: Default::default(),
            render_meta,
            pdf_meta: Some(PdfMeta { page_count: 1, page_sizes: vec![[612.0, 792.0]] }),
        }
    }

    #[test]
    fn full_coverage_no_violations() {
        let raw = vec![
            item("text", "hello world", Some([10.0, 10.0, 100.0, 30.0])),
            item("image", "", Some([10.0, 40.0, 100.0, 140.0])),
            item("page_number", "3", None), // 装饰：不计入内容度量
        ];
        let doc = doc_with(
            vec![
                para_block("b_0", "hello world"),
                Block::Image {
                    id: "b_1".into(),
                    resource_id: "r".into(),
                    width: None,
                    height: None,
                    alt_text: None,
                },
                Block::Skipped {
                    id: "b_2".into(),
                    source_type: "page_number".into(),
                    reason: crate::model::SkipReason::Decoration,
                },
            ],
            vec![("b_0", [10.0, 10.0, 100.0, 30.0]), ("b_1", [10.0, 40.0, 100.0, 140.0])],
        );
        let r = coverage_audit(&doc, &raw);
        assert!(r.violations.is_empty(), "{r:?}");
        assert!(r.char_coverage >= 0.99);
        assert!(r.area_coverage >= 0.99);
        // 装饰类型进账目但不进 violations
        let pn = r.per_type.iter().find(|t| t.kind == "page_number").unwrap();
        assert_eq!((pn.mineru_count, pn.skipped_count), (1, 1));
    }

    #[test]
    fn dropped_caption_and_algorithm_flagged() {
        let raw = vec![
            item("figure_caption", "Figure 1: demo", Some([10.0, 10.0, 90.0, 20.0])),
            item("algorithm", "Output: loss\nfor k do", Some([10.0, 30.0, 90.0, 80.0])),
        ];
        let doc = doc_with(vec![], vec![]);
        let r = coverage_audit(&doc, &raw);
        let viol_kinds: Vec<&str> = r.violations.iter().map(|v| v.kind.as_str()).collect();
        assert!(viol_kinds.contains(&"figure_caption"));
        assert!(viol_kinds.contains(&"algorithm"));
        assert!(r.char_coverage < 0.98);
        assert!(r.area_coverage < 0.95);
        assert!(r.violations.iter().all(|v| v.dropped >= 1));
    }

    #[test]
    fn unmapped_skip_is_violation_and_lowers_char_coverage() {
        // chart 显式 Skipped{Unmapped}：账目平但属内容型显式丢弃 → 违规 + 字符覆盖度低
        let raw = vec![
            item("text", "hello", Some([10.0, 10.0, 90.0, 20.0])),
            item("chart", "chart text lost", Some([10.0, 30.0, 90.0, 80.0])),
        ];
        let doc = doc_with(
            vec![
                para_block("b_0", "hello"),
                Block::Skipped {
                    id: "b_1".into(),
                    source_type: "chart".into(),
                    reason: crate::model::SkipReason::Unmapped { type_name: "chart".into() },
                },
            ],
            vec![("b_0", [10.0, 10.0, 90.0, 20.0])],
        );
        let r = coverage_audit(&doc, &raw);
        let kinds: Vec<&str> = r.violations.iter().map(|v| v.kind.as_str()).collect();
        assert!(kinds.contains(&"chart"), "{r:?}");
        assert!(r.char_coverage < 0.98, "char={}", r.char_coverage);
    }

    #[test]
    fn decoration_skip_is_not_violation() {
        let raw = vec![
            item("text", "hello", Some([10.0, 10.0, 90.0, 20.0])),
            item("page_number", "3", None),
        ];
        let doc = doc_with(
            vec![
                para_block("b_0", "hello"),
                Block::Skipped {
                    id: "b_1".into(),
                    source_type: "page_number".into(),
                    reason: crate::model::SkipReason::Decoration,
                },
            ],
            vec![("b_0", [10.0, 10.0, 90.0, 20.0])],
        );
        let r = coverage_audit(&doc, &raw);
        assert!(r.violations.is_empty(), "{r:?}");
    }

    #[test]
    fn empty_raw_reports_full_coverage() {
        let doc = doc_with(vec![], vec![]);
        let r = coverage_audit(&doc, &[]);
        assert_eq!(r.char_coverage, 1.0);
        assert_eq!(r.area_coverage, 1.0);
        assert!(r.violations.is_empty());
    }
}
