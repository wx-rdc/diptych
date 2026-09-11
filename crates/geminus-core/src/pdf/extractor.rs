//! 页面遍历与文本块/图片提取（v1_plan.md T3/T4）。
//!
//! 自建内容流状态机：CTM/Tm/Tlm/字形宽度推进（规范 `tx = w0×Tfs×Th`），
//! q/Q 图形状态栈（reportlab 等生成器以 `q cm … Q` 包裹块），
//! ToUnicode 解码 + latin-1 回退，TeX 逐字碎片由 segmenter 按 x 序合并。

use std::collections::HashMap;
use std::path::Path;

use pdf::content::{Matrix, Op, Point, TextDrawAdjusted, TextMode};
use pdf::enc::StreamFilter;
use pdf::file::FileOptions;
use pdf::font::Font;
use pdf::object::{Resolve, XObject};
use pdf::primitive::PdfString;

use super::{ImageObject, PdfError, TextBlock};

/// 单页提取结果
#[derive(Debug, Clone, Default)]
pub struct PageContent {
    pub index: usize,
    pub width: f64,
    pub height: f64,
    pub text_blocks: Vec<TextBlock>,
    pub images: Vec<ImageObject>,
}

/// 全文档提取结果
#[derive(Debug, Clone, Default)]
pub struct PdfExtraction {
    pub pages: Vec<PageContent>,
    pub page_count: usize,
}

/// 字体信息：解码表 + 宽度表
struct FontInfo {
    is_cid: bool,
    map: HashMap<u16, String>,
    widths: HashMap<u16, f32>,
}

impl FontInfo {
    fn load(font: &Font, resolve: &impl Resolve) -> Self {
        let mut map = HashMap::new();
        if let Some(Ok(touni)) = font.to_unicode(resolve) {
            for (code, s) in touni.iter() {
                map.insert(code, s.to_string());
            }
        }
        let mut widths = HashMap::new();
        if let Ok(Some(w)) = font.widths(resolve) {
            // 简单字体 FirstChar..LastChar 通常 <256；CID 主体也远小于 65536
            for cid in 0..=65535u32 {
                let wv = w.get(cid as usize);
                if wv != 0.0 {
                    widths.insert(cid as u16, wv);
                }
                if widths.len() >= 4096 {
                    break;
                }
            }
        }
        Self { is_cid: font.is_cid(), map, widths }
    }

    fn decode(&self, s: &PdfString) -> String {
        let bytes = s.as_bytes();
        let mut out = String::with_capacity(bytes.len());
        if self.is_cid {
            for pair in bytes.chunks(2) {
                let code = if pair.len() == 2 {
                    u16::from_be_bytes([pair[0], pair[1]])
                } else {
                    pair[0] as u16
                };
                out.push_str(self.map.get(&code).map(String::as_str).unwrap_or("\u{FFFD}"));
            }
        } else {
            for &b in bytes {
                out.push_str(self.map.get(&(b as u16)).map(String::as_str).unwrap_or(&latin1(b)));
            }
        }
        out
    }

    /// 字符串宽度（单位 = 字号倍数，即千分之一宽度/1000）
    fn width(&self, s: &PdfString) -> f32 {
        let bytes = s.as_bytes();
        let codes: Vec<u16> = if self.is_cid {
            bytes
                .chunks(2)
                .map(|p| {
                    if p.len() == 2 {
                        u16::from_be_bytes([p[0], p[1]])
                    } else {
                        p[0] as u16
                    }
                })
                .collect()
        } else {
            bytes.iter().map(|b| *b as u16).collect()
        };
        codes
            .iter()
            .map(|c| self.widths.get(c).copied().unwrap_or(500.0))
            .sum::<f32>()
            / 1000.0
    }
}

fn latin1(b: u8) -> String {
    match b {
        // TeX 文本字体编码的连字（无 ToUnicode 的 pdftex 老 PDF 常见）
        11 => "ff".to_string(),
        12 => "fi".to_string(),
        13 => "fl".to_string(),
        14 => "ffi".to_string(),
        15 => "ffl".to_string(),
        _ => (b as char).to_string(),
    }
}

/// 矩阵乘法（行向量约定：mul(a,b) = 先 a 后 b）
fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    Matrix {
        a: a.a * b.a + a.b * b.c,
        b: a.a * b.b + a.b * b.d,
        c: a.c * b.a + a.d * b.c,
        d: a.c * b.b + a.d * b.d,
        e: a.e * b.a + a.f * b.c + b.e,
        f: a.e * b.b + a.f * b.d + b.f,
    }
}

fn translate(m: &Matrix, tx: f32, ty: f32) -> Matrix {
    mul(
        &Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty },
        m,
    )
}

