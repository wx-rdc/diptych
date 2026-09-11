//! PoC：pdf crate 文本块（含坐标/字体）与图片提取验证（v1_plan.md T1）。
//!
//! 用法：
//!   cargo run -p geminus-core --example pdf_poc -- <file.pdf> [--images]
#![allow(missing_docs)]

use std::collections::HashMap;
use std::env;

use pdf::content::{Matrix, Op, Point};
use pdf::enc::StreamFilter;
use pdf::file::FileOptions;
use pdf::font::Font;
use pdf::object::*;
use pdf::primitive::PdfString;

/// 字体信息缓存：名字 → (is_cid, 解码表, 宽度表)
struct FontInfo {
    is_cid: bool,
    /// code → unicode（ToUnicode 优先；Base-14 回退 WinAnsi）
    map: HashMap<u16, String>,
    /// code → 字形宽度（千分之一单位）
    widths: HashMap<u16, f32>,
}

fn load_font_info(font: &Font, resolve: &impl Resolve) -> FontInfo {
    let mut map = HashMap::new();
    if let Some(Ok(touni)) = font.to_unicode(resolve) {
        for (code, s) in touni.iter() {
            map.insert(code, s.to_string());
        }
    }
    let mut widths = HashMap::new();
    if let Ok(Some(w)) = font.widths(resolve) {
        for cid in 0..65536u32 {
            let wv = w.get(cid as usize);
            if wv != 0.0 {
                widths.insert(cid as u16, wv);
            }
            if widths.len() > 512 {
                break; // 简单字体远少于这个数
            }
        }
    }
    FontInfo { is_cid: font.is_cid(), map, widths }
}

/// 字符串宽度（文本空间单位）：按码点查宽度表，缺省 500/1000
fn string_width(s: &PdfString, info: &FontInfo) -> f32 {
    let bytes = s.as_bytes();
    let codes: Vec<u16> = if info.is_cid {
        bytes
            .chunks(2)
            .map(|p| if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) } else { p[0] as u16 })
            .collect()
    } else {
        bytes.iter().map(|b| *b as u16).collect()
    };
    codes.iter().map(|c| info.widths.get(c).copied().unwrap_or(500.0)).sum::<f32>() / 1000.0
}

/// 解码 PdfString：CID 字体 2 字节/码，简单字体 1 字节/码；无 ToUnicode 时回退 latin1。
fn decode_string(s: &PdfString, info: &FontInfo) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    if info.is_cid {
        for pair in bytes.chunks(2) {
            let code = if pair.len() == 2 { u16::from_be_bytes([pair[0], pair[1]]) } else { pair[0] as u16 };
            out.push_str(info.map.get(&code).map(String::as_str).unwrap_or("\u{FFFD}"));
        }
    } else {
        for &b in bytes {
            out.push_str(info.map.get(&(b as u16)).map(String::as_str).unwrap_or(&latin1(b)));
        }
    }
    out
}

fn latin1(b: u8) -> String {
    (b as char).to_string()
}

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
    mul(&Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }, m)
}

fn main() -> Result<(), pdf::error::PdfError> {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("用法: pdf_poc <file.pdf> [--images]");
    let want_images = args.iter().any(|a| a == "--images");
    let save_images = args.iter().any(|a| a == "--save-images");

    let now = std::time::Instant::now();
    let file = FileOptions::cached().open(path)?;
    let resolve = file.resolver();
    println!("# 文件: {path}  页数: {}", file.num_pages());

    for (pno, page) in file.pages().enumerate() {
        let page = page?;
        let media = page.media_box()?;
        println!("## Page {pno}  media_box: {:?}", media);
        let resources = page.resources()?;
        let mut fonts: HashMap<String, FontInfo> = HashMap::new();
        for (name, lazy) in &resources.fonts {
            let f = lazy.load(&resolve)?;
            let info = load_font_info(&f, &resolve);
            println!("   font {name} cid={} touni_entries={}", info.is_cid, info.map.len());
            fonts.insert(name.as_str().to_string(), info);
        }

        if want_images {
            for (name, r) in &resources.xobjects {
                let o = resolve.get(*r)?;
                if let XObject::Image(im) = &*o {
                    let (data, filter) = im.raw_image_data(&resolve)?;
                    let mime = match filter {
                        Some(StreamFilter::DCTDecode(_)) => "image/jpeg",
                        Some(StreamFilter::FlateDecode(_)) => "raw/flate",
                        _ => "other",
                    };
                    println!("   image {name}: {}x{} {} {} bytes", im.width, im.height, mime, data.len());
                    if save_images && mime == "image/jpeg" {
                        let out = format!("/tmp/pdf_poc_p{pno}_{}.jpg", name.as_str().trim_start_matches('/'));
                        std::fs::write(&out, &data).unwrap();
                        println!("     saved → {out}");
                    }
                }
            }
        }

        let Some(content) = &page.contents else { continue };
        let ops = content.operations(&resolve)?;

        // 文本状态机（含 q/Q 图形状态栈：CTM+文本参数随栈保存恢复）
        let mut ctm = Matrix::default();
        let mut tm = Matrix::default();
        let mut tlm = Matrix::default();
        let mut leading = 0.0f32;
        let mut cur_font: Option<(String, f32)> = None;
        let mut horiz = 1.0f32;
        let mut default_font_info = FontInfo { is_cid: false, map: HashMap::new(), widths: HashMap::new() };
        type GsSnapshot = (Matrix, Option<(String, f32)>, f32);
        let mut gs_stack: Vec<GsSnapshot> = Vec::new();

        for op in &ops {
            match op {
                Op::Save => gs_stack.push((ctm, cur_font.clone(), leading)),
                Op::Restore => {
                    if let Some((c, f, l)) = gs_stack.pop() {
                        ctm = c;
                        cur_font = f;
                        leading = l;
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
                    let w = draw(&ctm, &tm, &cur_font, text, &fonts, &mut default_font_info, pno);
                    let fs = cur_font.as_ref().map(|f| f.1).unwrap_or(0.0);
                    tm = translate(&tm, w * fs * horiz, 0.0);
                }
                Op::TextDrawAdjusted { array } => {
                    let fs = cur_font.as_ref().map(|f| f.1).unwrap_or(0.0);
                    for item in array {
                        match item {
                            pdf::content::TextDrawAdjusted::Text(text) => {
                                let w = draw(&ctm, &tm, &cur_font, text, &fonts, &mut default_font_info, pno);
                                tm = translate(&tm, w * fs * horiz, 0.0);
                            }
                            pdf::content::TextDrawAdjusted::Spacing(s) => {
                                tm = translate(&tm, -*s / 1000.0 * fs * horiz, 0.0);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    println!("# 耗时: {:?}", now.elapsed());
    Ok(())
}

fn draw(
    ctm: &Matrix,
    tm: &Matrix,
    cur_font: &Option<(String, f32)>,
    text: &PdfString,
    fonts: &HashMap<String, FontInfo>,
    default_info: &mut FontInfo,
    pno: usize,
) -> f32 {
    let (fname, fsize) = cur_font.clone().unwrap_or(("?".into(), 0.0));
    let info = fonts.get(&fname).unwrap_or(default_info);
    let trm = mul(tm, ctm);
    let size = fsize * trm.d.abs().max(1e-3);
    let s = decode_string(text, info);
    if s.trim().is_empty() {
        return 0.0;
    }
    println!("   [{pno}] ({:6.1},{:6.1}) size={:5.1} font={:?} {:?} {:?}", trm.e, trm.f, size, fname, s, if info.is_cid { "cid" } else { "simple" });
    string_width(text, info)
}
