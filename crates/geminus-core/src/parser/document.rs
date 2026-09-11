//! DOCX 主解析器：`word/document.xml` 的 `<w:body>` → `Vec<Block>`（docs/design.md §5.1）。

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use thiserror::Error;
use uuid::Uuid;

use crate::model::*;

use super::rels::{normalize_target, parse_rels};
use super::styles::parse_styles;
use super::xml_tree::{parse_xml_to_tree, El, Node};

#[derive(Debug, Error)]
pub enum DocxError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP 读取错误: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("docx 缺少必需条目: {0}")]
    MissingEntry(String),
    #[error("XML 解析错误: {0}")]
    Xml(#[from] super::xml_tree::XmlTreeError),
    #[error("文档结构无效: {0}")]
    Invalid(String),
}

// ────────────────────────── 解析上下文 ──────────────────────────

struct ParseCtx<'a> {
    rels: &'a HashMap<String, String>,
    styles: &'a HashMap<String, u8>,
    media: &'a HashMap<String, Vec<u8>>,
    resources: HashMap<String, Resource>,
    block_seq: usize,
    img_seq: usize,
    formula_seq: usize,
    table_seq: usize,
}

impl<'a> ParseCtx<'a> {
    fn new(
        rels: &'a HashMap<String, String>,
        styles: &'a HashMap<String, u8>,
        media: &'a HashMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            rels,
            styles,
            media,
            resources: HashMap::new(),
            block_seq: 0,
            img_seq: 0,
            formula_seq: 0,
            table_seq: 0,
        }
    }

    fn next_block_id(&mut self) -> String {
        self.block_seq += 1;
        format!("b_{:04}", self.block_seq)
    }

    /// 注册图片资源，返回 resource_id。rels/media 缺失时降级为空资源（不中断解析）。
    fn register_image(&mut self, rid: &str) -> String {
        self.img_seq += 1;
        let resource_id = format!("r{:02}_image", self.img_seq);
        let (mime, data_b64) = self
            .rels
            .get(rid)
            .map(|target| normalize_target(target))
            .and_then(|path| self.media.get(&path))
            .map(|bytes| {
                (
                    mime_from_path(self.rels[rid].as_str()),
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                )
            })
            .unwrap_or_else(|| ("application/octet-stream".to_string(), String::new()));
        self.resources
            .insert(resource_id.clone(), Resource::image(mime, data_b64));
        resource_id
    }

    /// 注册公式资源（OMML 原始 XML；LaTeX 由 math 模块在 T4 接入）
    fn register_formula(&mut self, omml_el: &El) -> String {
        self.formula_seq += 1;
        let resource_id = format!("r{:02}_formula", self.formula_seq);
        let omml = omml_el.to_xml();
        let latex = crate::math::omml_to_latex(&omml);
        self.resources
            .insert(resource_id.clone(), Resource::formula(omml, latex));
        resource_id
    }

    /// 注册表格资源
    fn register_table(&mut self, tbl_el: &El) -> String {
        self.table_seq += 1;
        let resource_id = format!("r{:02}_table", self.table_seq);
        self.resources
            .insert(resource_id.clone(), Resource::table(tbl_el.to_xml()));
        resource_id
    }
}

fn mime_from_path(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 首个图片的几何信息（块级提升用）
#[derive(Clone)]
struct ImageGeom {
    resource_id: String,
    width: Option<u32>,
    height: Option<u32>,
    alt: Option<String>,
}

/// 段落遍历中收集的行内内容 + 首个图片的几何信息
#[derive(Default)]
struct ParagraphScan {
    items: Vec<InlineContent>,
    first_image: Option<ImageGeom>,
}

// ────────────────────────── 对外入口 ──────────────────────────

/// 解析 .docx 文件 → Document IR
pub fn parse_docx(path: &Path) -> Result<Document, DocxError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| DocxError::MissingEntry("word/document.xml".to_string()))?
        .read_to_string(&mut document_xml)?;

    let rels = match archive.by_name("word/_rels/document.xml.rels") {
        Ok(mut f) => {
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            parse_rels(&s)?
        }
        Err(_) => HashMap::new(),
    };

    let styles = match archive.by_name("word/styles.xml") {
        Ok(mut f) => {
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            parse_styles(&s)?
        }
        Err(_) => HashMap::new(),
    };

    let mut media = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.starts_with("word/media/") && !entry.is_dir() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            media.insert(name, bytes);
        }
    }

    let fallback_title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());

    parse_document_content(&document_xml, &rels, &styles, &media, &fallback_title)
}

