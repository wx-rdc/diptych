//! PDF 解析器（docs/v1_plan.md §2）——与 DOCX 解析器输出同构的 `Document` IR。
//!
//! 管线：`extractor`（页面遍历/文本块/图片）→ `layout`（多栏检测/阅读顺序）
//! → `segmenter`（行聚类/段落聚类/样式推断）→ `converter`（→ IR）。
//! 选型与验证见 docs/pdf-lib-eval.md。

pub mod converter;
pub mod extractor;
pub mod layout;
pub mod png;
pub mod segmenter;

use std::path::Path;

use thiserror::Error;

pub use converter::convert_to_document;
pub use extractor::{extract, PageContent, PdfExtraction};

/// PDF 解析错误
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF 解析失败: {0}")]
    Parse(#[from] pdf::error::PdfError),
    #[error("加密 PDF 暂不支持")]
    Encrypted,
    #[error("无效 PDF: {0}")]
    Invalid(String),
}

/// PDF 提取的原始文本块（带坐标；PDF 坐标系原点在左下角，y 向上）
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub text: String,
    /// 基线起点 x
    pub x: f64,
    /// 基线 y
    pub y: f64,
    /// 估算宽度（字形宽度累乘，页面单位）
    pub width: f64,
    pub font_size: f64,
    pub font_name: String,
    pub bold: bool,
    pub page: usize,
}

impl TextBlock {
    /// 行内末尾 x
    pub fn x1(&self) -> f64 {
        self.x + self.width
    }

    /// 近似行顶（基线 + 0.8×字号）
    pub fn y_top(&self) -> f64 {
        self.y + self.font_size * 0.8
    }

    /// 近似行底（基线 - 0.2×字号）
    pub fn y_bottom(&self) -> f64 {
        self.y - self.font_size * 0.2
    }
}

/// PDF 提取的图片对象
#[derive(Debug, Clone, PartialEq)]
pub struct ImageObject {
    pub data: Vec<u8>,
    pub mime: String,
    /// 页面坐标（左下角原点）
    pub x: f64,
    pub y: f64,
    /// 页面显示尺寸（点）
    pub width: f64,
    pub height: f64,
    /// 像素尺寸（IR Block::Image 用）
    pub px_width: u32,
    pub px_height: u32,
    pub page: usize,
}

/// 聚类后的行（同栏同页）
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub blocks: Vec<TextBlock>,
    pub page: usize,
    /// 基线 y（聚类主依据）
    pub baseline: f64,
    pub y_top: f64,
    pub y_bottom: f64,
    pub merged_text: String,
    /// 行首 x（阅读方向）
    pub x_start: f64,
    /// 行末 x
    pub x_end: f64,
    /// 主字号（按字符数加权）
    pub font_size: f64,
    pub bold: bool,
}

/// 聚类后的段落
#[derive(Debug, Clone, PartialEq)]
pub struct PdfParagraph {
    pub lines: Vec<TextLine>,
    pub text: String,
    pub page: usize,
    /// 段落顶部 y（页面坐标）
    pub y_top: f64,
    pub y_bottom: f64,
    pub x_start: f64,
    pub font_size: f64,
    pub bold: bool,
    /// 栏号（0 起；跨栏 = None）
    pub column: Option<usize>,
    /// 是否跨栏（标题等）
    pub spanning: bool,
}

impl PdfParagraph {
    /// 首行文本（列表/样式检测用）
    pub fn first_line_text(&self) -> &str {
        self.lines
            .first()
            .map(|l| l.merged_text.as_str())
            .unwrap_or("")
    }
}

/// 解析 PDF 文件 → Document IR（服务端入口，与 parse_docx 对偶）
pub fn parse_pdf(path: &Path) -> Result<geminus_core_model::Document, PdfError> {
    let extraction = extract(path)?;
    let paragraphs = segmenter::segment_document(&extraction);
    Ok(convert_to_document(
        &extraction,
        &paragraphs,
        &fallback_title(path),
    ))
}

/// 快速统计 PDF 页数（只加载交叉引用表，不解析内容）。
/// 匿名限流预检用：在调用 MinerU 云端 API 前判断是否会超出页数配额。
pub fn count_pages(path: &Path) -> Result<usize, PdfError> {
    let doc = lopdf::Document::load(path).map_err(|e| PdfError::Invalid(e.to_string()))?;
    Ok(doc.get_pages().len())
}

fn fallback_title(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// IR 类型别名（避免循环 use crate::model 语义混乱）
mod geminus_core_model {
    pub use crate::model::Document;
}

impl AsRef<TextLine> for TextLine {
    fn as_ref(&self) -> &TextLine {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_block_geometry() {
        let b = TextBlock {
            text: "hi".into(),
            x: 100.0,
            y: 200.0,
            width: 50.0,
            font_size: 10.0,
            font_name: "F1".into(),
            bold: false,
            page: 0,
        };
        assert_eq!(b.x1(), 150.0);
        assert_eq!(b.y_top(), 208.0);
        assert_eq!(b.y_bottom(), 198.0);
    }

    #[test]
    fn parse_missing_file_returns_io_error() {
        let err = parse_pdf(Path::new("/nonexistent/x.pdf")).unwrap_err();
        assert!(matches!(err, PdfError::Io(_)), "实际: {err:?}");
    }

    #[test]
    fn parse_garbage_file_returns_error_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.pdf");
        std::fs::write(&p, b"not a pdf at all").unwrap();
        let err = parse_pdf(&p).unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