/// 页眉页脚边缘带（页面坐标 y > height-30 或 y < 30 的基线丢弃）
const EDGE_BAND: f64 = 30.0;

/// 提取整个 PDF 的文本块与图片
pub fn extract(path: &Path) -> Result<PdfExtraction, PdfError> {
    let file = match FileOptions::cached().open(path) {
        Ok(f) => f,
        Err(e) => {
            if matches!(e, pdf::error::PdfError::InvalidPassword) {
                return Err(PdfError::Encrypted);
            }
            if let pdf::error::PdfError::Io { source } = e {
                return Err(PdfError::Io(source));
            }
            return Err(e.into());
        }
    };
    let resolve = file.resolver();

    let mut pages = Vec::new();
    for (idx, page) in file.pages().enumerate() {
        let page = page?;
        let media = page.media_box()?;
        let width = (media.right - media.left) as f64;
        let height = (media.top - media.bottom) as f64;

        let mut content = PageContent {
            index: idx,
            width,
            height,
            text_blocks: Vec::new(),
            images: Vec::new(),
        };

        let resources = page.resources()?;
        let mut fonts: HashMap<String, FontInfo> = HashMap::new();
        for (name, lazy) in &resources.fonts {
            if let Ok(f) = lazy.load(&resolve) {
                fonts.insert(name.as_str().to_string(), FontInfo::load(&f, &resolve));
            }
        }

        // 图片资源名 → XObject（用于 Do 操作符取位）
        let mut image_refs: HashMap<String, pdf::object::Ref<XObject>> = HashMap::new();
        for (name, r) in &resources.xobjects {
            image_refs.insert(name.as_str().to_string(), *r);
        }

        if let Some(pc) = &page.contents {
            let ops = pc.operations(&resolve)?;
            run_ops(&ops, &fonts, &image_refs, &resolve, idx, width, height, &mut content);
        }

        pages.push(content);
    }

    let page_count = pages.len();
    Ok(PdfExtraction { pages, page_count })
}

