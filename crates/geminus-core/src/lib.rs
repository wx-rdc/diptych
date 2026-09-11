//! geminus-core：DOCX/PDF 解析 / IR / LLM 翻译 / SQLite 缓存（纯业务核心，无壳依赖）

pub mod audit;
pub mod cache;
pub mod config;
pub mod mineru;
pub mod math;
pub mod model;
pub mod parser;
pub mod pdf;
pub mod translate;

pub use config::ApiConfig;
pub use model::*;
pub use parser::{parse_docx, DocxError};
pub use pdf::{parse_pdf, PdfError};
