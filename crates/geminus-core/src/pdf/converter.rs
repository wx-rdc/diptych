//! PDF 结构 → Document IR（v1_plan.md T7）。
//!
//! 输出与 DOCX 解析器完全同构：b_{:04} 块 id、首个标题/文件名为文档标题、
//! 图片入资源池（base64）、样式推断（标题级别/列表/空段插入）。

use std::collections::HashMap;

use base64::Engine as _;

use super::extractor::PdfExtraction;
use super::layout::{self, PageElement};
use super::segmenter::{body_font_size, cluster_lines, heading_level, is_list_marker};
use super::{ImageObject, PdfParagraph};
use crate::model::{Alignment, Block, Document, InlineContent, ParagraphStyle, Resource};

/// 段落级输入（segmenter 输出）+ 提取结果 → IR
pub fn convert_to_document(
    extraction: &PdfExtraction,
    paragraphs: &[PdfParagraph],
    fallback_title: &str,
) -> Document {
    let body_size = body_font_size(paragraphs);
    let mut blocks: Vec<Block> = Vec::new();
    let mut resources: HashMap<String, Resource> = HashMap::new();
    let mut seq: usize = 0;
    let mut next_id = || {
        let id = format!("b_{:04}", seq);
        seq += 1;
        id
    };

    for page in &extraction.pages {
        let page_paras: Vec<PdfParagraph> =
            paragraphs.iter().filter(|p| p.page == page.index).cloned().collect();
        let columns = layout::detect_columns(&cluster_lines(&page.text_blocks));
        let ordered = layout::order_page(&page_paras, &page.images, &columns);

        // 同栏段落间隙 > 3×行距 → 插入 Empty（v1_plan T6.4）
        let empty_gap_threshold = body_size * 1.35 * 3.0;
        let mut prev_para: Option<&PdfParagraph> = None;
        for elem in ordered {
            match elem {
                PageElement::Paragraph(i) => {
                    let Some(p) = page_paras.get(i) else { continue };
                    if let Some(prev) = prev_para {
                        if prev.column.is_some() && prev.column == p.column && p.page == prev.page {
                            let gap = prev.y_bottom - p.y_top;
                            if gap > empty_gap_threshold {
                                blocks.push(Block::Empty { id: next_id() });
                            }
                        }
                    }
                    blocks.push(paragraph_to_block(p, body_size, &mut next_id));
                    prev_para = Some(p);
                }
                PageElement::Image(i) => {
                    let Some(im) = page.images.get(i) else { continue };
                    blocks.push(image_to_block(im, &mut next_id, &mut resources));
                    prev_para = None; // 图片本身是分隔符，其后不做空段判断
                }
            }
        }
    }

    let title = paragraphs
        .iter()
        .find(|p| heading_level(p.font_size, body_size).is_some_and(|l| l <= 2))
        .map(|p| p.text.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| fallback_title.to_string());

    Document {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        blocks,
        resources,
        render_meta: HashMap::new(),
        pdf_meta: None,
    }
}

fn paragraph_to_block(p: &PdfParagraph, body_size: f64, next_id: &mut impl FnMut() -> String) -> Block {
    let hl = heading_level(p.font_size, body_size);
    let style = ParagraphStyle {
        heading_level: hl,
        alignment: if p.spanning && hl.is_some() { Alignment::Center } else { Alignment::Left },
        indent_level: 0,
        is_list_item: is_list_marker(&p.text),
        is_caption: false,
    };
    Block::Paragraph {
        id: next_id(),
        style,
        content: vec![InlineContent::Text {
            text: p.text.clone(),
            bold: p.bold,
            italic: false,
            underline: false,
        }],
    }
}

