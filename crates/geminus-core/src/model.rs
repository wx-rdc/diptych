//! 中间表示（IR）数据模型 —— 与 docs/plan.md §3.2 一致。
//!
//! 传输适配（见 docs/design.md §3）：
//! 1. 图片二进制以 base64 字符串（`data_b64`）在 JSON 中传输；
//! 2. `Resource` 为带 `kind` 标签的结构体；公式资源在解析期附带 `latex`。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 整个文档的中间表示
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub blocks: Vec<Block>,
    /// 资源池：图片、公式、表格原始数据
    #[serde(default)]
    pub resources: HashMap<String, Resource>,
    /// PDF 渲染坐标：block_id → 页号/bbox（MinerU 解析填充；DOCX/v1 PDF 无）
    #[serde(default)]
    pub render_meta: HashMap<String, RenderMeta>,
    /// PDF 页面尺寸（原始渲染模式需要；无则不支持原始模式）
    #[serde(default)]
    pub pdf_meta: Option<PdfMeta>,
}

impl Document {
    /// 文档页数（匿名限流计量用）。
    /// PDF 取真实页数；DOCX 无页概念，按文本量估算（≈2600 字符/页，学术排版经验值）。
    pub fn estimated_pages(&self) -> usize {
        if let Some(meta) = &self.pdf_meta {
            return meta.page_count.max(1);
        }
        let chars: usize = self
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph { content, .. } => content
                    .iter()
                    .map(|c| match c {
                        InlineContent::Text { text, .. } => text.chars().count(),
                        _ => 0,
                    })
                    .sum::<usize>(),
                _ => 0,
            })
            .sum();
        chars.div_ceil(2600).max(1)
    }
}

/// PDF 原始渲染的页面信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PdfMeta {
    pub page_count: usize,
    /// 每页 [宽, 高]（PDF 点；与 pdf.js viewport 同一坐标系）
    pub page_sizes: Vec<[f64; 2]>,
}

/// 块级渲染坐标：bbox 原点=左上，PDF 点（docs/mineru-output-spec.md §四）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RenderMeta {
    pub page_idx: usize,
    /// [x1, y1, x2, y2]
    pub bbox: [f64; 4],
}

/// 文档内容块 —— 原文与译文对齐的锚点
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// 文本段落（可翻译）
    Paragraph {
        id: String,
        style: ParagraphStyle,
        content: Vec<InlineContent>,
    },
    /// 图片占位（不翻译）
    Image {
        id: String,
        resource_id: String,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        alt_text: Option<String>,
    },
    /// 表格占位（不翻译）
    Table {
        id: String,
        resource_id: String,
        #[serde(default)]
        preview_text: Option<String>,
    },
    /// 公式占位（不翻译）
    Formula {
        id: String,
        resource_id: String,
        /// OMML 原始 XML（plan.md 契约保留）
        #[serde(default)]
        omml: String,
    },
    /// 空段落（分隔用）
    Empty { id: String },
    /// 解析层显式跳过块（v2.2 T1 无静默丢弃）：不渲染、不翻译，
    /// 但存在于 IR 与覆盖度审计统计中
    Skipped {
        id: String,
        /// MinerU 原始类型名（审计账目键）
        #[serde(default)]
        source_type: String,
        reason: SkipReason,
    },
    /// 算法伪代码块（v2.2 T4）
    Algorithm {
        id: String,
        /// "Algorithm 1 Training episode loss…"（可为空串）
        #[serde(default)]
        header: String,
        lines: Vec<AlgoLine>,
    },
}

/// 跳过原因（v2.2 T1）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkipReason {
    /// 页眉/页脚/页码等装饰性内容（策略性排除）
    Decoration,
    /// 未映射类型（计入覆盖度审计 violations）。
    /// 结构体变体：serde 内部标签不支持 newtype 包原始类型
    Unmapped { type_name: String },
}

/// 算法块的一行（v2.2 T4）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlgoLine {
    pub kind: AlgoLineKind,
    pub segments: Vec<InlineContent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlgoLineKind {
    /// 结构标签行：Input:/Output:/for…do/end for/while/return 等（不翻译）
    Label,
    /// 可翻译语句行
    Stmt,
    /// ▷ 行内注释（可翻译，灰斜体渲染）
    Comment,
}

impl Block {
    pub fn id(&self) -> &str {
        match self {
            Block::Paragraph { id, .. }
            | Block::Image { id, .. }
            | Block::Table { id, .. }
            | Block::Formula { id, .. }
            | Block::Empty { id }
            | Block::Skipped { id, .. }
            | Block::Algorithm { id, .. } => id,
        }
    }

