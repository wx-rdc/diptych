//! 解析器集成测试（任务 T3）：真实文档 + 生成夹具

use base64::Engine as _;
use geminus_core::{parse_docx, Alignment, Block, InlineContent, ResourceKind};

fn repo_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// 真实测试文档：97 段、1 张 VML 图片（水平线）、无表格/公式
/// （私有文档未随社区版分发——缺失时跳过而非失败）
#[test]
fn parses_real_document() {
    let path = repo_path("test/Response-to-Reviewers-Access-2026-38888.docx");
    if !path.exists() {
        eprintln!("跳过：私有文档 {} 不在本仓库", path.display());
        return;
    }
    let doc = parse_docx(&path).expect("真实文档应可解析");
    assert!(
        doc.blocks.len() >= 90,
        "段落块应 ≥90，实际 {}",
        doc.blocks.len()
    );

    // 文本抽查
    let all_text: String = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        InlineContent::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert!(all_text.contains("SSM-Match"), "应包含标题文本");

    // VML 图片：存在 Image 块，资源为 PNG
    let images: Vec<&Block> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Image { .. }))
        .collect();
    assert!(!images.is_empty(), "应存在图片块");
    if let Block::Image { resource_id, alt_text, .. } = images[0] {
        let res = doc.resources.get(resource_id).expect("图片资源应在资源池");
        assert_eq!(res.kind, ResourceKind::Image);
        assert_eq!(res.mime, "image/png");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&res.data_b64)
            .expect("base64 应可解码");
        assert_eq!(&bytes[..4], b"\x89PNG", "应为 PNG 魔数");
        assert_eq!(alt_text.as_deref(), Some("Default Line"));
    }

    // 真实文档大量 jc=both → 应有 Justify 段落
    let justify_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph { style, .. } if style.alignment == Alignment::Justify))
        .count();
    // 真实文档 20 处 jc=both，其中约半数在空段落（Empty 不带样式）上
    assert!(justify_count >= 5, "jc=both 段落应 ≥5，实际 {justify_count}");
}

#[test]
fn parses_simple_fixture() {
    let doc = parse_docx(&repo_path("test/fixtures/simple.docx")).unwrap();

    let headings: Vec<Option<u8>> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph { style, .. } => Some(style.heading_level),
            _ => None,
        })
        .collect();
    assert!(headings.contains(&Some(1)), "应有 H1：{headings:?}");
    assert!(headings.contains(&Some(2)), "应有 H2：{headings:?}");

    let list_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph { style, .. } if style.is_list_item))
        .count();
    assert!(list_count >= 4, "列表项应 ≥4，实际 {list_count}");

    assert!(
        doc.blocks.iter().any(|b| matches!(b, Block::Empty { .. })),
        "应有空段落块"
    );

    assert!(
        doc.blocks.iter().any(|b| matches!(b, Block::Paragraph { style, .. } if style.alignment == Alignment::Center)),
        "应有居中段落"
    );

    // 粗/斜/下划线 run
    let has_bold = doc.blocks.iter().any(|b| match b {
        Block::Paragraph { content, .. } => content.iter().any(|c| matches!(c, InlineContent::Text { bold: true, .. })),
        _ => false,
    });
    assert!(has_bold, "应有粗体 run");
    let mixed = doc.blocks.iter().any(|b| match b {
        Block::Paragraph { content, .. } => {
            content.iter().any(|c| matches!(c, InlineContent::Text { italic: true, .. }))
                && content.iter().any(|c| matches!(c, InlineContent::Text { underline: true, .. }))
        }
        _ => false,
    });
    assert!(mixed, "应有斜体与下划线 run");

    // 无图片/表格/公式资源
    assert!(doc.resources.is_empty());
    assert_eq!(doc.title, "Chapter One: Introduction");
}

#[test]
fn parses_rich_fixture() {
    let doc = parse_docx(&repo_path("test/fixtures/rich.docx")).unwrap();

    // 块级图片（独立段落，python-docx add_picture）
    let block_images = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Image { .. }))
        .count();
    assert!(block_images >= 1, "块级图片应 ≥1，实际 {block_images}");

    // 行内图片：出现在某段落 content 中
    let inline_images = doc.blocks.iter().filter_map(|b| match b {
        Block::Paragraph { content, .. } => Some(
            content
                .iter()
                .filter(|c| matches!(c, InlineContent::InlineImage { .. }))
                .count(),
        ),
        _ => None,
    }).sum::<usize>();
    assert!(inline_images >= 1, "行内图片应 ≥1，实际 {inline_images}");

    // 表格块 + 预览
    let tables: Vec<&Block> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Table { .. }))
        .collect();
    assert_eq!(tables.len(), 1);
    if let Block::Table { preview_text, resource_id, .. } = tables[0] {
        assert!(preview_text.as_deref().unwrap_or("").contains("Name"));
        assert!(doc.resources.get(resource_id).unwrap().raw_xml.starts_with("<w:tbl"));
    }

    // 公式：独立段（display）+ 行内（分数），资源含 latex
    let formulas_as_blocks = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Formula { .. }))
        .count();
    let formulas_inline = doc.blocks.iter().filter_map(|b| match b {
        Block::Paragraph { content, .. } => Some(
            content
                .iter()
                .filter(|c| matches!(c, InlineContent::InlineFormula { .. }))
                .count(),
        ),
        _ => None,
    }).sum::<usize>();
    assert!(formulas_as_blocks >= 1, "独立公式段应 ≥1，实际 {formulas_as_blocks}");
    assert!(formulas_inline >= 1, "行内公式应 ≥1，实际 {formulas_inline}");

    let formula_resources: Vec<&geminus_core::Resource> = doc
        .resources
        .values()
        .filter(|r| r.kind == ResourceKind::Formula)
        .collect();
    assert!(formula_resources.len() >= 2);
    let latex_joined: String = formula_resources
        .iter()
        .filter_map(|r| r.latex.clone())
        .collect::<Vec<_>>()
        .join(" ; ");
    assert!(
        latex_joined.contains("E=mc") && latex_joined.contains("^{2}"),
        "E=mc² 应转出：{latex_joined}"
    );
    assert!(
        latex_joined.contains("\\frac{a+b}{c+d}"),
        "分数应转出：{latex_joined}"
    );

    // 图片资源 ≥2（块级 + 行内）
    let image_resources = doc
        .resources
        .values()
        .filter(|r| r.kind == ResourceKind::Image)
        .count();
    assert!(image_resources >= 2, "图片资源应 ≥2，实际 {image_resources}");
}

#[test]
fn missing_file_is_error() {
    let err = parse_docx(std::path::Path::new("/nonexistent/x.docx"));
    assert!(err.is_err());
}

#[test]
fn not_a_docx_is_error() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), b"not a zip").unwrap();
    assert!(parse_docx(tmp.path()).is_err());
}