fn image_to_block(
    im: &ImageObject,
    next_id: &mut impl FnMut() -> String,
    resources: &mut HashMap<String, Resource>,
) -> Block {
    let resource_id = format!("pdf_img_p{}_{}", im.page, resources.len());
    resources.insert(
        resource_id.clone(),
        Resource::image(
            im.mime.clone(),
            base64::engine::general_purpose::STANDARD.encode(&im.data),
        ),
    );
    Block::Image {
        id: next_id(),
        resource_id,
        width: Some(im.px_width),
        height: Some(im.px_height),
        alt_text: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::parse_pdf;
    use std::path::Path;

    fn fixture(name: &str) -> std::path::PathBuf {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/pdf").join(name);
        assert!(p.exists(), "缺少夹具 {name}");
        p
    }

    fn parse(name: &str) -> Document {
        parse_pdf(&fixture(name)).unwrap()
    }

    fn paragraph_texts(doc: &Document) -> Vec<String> {
        doc.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph { content, .. } => Some(
                    content
                        .iter()
                        .filter_map(|c| match c {
                            InlineContent::Text { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()
    }

    fn block_kind(b: &Block) -> &'static str {
        match b {
            Block::Paragraph { .. } => "P",
            Block::Image { .. } => "I",
            Block::Table { .. } => "T",
            Block::Formula { .. } => "F",
            Block::Empty { .. } => "E",
            Block::Skipped { .. } => "S",
            Block::Algorithm { .. } => "A",
        }
    }

    #[test]
    fn simple_pdf_five_paragraphs() {
        let doc = parse("simple.pdf");
        let paras = paragraph_texts(&doc);
        assert_eq!(paras.len(), 5, "5 个段落: {paras:?}");
        for kw in [
            "Machine learning",
            "Natural language",
            "Computer vision",
            "Reinforcement",
            "Transfer learning",
        ] {
            assert!(paras.iter().any(|t| t.contains(kw)), "缺少 {kw}");
        }
        for b in &doc.blocks {
            if let Block::Paragraph { style, .. } = b {
                assert!(style.heading_level.is_none());
                assert!(!style.is_list_item);
            }
        }
        assert_eq!(doc.title, "simple"); // 无标题 → 文件名回退
    }

    #[test]
    fn headings_pdf_styles() {
        let doc = parse("headings.pdf");
        let count = |lvl: Option<u8>| {
            doc.blocks
                .iter()
                .filter(|b| matches!(b, Block::Paragraph { style, .. } if style.heading_level == lvl))
                .count()
        };
        assert_eq!(count(Some(1)), 1, "H1 唯一");
        assert_eq!(count(Some(2)), 1, "H2 唯一");
        assert_eq!(count(Some(3)), 1, "H3 唯一");
        let lists = doc
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Paragraph { style, .. } if style.is_list_item))
            .count();
        assert_eq!(lists, 5, "3 bullet + 2 numbered: 实际 {lists}");
        let texts = paragraph_texts(&doc);
        assert!(texts.iter().any(|t| t.contains("Helvetica")), "Base-14 回退段落存在");
        assert!(doc.title.contains("Chapter One"));
        // Spacer 56 → 空段
        let empties = doc.blocks.iter().filter(|b| matches!(b, Block::Empty { .. })).count();
        assert!(empties >= 1, "大间隙应产生 Empty 块");
    }

    #[test]
    fn with_image_pdf_block_positions() {
        let doc = parse("with_image.pdf");
        let kinds: Vec<&str> = doc.blocks.iter().map(block_kind).collect();
        assert_eq!(kinds, vec!["P", "I", "P", "P", "I", "P"], "{kinds:?}");
        assert_eq!(doc.resources.len(), 2);
        for b in doc.blocks.iter().filter(|b| matches!(b, Block::Image { .. })) {
            let Block::Image { resource_id, width, height, .. } = b else { unreachable!() };
            let res = doc.resources.get(resource_id).unwrap();
            assert_eq!(res.mime, "image/jpeg");
            assert!(!res.data_b64.is_empty());
            assert!(width.unwrap() > 0 && height.unwrap() > 0);
        }
    }

    #[test]
    fn single_image_pdf_all_image_blocks() {
        let doc = parse("single_image.pdf");
        assert_eq!(doc.blocks.len(), 1);
        assert!(matches!(doc.blocks[0], Block::Image { .. }));
    }

    #[test]
    fn empty_pdf_empty_document() {
        let doc = parse("empty.pdf");
        assert!(doc.blocks.is_empty());
        assert!(doc.resources.is_empty());
        assert!(!doc.id.is_empty());
    }

    #[test]
    fn with_formula_and_table_do_not_crash() {
        // 公式/表格以文本形式进入流（MVP 明确不做识别，见 v1_plan §1.2）
        let f = parse("with_formula.pdf");
        assert!(paragraph_texts(&f).iter().any(|t| t.contains("FORMULAMARKER")));
        let t = parse("with_table.pdf");
        assert!(paragraph_texts(&t).iter().any(|x| x.contains("TABLEMARKER") || x.contains("Method")));
    }

    #[test]
    fn two_column_reading_order() {
        let doc = parse("two_column.pdf");
        let paras = paragraph_texts(&doc);
        let left = paras.iter().position(|t| t.contains("LEFTCOL-MARKER")).unwrap();
        let right = paras.iter().position(|t| t.contains("RIGHTCOL-MARKER")).unwrap();
        assert!(left < right, "页 1 左栏先于右栏: #{left} vs #{right}");
        let left2 = paras.iter().position(|t| t.contains("LEFTCOL2-MARKER")).unwrap();
        let right2 = paras.iter().position(|t| t.contains("RIGHTCOL2-MARKER")).unwrap();
        assert!(right < left2, "页 1 右栏先于页 2 左栏");
        assert!(left2 < right2, "页 2 左栏先于右栏");
        assert!(paras[0].contains("Abstract"), "首段应为摘要: {}", &paras[0][..paras[0].len().min(50)]);
    }

    #[test]
    fn mixed_layout_spanning_title_first_then_columns() {
        let doc = parse("mixed_layout.pdf");
        let paras = paragraph_texts(&doc);
        assert!(
            paras[0].contains("MIXEDTITLE-MARKER"),
            "跨栏标题应在最前: {}",
            &paras[0][..paras[0].len().min(60)]
        );
        let left = paras.iter().position(|t| t.contains("MIXEDLEFT-MARKER")).unwrap();
        let right = paras.iter().position(|t| t.contains("MIXEDRIGHT-MARKER")).unwrap();
        assert!(left < right, "左栏先于右栏");
        // 右栏图片块在 MIXEDRIGHT 段之后
        let text_of = |b: &Block| match b {
            Block::Paragraph { content, .. } => content
                .iter()
                .filter_map(|c| match c {
                    InlineContent::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>(),
            _ => String::new(),
        };
        let right_pos = doc
            .blocks
            .iter()
            .position(|b| text_of(b).contains("MIXEDRIGHT-MARKER"))
            .unwrap();
        let img_pos = doc.blocks.iter().position(|b| matches!(b, Block::Image { .. })).expect("应有图片块");
        assert!(right_pos < img_pos, "图片应位于 MIXEDRIGHT 段之后（右栏图）");
    }

    #[test]
    fn block_ids_sequential_unique() {
        let doc = parse("mixed_layout.pdf");
        let ids: Vec<&str> = doc.blocks.iter().map(|b| b.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "id 唯一");
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(*id, format!("b_{:04}", i), "id 连续: {id}");
        }
    }

    #[test]
    fn ir_compatible_with_docx_shape() {
        // 与 DOCX 输出同构：可被翻译管线消费（is_placeholder/paragraph_content 语义）
        let doc = parse("simple.pdf");
        let n_para = doc.blocks.iter().filter(|b| !b.is_placeholder()).count();
        assert_eq!(n_para, 5);
        for b in &doc.blocks {
            if !b.is_placeholder() {
                assert!(b.paragraph_content().is_some());
            }
        }
        // serde 往返（HTTP 传输契约）
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn large_pdf_100_pages_performance() {
        let p = fixture("large.pdf");
        let now = std::time::Instant::now();
        let doc = parse_pdf(&p).unwrap();
        let elapsed = now.elapsed();
        // 每页 = 1 标题段（13pt）+ 1 正文段（12 行等距 15pt 聚为一段）
        assert_eq!(doc.blocks.len(), 200, "100 页 × 2 块: 实际 {}", doc.blocks.len());
        assert!(elapsed.as_secs_f64() < 15.0, "100 页解析应 <15s: {elapsed:?}");
    }

    #[test]
    fn real_paper_end_to_end_ir() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/b2_Prototypical_Networks.pdf");
        if !p.exists() {
            return;
        }
        let doc = parse_pdf(&p).unwrap();
        let paras = paragraph_texts(&doc);
        assert!(paras.len() > 100, "真实论文段落数: {}", paras.len());
        let joined = paras.join(" ");
        assert!(joined.contains("Prototypical Networks"), "标题文本可拼回");
        assert!(joined.contains("few-shot"), "关键术语存在");
        // TeX 逐字碎片可能拆段：全文拼接验证关键内容
        assert!(joined.contains("In this paper"), "摘要文本存在");
        assert!(joined.contains("prototypical"), "核心术语存在");
    }
}