    /// 是否为无需翻译的占位块
    pub fn is_placeholder(&self) -> bool {
        !matches!(self, Block::Paragraph { .. } | Block::Algorithm { .. })
    }

    /// 段落内容引用（非段落返回 None）
    pub fn paragraph_content(&self) -> Option<&Vec<InlineContent>> {
        match self {
            Block::Paragraph { content, .. } => Some(content),
            _ => None,
        }
    }

    /// 算法块行文本（供审计/翻译判断；非算法块返回 None）
    pub fn algorithm_text(&self) -> Option<String> {
        match self {
            Block::Algorithm { header, lines, .. } => {
                let mut out = header.clone();
                for line in lines {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(
                        &line
                            .segments
                            .iter()
                            .filter_map(|c| match c {
                                InlineContent::Text { text, .. } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<String>(),
                    );
                }
                Some(out)
            }
            _ => None,
        }
    }
}

/// 段落内的行内内容
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InlineContent {
    /// 纯文本（可翻译的最小单元）
    Text {
        text: String,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        italic: bool,
        #[serde(default)]
        underline: bool,
    },
    /// 行内图片
    InlineImage { resource_id: String },
    /// 行内公式
    InlineFormula { resource_id: String },
    /// 换行符
    LineBreak,
}

/// 段落样式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParagraphStyle {
    /// None=正文, Some(1)=H1, ...
    #[serde(default)]
    pub heading_level: Option<u8>,
    #[serde(default)]
    pub alignment: Alignment,
    #[serde(default)]
    pub indent_level: u8,
    #[serde(default)]
    pub is_list_item: bool,
    /// v2.2 T3：题注（figure/table caption）样式位
    #[serde(default)]
    pub is_caption: bool,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            heading_level: None,
            alignment: Alignment::Left,
            indent_level: 0,
            is_list_item: false,
            is_caption: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// 资源池条目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub kind: ResourceKind,
    /// Image: MIME 类型（image/png 等）
    #[serde(default)]
    pub mime: String,
    /// Image: base64 编码的字节
    #[serde(default)]
    pub data_b64: String,
    /// Formula: OMML 原始 XML
    #[serde(default)]
    pub omml: String,
    /// Formula: 解析期计算的 LaTeX（失败为 None → 前端降级占位框）
    #[serde(default)]
    pub latex: Option<String>,
    /// Table: 表格原始 XML
    #[serde(default)]
    pub raw_xml: String,
}

impl Resource {
    pub fn image(mime: impl Into<String>, data_b64: impl Into<String>) -> Self {
        Self {
            kind: ResourceKind::Image,
            mime: mime.into(),
            data_b64: data_b64.into(),
            omml: String::new(),
            latex: None,
            raw_xml: String::new(),
        }
    }

    pub fn formula(omml: impl Into<String>, latex: Option<String>) -> Self {
        Self {
            kind: ResourceKind::Formula,
            mime: String::new(),
            data_b64: String::new(),
            omml: omml.into(),
            latex,
            raw_xml: String::new(),
        }
    }

