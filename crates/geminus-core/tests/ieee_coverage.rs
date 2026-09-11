//! v2.3 B3 门禁：IEEE Access 夹具（公网实测文档冻结，feedback_0910）。
//! 重放 MinerU raw → 适配 → 审计：violations 空 + 参考文献条目全部映射为列表段落。

use geminus_core::audit::coverage_audit;
use geminus_core::mineru;
use geminus_core::model::{Block, InlineContent};

fn ieee_fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/mineru/ieee_access")
}

#[test]
fn ieee_references_map_and_coverage_gates_pass() {
    let fixture = ieee_fixture();
    // 与生产同款重放链路：分块缓存根目录 → 合并 IR + 平移 raw
    let ir = mineru::reparse_from_dir(&fixture, "ieee").unwrap();
    let raw = mineru::raw::load_raw_items_from_root(&fixture).unwrap();

    // 参考文献条目（"[1] Author..." 形态）必须映射为 is_list_item 段落
    let refs = ir
        .blocks
        .iter()
        .filter(|b| {
            matches!(b, Block::Paragraph { style, content, .. }
                if style.is_list_item
                    && content.iter().any(|c| matches!(c, InlineContent::Text { text, .. }
                        if text.trim_start().starts_with("[1]"))))
        })
        .count();
    assert!(refs >= 1, "应存在 [1] 起头的列表段落（参考文献），实际 {refs}");

    let report = coverage_audit(&ir, &raw);
    eprintln!(
        "ieee coverage: char={:.4} area={:.4} violations={:?}",
        report.char_coverage,
        report.area_coverage,
        report.violations
            .iter()
            .map(|v| v.kind.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        report.violations.is_empty(),
        "violations 应为空: {:?}",
        report.violations
    );
    assert!(report.char_coverage >= 0.98, "char={}", report.char_coverage);
    assert!(report.area_coverage >= 0.95, "area={}", report.area_coverage);
}