/// 纯 XML 层解析（供单元测试与 zip 层复用）
pub fn parse_document_content(
    document_xml: &str,
    rels: &HashMap<String, String>,
    styles: &HashMap<String, u8>,
    media: &HashMap<String, Vec<u8>>,
    fallback_title: &str,
) -> Result<Document, DocxError> {
    let root = parse_xml_to_tree(document_xml)?;
    let body = root
        .descendants("w:body")
        .into_iter()
        .next()
        .ok_or_else(|| DocxError::Invalid("缺少 w:body".to_string()))?;

    let mut ctx = ParseCtx::new(rels, styles, media);
    let mut blocks = Vec::new();
    let mut title: Option<String> = None;

    for node in &body.children {
        let Node::El(el) = node else { continue };
        match el.name.as_str() {
            "w:p" => {
                let block = parse_paragraph(el, &mut ctx);
                if title.is_none() {
                    if let Block::Paragraph { style, content, .. } = &block {
                        if style.heading_level.is_some_and(|l| l <= 2) {
                            let text: String = content
                                .iter()
                                .filter_map(|c| match c {
                                    InlineContent::Text { text, .. } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect();
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                title = Some(trimmed.to_string());
                            }
                        }
                    }
                }
                blocks.push(block);
            }
            "w:tbl" => blocks.push(parse_table_block(el, &mut ctx)),
            _ => {} // w:sectPr 等忽略
        }
    }

    Ok(Document {
        id: Uuid::new_v4().to_string(),
        title: title.unwrap_or_else(|| fallback_title.to_string()),
        blocks,
        resources: ctx.resources,
        render_meta: HashMap::new(),
        pdf_meta: None,
    })
}

// ────────────────────────── 段落 ──────────────────────────

fn parse_paragraph(p: &El, ctx: &mut ParseCtx) -> Block {
    let style = parse_style(p, ctx.styles);
    let mut scan = ParagraphScan::default();
    walk_paragraph_children(p, ctx, &mut scan);

    let has_visible_text = scan.items.iter().any(|c| {
        matches!(c, InlineContent::Text { text, .. } if !text.trim().is_empty())
    });
    let image_count = scan
        .items
        .iter()
        .filter(|c| matches!(c, InlineContent::InlineImage { .. }))
        .count();
    let formula_count = scan
        .items
        .iter()
        .filter(|c| matches!(c, InlineContent::InlineFormula { .. }))
        .count();

    let id = ctx.next_block_id();

    if !has_visible_text {
        // 仅含一张图片（其余为空）→ 提升为块级图片
        if image_count == 1 && formula_count == 0 {
            if let InlineContent::InlineImage { resource_id } = &scan.items[image_count - 1] {
                let geom = scan.first_image.clone().unwrap_or(ImageGeom {
                    resource_id: resource_id.clone(),
                    width: None,
                    height: None,
                    alt: None,
                });
                return Block::Image {
                    id,
                    resource_id: geom.resource_id,
                    width: geom.width,
                    height: geom.height,
                    alt_text: geom.alt,
                };
            }
        }
        // 仅含一个公式的独立段（display 公式）→ 提升为公式块
        if formula_count == 1 && image_count == 0 {
            let formula = scan.items.iter().find_map(|c| match c {
                InlineContent::InlineFormula { resource_id } => Some(resource_id.clone()),
                _ => None,
            });
            if let Some(resource_id) = formula {
                let omml = ctx
                    .resources
                    .get(&resource_id)
                    .map(|r| r.omml.clone())
                    .unwrap_or_default();
                return Block::Formula {
                    id,
                    resource_id,
                    omml,
                };
            }
        }
        // 无任何可见资产 → 空段
        if image_count == 0 && formula_count == 0 {
            return Block::Empty { id };
        }
    }

    Block::Paragraph {
        id,
        style,
        content: scan.items,
    }
}

/// 解析 w:pPr → ParagraphStyle
fn parse_style(p: &El, styles: &HashMap<String, u8>) -> ParagraphStyle {
    let mut out = ParagraphStyle::default();
    let Some(ppr) = p.child("w:pPr") else {
        return out;
    };

    if let Some(pstyle) = ppr
        .child("w:pStyle")
        .and_then(|s| s.attr("w:val").or_else(|| s.attr("val")))
    {
        if let Some(level) = styles.get(pstyle) {
            out.heading_level = Some(*level);
        }
        if pstyle.to_lowercase().starts_with("list") {
            out.is_list_item = true;
        }
    }
    if ppr.child("w:numPr").is_some() {
        out.is_list_item = true;
    }
    if let Some(jc) = ppr
        .child("w:jc")
        .and_then(|j| j.attr("w:val").or_else(|| j.attr("val")))
    {
        out.alignment = match jc {
            "center" => Alignment::Center,
            "right" => Alignment::Right,
            "both" | "justified" => Alignment::Justify,
            _ => Alignment::Left,
        };
    }
    if let Some(ind) = ppr.child("w:ind") {
        let twips: i64 = ind
            .attr("w:left")
            .or_else(|| ind.attr("left"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        out.indent_level = (twips / 720).clamp(0, 8) as u8;
    }
    out
}

/// 遍历段落直接子元素（run / hyperlink / 裸公式）
fn walk_paragraph_children(p: &El, ctx: &mut ParseCtx, scan: &mut ParagraphScan) {
    for node in &p.children {
        let Node::El(el) = node else { continue };
        match el.name.as_str() {
            "w:r" => walk_run(el, ctx, scan, RunFlags::default()),
            "w:hyperlink" | "w:smartTag" | "w:ins" => {
                for sub in el.children_named("w:r") {
                    walk_run(sub, ctx, scan, RunFlags::default());
                }
            }
            "m:oMath" => {
                let rid = ctx.register_formula(el);
                scan.items.push(InlineContent::InlineFormula { resource_id: rid });
            }
            "m:oMathPara" => {
                if let Some(om) = el.descendants("m:oMath").into_iter().next() {
                    let rid = ctx.register_formula(om);
                    scan.items.push(InlineContent::InlineFormula { resource_id: rid });
                }
            }
            _ => {} // w:pPr / bookmark 等跳过
        }
    }
}

#[derive(Default, Clone, Copy)]
struct RunFlags {
    bold: bool,
    italic: bool,
    underline: bool,
}

fn parse_run_flags(r: &El) -> RunFlags {
    let Some(rpr) = r.child("w:rPr") else {
        return RunFlags::default();
    };
    let on = |name: &str| -> bool {
        rpr.child(name).is_some_and(|e| {
            !matches!(
                e.attr("w:val").or_else(|| e.attr("val")),
                Some("false") | Some("0") | Some("none")
            )
        })
    };
    RunFlags {
        bold: on("w:b"),
        italic: on("w:i"),
        underline: on("w:u"),
    }
}

/// 遍历 run 子元素
fn walk_run(r: &El, ctx: &mut ParseCtx, scan: &mut ParagraphScan, flags: RunFlags) {
    let flags = {
        let mut f = parse_run_flags(r);
        f.bold |= flags.bold;
        f.italic |= flags.italic;
        f.underline |= flags.underline;
        f
    };

    for node in &r.children {
        let Node::El(el) = node else { continue };
        match el.name.as_str() {
            "w:t" => scan.items.push(InlineContent::Text {
                text: el.text_content(),
                bold: flags.bold,
                italic: flags.italic,
                underline: flags.underline,
            }),
            "w:tab" => scan.items.push(InlineContent::Text {
                text: "\t".to_string(),
                bold: flags.bold,
                italic: flags.italic,
                underline: flags.underline,
            }),
            "w:br" | "w:cr" => scan.items.push(InlineContent::LineBreak),
            "w:drawing" => push_drawing_image(el, ctx, scan),
            "w:pict" => push_vml_image(el, ctx, scan),
            "m:oMath" => {
                let rid = ctx.register_formula(el);
                scan.items.push(InlineContent::InlineFormula { resource_id: rid });
            }
            // DrawingML 常被 mc:AlternateContent 包裹：优先 Choice，降级 Fallback
            "mc:AlternateContent" => {
                let inner = el
                    .child("mc:Choice")
                    .or_else(|| el.child("mc:Fallback"));
                if let Some(inner) = inner {
                    walk_run(inner, ctx, scan, flags);
                }
            }
            _ => {}
        }
    }
}

/// DrawingML 图片：a:blip@r:embed + wp:extent（EMU→px：÷9525）
fn push_drawing_image(drawing: &El, ctx: &mut ParseCtx, scan: &mut ParagraphScan) {
    let Some(blip) = drawing.descendants("a:blip").into_iter().next() else {
        return;
    };
    let Some(rid) = blip.attr("r:embed").or_else(|| blip.attr_local("embed")) else {
        return;
    };
    let resource_id = ctx.register_image(rid);

    let extent = drawing
        .child("wp:extent")
        .or_else(|| drawing.descendants("a:ext").into_iter().next());
    let (width, height) = extent
        .map(|e| {
            let cx: f64 = e
                .attr("cx")
                .or_else(|| e.attr_local("cx"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let cy: f64 = e
                .attr("cy")
                .or_else(|| e.attr_local("cy"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            (
                (cx / 9525.0).round() as u32,
                (cy / 9525.0).round() as u32,
            )
        })
        .unwrap_or((0, 0));
    let (width, height) = if width > 0 && height > 0 {
        (Some(width), Some(height))
    } else {
        (None, None)
    };

    let alt = drawing
        .descendants("wp:docPr")
        .into_iter()
        .next()
        .and_then(|d| {
            d.attr("descr")
                .or_else(|| d.attr("name"))
                .map(|s| s.to_string())
        });

    if scan.first_image.is_none() {
        scan.first_image = Some(ImageGeom {
            resource_id: resource_id.clone(),
            width,
            height,
            alt,
        });
    }
    scan.items.push(InlineContent::InlineImage { resource_id });
}

/// VML 图片：v:imagedata@r:id（本机真实测试文档路径）
fn push_vml_image(pict: &El, ctx: &mut ParseCtx, scan: &mut ParagraphScan) {
    let Some(imgdata) = pict.descendants("v:imagedata").into_iter().next() else {
        return;
    };
    let Some(rid) = imgdata.attr("r:id").or_else(|| imgdata.attr_local("id")) else {
        return;
    };
    let resource_id = ctx.register_image(rid);
    let alt = pict
        .descendants("v:shape")
        .into_iter()
        .next()
        .and_then(|s| s.attr("alt").map(|a| a.to_string()));
    if scan.first_image.is_none() {
        scan.first_image = Some(ImageGeom {
            resource_id: resource_id.clone(),
            width: None,
            height: None,
            alt,
        });
    }
    scan.items.push(InlineContent::InlineImage { resource_id });
}

// ────────────────────────── 表格 ──────────────────────────

fn parse_table_block(tbl: &El, ctx: &mut ParseCtx) -> Block {
    let resource_id = ctx.register_table(tbl);
    let preview = table_preview(tbl);
    Block::Table {
        id: ctx.next_block_id(),
        resource_id,
        preview_text: preview,
    }
}

/// 前 3 行单元格文本拼接（≤120 字符）
fn table_preview(tbl: &El) -> Option<String> {
    let rows: Vec<&El> = tbl.descendants("w:tr").into_iter().take(3).collect();
    let mut parts = Vec::new();
    for row in rows {
        let cells: Vec<String> = row
            .descendants("w:tc")
            .into_iter()
            .map(|tc| tc.text_content().trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if !cells.is_empty() {
            parts.push(cells.join(" "));
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut preview = parts.join(" | ");
    if preview.len() > 120 {
        preview.truncate(120);
    }
    Some(preview)
}

// ────────────────────────── 单元测试（内嵌 XML，不依赖 zip） ──────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_body_xml(inner: &str) -> Document {
        let xml = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:body>{inner}</w:body></w:document>"#
        );
        parse_document_content(&xml, &HashMap::new(), &HashMap::new(), &HashMap::new(), "fallback")
            .unwrap()
    }

    fn parse_with_styles(inner: &str, styles: &HashMap<String, u8>) -> Document {
        let xml = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{inner}</w:body></w:document>"#
        );
        parse_document_content(&xml, &HashMap::new(), styles, &HashMap::new(), "fallback").unwrap()
    }

    fn para_text(b: &Block) -> String {
        match b {
            Block::Paragraph { content, .. } => content
                .iter()
                .filter_map(|c| match c {
                    InlineContent::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect(),
            _ => panic!("不是段落"),
        }
    }

    #[test]
    fn plain_paragraph_text() {
        let doc = parse_body_xml(
            r#"<w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>World</w:t></w:r></w:p>"#,
        );
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(para_text(&doc.blocks[0]), "Hello World");
    }

    #[test]
    fn heading_from_style_map() {
        let mut styles = HashMap::new();
        styles.insert("Heading1".to_string(), 1u8);
        let doc = parse_with_styles(
            r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Chapter</w:t></w:r></w:p>"#,
            &styles,
        );
        match &doc.blocks[0] {
            Block::Paragraph { style, .. } => assert_eq!(style.heading_level, Some(1)),
            _ => panic!(),
        }
        assert_eq!(doc.title, "Chapter");
    }

    #[test]
    fn run_formatting_flags() {
        let doc = parse_body_xml(
            r#"<w:p><w:r><w:rPr><w:b/><w:u/></w:rPr><w:t>bold-under</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>ital</w:t></w:r><w:r><w:t>plain</w:t></w:r></w:p>"#,
        );
        match &doc.blocks[0] {
            Block::Paragraph { content, .. } => {
                assert!(matches!(&content[0], InlineContent::Text { bold: true, underline: true, italic: false, .. }));
                assert!(matches!(&content[1], InlineContent::Text { italic: true, bold: false, .. }));
                assert!(matches!(&content[2], InlineContent::Text { bold: false, italic: false, underline: false, .. }));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn list_item_via_numpr_and_style_prefix() {
        let doc = parse_body_xml(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>"#,
        );
        match &doc.blocks[0] {
            Block::Paragraph { style, .. } => assert!(style.is_list_item),
            _ => panic!(),
        }
        let doc2 = parse_body_xml(
            r#"<w:p><w:pPr><w:pStyle w:val="ListBullet"/></w:pPr><w:r><w:t>bullet</w:t></w:r></w:p>"#,
        );
        match &doc2.blocks[0] {
            Block::Paragraph { style, .. } => assert!(style.is_list_item),
            _ => panic!(),
        }
    }

    #[test]
    fn alignment_and_indent() {
        let doc = parse_body_xml(
            r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>c</w:t></w:r></w:p>
               <w:p><w:pPr><w:jc w:val="both"/></w:pPr><w:r><w:t>j</w:t></w:r></w:p>
               <w:p><w:pPr><w:ind w:left="1440"/></w:pPr><w:r><w:t>i</w:t></w:r></w:p>"#,
        );
        match &doc.blocks[0] {
            Block::Paragraph { style, .. } => assert_eq!(style.alignment, Alignment::Center),
            _ => panic!(),
        }
        match &doc.blocks[1] {
            Block::Paragraph { style, .. } => assert_eq!(style.alignment, Alignment::Justify),
            _ => panic!(),
        }
        match &doc.blocks[2] {
            Block::Paragraph { style, .. } => assert_eq!(style.indent_level, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn drawing_only_paragraph_promotes_to_image_block() {
        let mut rels = HashMap::new();
        rels.insert("rId5".to_string(), "media/image1.png".to_string());
        let mut media = HashMap::new();
        media.insert(
            "word/media/image1.png".to_string(),
            vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A],
        );
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="urn:r" xmlns:a="urn:a" xmlns:wp="urn:wp"><w:body><w:p><w:r><w:drawing><wp:extent cx="952500" cy="571500"/><a:graphic><a:graphicData><pic:pic xmlns:pic="urn:pic"><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></w:drawing></w:r></w:p></w:body></w:document>"#;
        let doc = parse_document_content(xml, &rels, &HashMap::new(), &media, "t").unwrap();
        match &doc.blocks[0] {
            Block::Image { resource_id, width, height, .. } => {
                assert_eq!(width, &Some(100)); // 952500/9525
                assert_eq!(height, &Some(60));
                let res = doc.resources.get(resource_id).unwrap();
                assert_eq!(res.mime, "image/png");
                assert!(!res.data_b64.is_empty());
            }
            other => panic!("应为 Image 块，实际 {other:?}"),
        }
    }

    #[test]
    fn vml_imagedata_inline_image() {
        let mut rels = HashMap::new();
        rels.insert("rId5".to_string(), "media/line.png".to_string());
        let mut media = HashMap::new();
        media.insert("word/media/line.png".to_string(), vec![1, 2, 3]);
        let xml = r#"<w:document xmlns:w="urn:w" xmlns:r="urn:r" xmlns:v="urn:v"><w:body><w:p><w:r><w:t>before </w:t></w:r><w:r><w:pict><v:shape alt="Default Line"><v:imagedata r:id="rId5"/></v:shape></w:pict></w:r><w:r><w:t> after</w:t></w:r></w:p></w:body></w:document>"#;
        let doc = parse_document_content(xml, &rels, &HashMap::new(), &media, "t").unwrap();
        match &doc.blocks[0] {
            Block::Paragraph { content, .. } => {
                assert!(matches!(content[1], InlineContent::InlineImage { .. }));
                let InlineContent::InlineImage { resource_id } = &content[1] else {
                    panic!()
                };
                assert!(doc.resources.contains_key(resource_id));
            }
            other => panic!("应为 Paragraph，实际 {other:?}"),
        }
    }

    #[test]
    fn omml_formula_inline() {
        let doc = parse_body_xml(
            r#"<w:p><w:r><w:t>eq </w:t></w:r><m:oMath><m:r><m:t>E</m:t></m:r></m:oMath></w:p>"#,
        );
        match &doc.blocks[0] {
            Block::Paragraph { content, .. } => {
                assert!(matches!(content[1], InlineContent::InlineFormula { .. }));
                let InlineContent::InlineFormula { resource_id } = &content[1] else {
                    panic!()
                };
                let res = doc.resources.get(resource_id).unwrap();
                assert!(res.omml.contains("m:oMath"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn display_formula_paragraph_promotes_to_formula_block() {
        let doc = parse_body_xml(
            r#"<w:p><m:oMathPara><m:oMath><m:r><m:t>E</m:t></m:r></m:oMath></m:oMathPara></w:p>"#,
        );
        match &doc.blocks[0] {
            Block::Formula { resource_id, omml, .. } => {
                assert!(omml.contains("m:oMath"));
                assert!(doc.resources.contains_key(resource_id));
            }
            other => panic!("应为 Formula 块，实际 {other:?}"),
        }
    }

    #[test]
    fn empty_paragraph() {
        let doc = parse_body_xml(r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr></w:p>"#);
        assert!(matches!(doc.blocks[0], Block::Empty { .. }));
    }

    #[test]
    fn table_block_with_preview() {
        let doc = parse_body_xml(
            r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Age</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Alice</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        match &doc.blocks[0] {
            Block::Table { preview_text, resource_id, .. } => {
                assert_eq!(preview_text.as_deref(), Some("Name Age | Alice"));
                assert!(doc
                    .resources
                    .get(resource_id)
                    .unwrap()
                    .raw_xml
                    .starts_with("<w:tbl"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn title_falls_back_to_file_stem() {
        let doc = parse_body_xml(r#"<w:p><w:r><w:t>plain</w:t></w:r></w:p>"#);
        assert_eq!(doc.title, "fallback");
    }

    #[test]
    fn missing_file_is_error() {
        assert!(parse_docx(Path::new("/nonexistent/file.docx")).is_err());
    }
}