    pub fn table(raw_xml: impl Into<String>) -> Self {
        Self {
            kind: ResourceKind::Table,
            mime: String::new(),
            data_b64: String::new(),
            omml: String::new(),
            latex: None,
            raw_xml: raw_xml.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Image,
    Formula,
    Table,
}

/// 翻译结果：与原文 Block 一一对应
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslatedBlock {
    pub block_id: String,
    pub original: Block,
    /// None = 无需翻译（占位符/空段/翻译失败时为原文副本兜底）
    pub translated: Option<Block>,
    /// true = 翻译失败/配平失败，translated 为原文副本（前端打「未翻译」徽标）
    #[serde(default)]
    pub fallback: bool,
}

/// 翻译后的完整文档
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslatedDocument {
    pub doc_id: String,
    pub pairs: Vec<TranslatedBlock>,
}

/// 翻译进度事件（plan.md §5.2 translate-progress）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslateProgress {
    pub block_id: String,
    pub current: u32,
    pub total: u32,
    pub status: ProgressStatus,
    #[serde(default)]
    pub translated_text: Option<String>,
    /// 错误信息（status=Error 时）
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProgressStatus {
    Translating,
    Done,
    Cached,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_document() -> Document {
        let mut resources = HashMap::new();
        resources.insert(
            "r01_image".to_string(),
            Resource::image("image/png", "aGVsbG8="),
        );
        resources.insert(
            "r02_formula".to_string(),
            Resource::formula("<m:oMath/>", Some("E^{2}".to_string())),
        );
        resources.insert("r03_table".to_string(), Resource::table("<w:tbl/>"));

        Document {
            id: "doc_1".to_string(),
            title: "Sample".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "b_0001".to_string(),
                    style: ParagraphStyle {
                        heading_level: Some(1),
                        alignment: Alignment::Center,
                        indent_level: 0,
                        is_list_item: false,
                        is_caption: false,
                    },
                    content: vec![InlineContent::Text {
                        text: "Title".to_string(),
                        bold: true,
                        italic: false,
                        underline: false,
                    }],
                },
                Block::Paragraph {
                    id: "b_0002".to_string(),
                    style: ParagraphStyle {
                        heading_level: None,
                        alignment: Alignment::Justify,
                        indent_level: 2,
                        is_list_item: true,
                        is_caption: false,
                    },
                    content: vec![
                        InlineContent::Text {
                            text: "see ".to_string(),
                            bold: false,
                            italic: true,
                            underline: true,
                        },
                        InlineContent::InlineImage {
                            resource_id: "r01_image".to_string(),
                        },
                        InlineContent::InlineFormula {
                            resource_id: "r02_formula".to_string(),
                        },
                        InlineContent::LineBreak,
                    ],
                },
                Block::Image {
                    id: "b_0003".to_string(),
                    resource_id: "r01_image".to_string(),
                    width: Some(100),
                    height: Some(60),
                    alt_text: Some("figure".to_string()),
                },
                Block::Table {
                    id: "b_0004".to_string(),
                    resource_id: "r03_table".to_string(),
                    preview_text: Some("Name Age City".to_string()),
                },
                Block::Formula {
                    id: "b_0005".to_string(),
                    resource_id: "r02_formula".to_string(),
                    omml: "<m:oMath/>".to_string(),
                },
                Block::Empty {
                    id: "b_0006".to_string(),
                },
            ],
            resources,
            render_meta: HashMap::new(),
            pdf_meta: None,
        }
    }

