//! DOCX 解析器（plan.md §4.1）

pub mod document;
pub mod rels;
pub mod styles;
pub mod xml_tree;

pub use document::{parse_docx, parse_document_content, DocxError};