#[allow(clippy::too_many_arguments)]
fn run_ops(
    ops: &[Op],
    fonts: &HashMap<String, FontInfo>,
    image_refs: &HashMap<String, pdf::object::Ref<XObject>>,
    resolve: &impl Resolve,
    page: usize,
    page_w: f64,
    page_h: f64,
    out: &mut PageContent,
) {
    let default_font = FontInfo { is_cid: false, map: HashMap::new(), widths: HashMap::new() };

    let mut ctm = Matrix::default();
    let mut tm = Matrix::default();
    let mut tlm = Matrix::default();
    let mut leading = 0.0f32;
    let mut horiz = 1.0f32;
    let mut rise = 0.0f32;
    let mut render_invisible = false;
    let mut cur_font: Option<(String, f32)> = None;
    let mut stack: Vec<GsState> = Vec::new();

    for op in ops {
        match op {
            Op::Save => stack.push(GsState { ctm, cur_font: cur_font.clone(), leading, horiz, rise, render_invisible }),
            Op::Restore => {
                if let Some(s) = stack.pop() {
                    ctm = s.ctm;
                    cur_font = s.cur_font;
                    leading = s.leading;
                    horiz = s.horiz;
                    rise = s.rise;
                    render_invisible = s.render_invisible;
                }
            }
            Op::Transform { matrix } => ctm = mul(matrix, &ctm),
            Op::BeginText => {
                tm = Matrix::default();
                tlm = Matrix::default();
            }
            Op::EndText => {}
            Op::TextFont { name, size } => cur_font = Some((name.as_str().to_string(), *size)),
            Op::Leading { leading: l } => leading = *l,
            Op::TextScaling { horiz_scale } => horiz = *horiz_scale / 100.0,
            Op::TextRise { rise: r } => rise = *r,
            Op::TextRenderMode { mode } => render_invisible = *mode == TextMode::Invisible,
            Op::SetTextMatrix { matrix } => {
                tlm = *matrix;
                tm = tlm;
            }
            Op::MoveTextPosition { translation: Point { x, y } } => {
                tlm = translate(&tlm, *x, *y);
                tm = tlm;
            }
            Op::TextNewline => {
                tlm = translate(&tlm, 0.0, -leading);
                tm = tlm;
            }
            Op::TextDraw { text } => {
                let fs = cur_font.as_ref().map(|f| f.1).unwrap_or(0.0);
                let adv = show_text(text, fonts, &default_font, &cur_font, &ctm, &tm, rise, horiz, render_invisible, page, page_w, page_h, out);
                tm = translate(&tm, adv * fs * horiz, 0.0);
            }
            Op::TextDrawAdjusted { array } => {
                let fs = cur_font.as_ref().map(|f| f.1).unwrap_or(0.0);
                for item in array {
                    match item {
                        TextDrawAdjusted::Text(text) => {
                            let adv = show_text(text, fonts, &default_font, &cur_font, &ctm, &tm, rise, horiz, render_invisible, page, page_w, page_h, out);
                            tm = translate(&tm, adv * fs * horiz, 0.0);
                        }
                        TextDrawAdjusted::Spacing(s) => {
                            tm = translate(&tm, -*s / 1000.0 * fs * horiz, 0.0);
                        }
                    }
                }
            }
            Op::XObject { name } => {
                if let Some(r) = image_refs.get(name.as_str()) {
                    if let Ok(obj) = resolve.get(*r) {
                        if let XObject::Image(im) = &*obj {
                            if let Ok((data, filter)) = im.raw_image_data(resolve) {
                                let mime_data: Option<(&str, Vec<u8>)> = match filter {
                                    Some(StreamFilter::DCTDecode(_)) => Some(("image/jpeg", data.to_vec())),
                                    Some(StreamFilter::FlateDecode(_)) => reencode_flate(im, &data),
                                    _ => None, // JBIG2/JPX/CCITT：MVP 跳过（见 pdf-lib-eval.md 限制 3）
                                };
                                if let Some((mime, data)) = mime_data {
                                    out.images.push(ImageObject {
                                        data,
                                        mime: mime.to_string(),
                                        x: ctm.e as f64,
                                        y: ctm.f as f64,
                                        width: ctm.a as f64,
                                        height: ctm.d as f64,
                                        px_width: im.width,
                                        px_height: im.height,
                                        page,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Op::InlineImage { .. } => {} // MVP 跳过内联图片（夹具与主流论文均用 XObject）
            _ => {}
        }
    }
}

/// Flate 压缩采样 → PNG（Gray/RGB/CMYK 8bit 与 1bit 灰度；Indexed 等暂不支持）。
///
/// pdf-rs 将 FlateDecode 视作"图片滤镜"不解码（raw_image_data 返回压缩流），
/// 此处自行 inflate 后按 ColorSpace 重组。
fn reencode_flate(im: &pdf::object::ImageXObject, data: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    use pdf::object::ColorSpace;
    use std::io::Read as _;
    let mut samples = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut samples)
        .ok()?;
    let w = im.width;
    let h = im.height;
    let bits = im.bits_per_component.unwrap_or(8);
    let cs = im.color_space.as_ref()?;
    let png = match (cs, bits) {
        (ColorSpace::DeviceGray, 1) => {
            super::png::encode_png(&super::png::expand_gray1(&samples, (w * h) as usize, im.image_mask), w, h, 0)
        }
        (ColorSpace::DeviceGray, 8) if samples.len() >= (w * h) as usize => super::png::encode_png(&samples, w, h, 0),
        (ColorSpace::DeviceRGB, 8) if samples.len() >= (w * h * 3) as usize => super::png::encode_png(&samples, w, h, 2),
        (ColorSpace::DeviceCMYK, 8) if samples.len() >= (w * h * 4) as usize => {
            super::png::encode_png(&super::png::cmyk_to_rgb(&samples), w, h, 2)
        }
        _ => return None, // 尺寸不符/Indexed/16bit 等：MVP 跳过
    };
    Some(("image/png", png))
}

struct GsState {
    ctm: Matrix,
    cur_font: Option<(String, f32)>,
    leading: f32,
    horiz: f32,
    rise: f32,
    render_invisible: bool,
}

/// 输出一个文本运行；返回推进宽度（字号倍数）
#[allow(clippy::too_many_arguments)]
fn show_text(
    text: &PdfString,
    fonts: &HashMap<String, FontInfo>,
    default_font: &FontInfo,
    cur_font: &Option<(String, f32)>,
    ctm: &Matrix,
    tm: &Matrix,
    rise: f32,
    horiz: f32,
    invisible: bool,
    page: usize,
    page_w: f64,
    page_h: f64,
    out: &mut PageContent,
) -> f32 {
    let (fname, fsize) = cur_font.clone().unwrap_or_default();
    let info = fonts.get(&fname).unwrap_or(default_font);
    let decoded = info.decode(text);
    if invisible || decoded.trim().is_empty() {
        return info.width(text);
    }
    let trm = mul(tm, ctm);
    let size = (fsize * trm.d.abs()).max(1e-3) as f64;
    let x = trm.e as f64;
    let y = (trm.f + rise * trm.d.abs()) as f64;
    // 页眉页脚带与旋转文本（侧边水印/邮戳）跳过
    let rotated = trm.b.abs() > 0.01 || trm.c.abs() > 0.01;
    if rotated || y > page_h - EDGE_BAND || y < EDGE_BAND || x < -1.0 || x > page_w {
        return info.width(text);
    }
    let width = (info.width(text) * fsize * horiz) as f64;
    out.text_blocks.push(TextBlock {
        text: decoded,
        x,
        y,
        width,
        font_size: size,
        bold: fname.to_lowercase().contains("bold"),
        font_name: fname,
        page,
    });
    info.width(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::PdfError;

    fn fixture(name: &str) -> std::path::PathBuf {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/pdf")
            .join(name);
        assert!(p.exists(), "缺少夹具 {name}");
        p
    }

    #[test]
    fn simple_pdf_text_blocks_complete() {
        let ex = extract(&fixture("simple.pdf")).unwrap();
        assert_eq!(ex.page_count, 1);
        let all: String = ex.pages[0]
            .text_blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for kw in ["Machine learning", "Natural language", "Computer vision", "Reinforcement", "Transfer learning"] {
            assert!(all.contains(kw), "缺少 {kw}");
        }
        // 坐标在页面内（letter 612x792）
        for b in &ex.pages[0].text_blocks {
            assert!((0.0..=612.0).contains(&b.x), "x 越界: {}", b.x);
            assert!((0.0..=792.0).contains(&b.y), "y 越界: {}", b.y);
            assert!(b.font_size > 0.0 && b.font_size < 100.0);
        }
    }

    #[test]
    fn simple_pdf_has_five_paragraph_groups() {
        // 5 段 × 3 行：基线 y 去重后应恰有 15 个行基线（每段行距 15、段距 29）
        let ex = extract(&fixture("simple.pdf")).unwrap();
        let mut ys: Vec<f64> = ex.pages[0]
            .text_blocks
            .iter()
            .map(|b| (b.y * 10.0).round() / 10.0)
            .collect();
        ys.sort_by(|a, b| b.partial_cmp(a).unwrap());
        ys.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        assert_eq!(ys.len(), 15, "行数 {ys:?}");
        // 段落间距 ≈29，行距 15
        let gaps: Vec<f64> = ys.windows(2).map(|w| w[0] - w[1]).collect();
        let para_gaps = gaps.iter().filter(|g| **g > 20.0).count();
        assert_eq!(para_gaps, 4, "段间距数 {gaps:?}");
    }

    #[test]
    fn with_image_pdf_extracts_two_jpegs() {
        let ex = extract(&fixture("with_image.pdf")).unwrap();
        assert_eq!(ex.page_count, 2);
        let imgs: Vec<&ImageObject> = ex.pages.iter().flat_map(|p| p.images.iter()).collect();
        assert_eq!(imgs.len(), 2);
        assert!(imgs.iter().all(|i| i.mime == "image/jpeg"));
        assert_eq!(imgs[0].px_width, 160);
        assert_eq!(imgs[0].px_height, 100);
        assert_eq!(imgs[1].px_width, 120);
        // 显示尺寸 > 0（来自 cm 矩阵）
        assert!(imgs.iter().all(|i| i.width > 1.0 && i.height > 1.0));
        // 图片在页面内
        assert!(imgs[0].page == 0 && imgs[1].page == 1);
    }

    #[test]
    fn single_image_pdf_only_image() {
        let ex = extract(&fixture("single_image.pdf")).unwrap();
        assert_eq!(ex.pages[0].text_blocks.len(), 0);
        assert_eq!(ex.pages[0].images.len(), 1);
    }

    #[test]
    fn empty_pdf_no_blocks_no_panic() {
        let ex = extract(&fixture("empty.pdf")).unwrap();
        assert_eq!(ex.page_count, 1);
        assert!(ex.pages[0].text_blocks.is_empty());
        assert!(ex.pages[0].images.is_empty());
    }

    #[test]
    fn encrypted_or_corrupt_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("garbage.pdf");
        std::fs::write(&p, b"%PDF-1.4 broken content").unwrap();
        let r = extract(&p);
        assert!(r.is_err());
        assert!(!matches!(r.unwrap_err(), PdfError::Io(_)));
    }

    #[test]
    fn real_paper_extracts_text_and_images() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/b2_Prototypical_Networks.pdf");
        if !p.exists() {
            return; // 真实样本缺失时跳过（不阻断 CI）
        }
        let ex = extract(&p).unwrap();
        assert_eq!(ex.page_count, 13);
        let first_page: String = ex.pages[0]
            .text_blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        assert!(first_page.contains("ototypical"), "标题碎片应可拼回: {first_page:.200}");
        // 图表至少一张
        assert!(
            ex.pages.iter().flat_map(|p| p.images.iter()).count() >= 1,
            "真实论文应含图片"
        );
    }
}