    #[test]
    fn serde_document_roundtrip() {
        let doc = sample_document();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn serde_block_tags_are_snake_case() {
        let json = serde_json::to_string(&sample_document()).unwrap();
        for tag in [
            r#""type":"paragraph""#,
            r#""type":"image""#,
            r#""type":"table""#,
            r#""type":"formula""#,
            r#""type":"empty""#,
        ] {
            assert!(json.contains(tag), "缺少标签 {tag}，JSON: {json}");
        }
    }

    #[test]
    fn serde_inline_content_tags() {
        let json = serde_json::to_string(&sample_document()).unwrap();
        for tag in [
            r#""type":"text""#,
            r#""type":"inline_image""#,
            r#""type":"inline_formula""#,
            r#""type":"line_break""#,
        ] {
            assert!(json.contains(tag), "缺少标签 {tag}");
        }
    }

    #[test]
    fn serde_alignment_lowercase() {
        let json = serde_json::to_string(&Alignment::Justify).unwrap();
        assert_eq!(json, r#""justify""#);
        let json = serde_json::to_string(&Alignment::Center).unwrap();
        assert_eq!(json, r#""center""#);
    }

    #[test]
    fn serde_progress_event() {
        let p = TranslateProgress {
            block_id: "b_0002".to_string(),
            current: 3,
            total: 42,
            status: ProgressStatus::Done,
            translated_text: Some("译文".to_string()),
            error: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""status":"done""#));
        let back: TranslateProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn serde_resource_kind_and_fields() {
        let json = serde_json::to_string(&sample_document()).unwrap();
        assert!(json.contains(r#""kind":"image""#));
        assert!(json.contains(r#""kind":"formula""#));
        assert!(json.contains(r#""kind":"table""#));
        assert!(json.contains(r#""latex":"E^{2}""#));
        assert!(json.contains(r#""data_b64":"aGVsbG8=""#));
    }

    #[test]
    fn block_helpers() {
        let doc = sample_document();
        assert_eq!(doc.blocks[0].id(), "b_0001");
        assert!(!doc.blocks[0].is_placeholder());
        assert!(doc.blocks[2].is_placeholder());
        assert!(doc.blocks[5].is_placeholder());
    }

    #[test]
    fn serde_document_with_pdf_meta_and_render_meta_roundtrip() {
        let mut doc = sample_document();
        doc.pdf_meta = Some(PdfMeta {
            page_count: 2,
            page_sizes: vec![[612.0, 792.0], [612.0, 792.0]],
        });
        doc.render_meta.insert(
            "b_0001".into(),
            RenderMeta {
                page_idx: 1,
                bbox: [93.0, 78.0, 679.0, 111.0],
            },
        );
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pdf_meta.as_ref().unwrap().page_count, 2);
        assert_eq!(back.pdf_meta.as_ref().unwrap().page_sizes.len(), 2);
        assert_eq!(back.render_meta["b_0001"].page_idx, 1);
        assert_eq!(back.render_meta["b_0001"].bbox, [93.0, 78.0, 679.0, 111.0]);
    }

    #[test]
    fn old_document_json_without_new_fields_still_parses() {
        // v1 JSON 无 render_meta/pdf_meta 字段 → serde default 生效（向后兼容）
        let json = r#"{"id":"d","title":"t","blocks":[],"resources":{}}"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert!(doc.pdf_meta.is_none());
        assert!(doc.render_meta.is_empty());
    }

    #[test]
    fn old_translated_block_json_without_fallback_still_parses() {
        // v2 及更早的 JSON 无 fallback 字段 → serde default 生效（向后兼容）
        let json = r#"{"block_id":"b","original":{"type":"empty","id":"b"},"translated":null}"#;
        let tb: TranslatedBlock = serde_json::from_str(json).unwrap();
        assert!(!tb.fallback);
    }

    #[test]
    fn serde_translated_document() {
        let doc = sample_document();
        let td = TranslatedDocument {
            doc_id: doc.id.clone(),
            pairs: doc
                .blocks
                .clone()
                .into_iter()
                .map(|b| TranslatedBlock {
                    block_id: b.id().to_string(),
                    translated: if b.is_placeholder() {
                        None
                    } else {
                        Some(b.clone())
                    },
                    original: b,
                    fallback: false,
                })
                .collect(),
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: TranslatedDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(td, back);
        assert_eq!(back.pairs.len(), 6);
        assert!(back.pairs[2].translated.is_none());
        assert!(back.pairs[0].translated.is_some());
    }

    fn text_seg(t: &str) -> InlineContent {
        InlineContent::Text { text: t.into(), bold: false, italic: false, underline: false }
    }

    #[test]
    fn serde_skipped_and_algorithm_variants() {
        let blocks = vec![
            Block::Skipped {
                id: "b_0001".into(),
                source_type: "page_number".into(),
                reason: SkipReason::Decoration,
            },
            Block::Skipped {
                id: "b_0002".into(),
                source_type: "handwriting".into(),
                reason: SkipReason::Unmapped { type_name: "handwriting".into() },
            },
            Block::Algorithm {
                id: "b_0003".into(),
                header: "Algorithm 1 Test".into(),
                lines: vec![AlgoLine {
                    kind: AlgoLineKind::Label,
                    segments: vec![text_seg("Input: D")],
                }],
            },
        ];
        let json = serde_json::to_string(&blocks).unwrap();
        assert!(json.contains(r#""type":"skipped""#));
        assert!(json.contains(r#""type":"algorithm""#));
        assert!(json.contains(r#""kind":"decoration""#));
        assert!(json.contains(r#""kind":"unmapped""#));
        let back: Vec<Block> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, blocks);
        // Skipped 是占位（不翻译不渲染）；Algorithm 不是占位（可翻译）
        assert!(back[0].is_placeholder());
        assert!(!back[2].is_placeholder());
        assert_eq!(back[2].id(), "b_0003");
    }

    #[test]
    fn algorithm_text_joins_lines() {
        let b = Block::Algorithm {
            id: "b_1".into(),
            header: "Algorithm 1".into(),
            lines: vec![
                AlgoLine { kind: AlgoLineKind::Label, segments: vec![text_seg("Output: loss")] },
                AlgoLine {
                    kind: AlgoLineKind::Stmt,
                    segments: vec![
                        text_seg("choose "),
                        InlineContent::InlineFormula { resource_id: "r_eq".into() },
                    ],
                },
            ],
        };
        assert_eq!(b.algorithm_text().unwrap(), "Algorithm 1\nOutput: loss\nchoose ");
        assert!(sample_document().blocks[0].algorithm_text().is_none());
    }

    #[test]
    fn paragraph_style_is_caption_default_false() {
        let json = r#"{"heading_level":null,"alignment":"left","indent_level":0,"is_list_item":false}"#;
        let s: ParagraphStyle = serde_json::from_str(json).unwrap();
        assert!(!s.is_caption); // 旧 JSON 兼容
    }
}
