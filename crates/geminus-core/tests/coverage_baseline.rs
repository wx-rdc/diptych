//! v2.2 覆盖度门禁（T1 红灯基线 → T3 题注转绿 → T4 算法转绿，全绿终态）。
//!
//! 重放 b2 夹具（v2 数据源适配）→ 覆盖度审计 → violations 空 + 字符/面积门禁达标。
//! 历史：T1 交付时 violations=[text, figure_caption, algorithm, table_caption]、char=0.72
//! （v1 适配路径的红灯基线，证明审计能抓到 A1/A2 丢块）；T3/T4 逐项转绿。

use geminus_core::audit::coverage_audit;
use geminus_core::mineru;

fn b2_fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/mineru/b2_prototypical")
}

#[test]
fn b2_coverage_baseline_flags_missing_mappings() {
    let fixture = b2_fixture();
    let raw = mineru::raw::load_raw_items(&fixture).unwrap();
    let ir = mineru::adapter::adapt_dir(&fixture, 0, "b2").unwrap();
    let report = coverage_audit(&ir, &raw);
    eprintln!(
        "b2 coverage: char={:.4} area={:.4} violations={:?}",
        report.char_coverage,
        report.area_coverage,
        report.violations.iter().map(|v| v.kind.as_str()).collect::<Vec<_>>()
    );
    // T4 后终态：全绿（A1 题注 + A2 算法 + chart/list 全部映射）
    assert!(
        report.violations.is_empty(),
        "violations 应为空，实际 {:?}",
        report.violations
    );
    assert!(report.char_coverage >= 0.98, "char={}", report.char_coverage);
    assert!(report.area_coverage >= 0.95, "area={}", report.area_coverage);
}

#[test]
fn v1_path_still_exposes_char_loss() {
    // 对照组：v1 数据源（无 v2 文件）对同数据不可用——用 v1 夹具验证审计仍能暴露
    // Unmapped 丢弃（构造含未知类型的临时目录）
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("content_list.json"),
        r#"[{"type":"text","text":"hello","page_idx":0},{"type":"handwriting","text":"lost","page_idx":0}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("layout.json"),
        r#"{"pdf_info":[{"page_size":[612,792],"page_idx":0}]}"#,
    )
    .unwrap();
    let raw = mineru::raw::load_raw_items(dir.path()).unwrap();
    let ir = mineru::adapter::adapt_dir(dir.path(), 0, "x").unwrap();
    let report = coverage_audit(&ir, &raw);
    let kinds: Vec<&str> = report.violations.iter().map(|v| v.kind.as_str()).collect();
    assert!(kinds.contains(&"handwriting"), "{kinds:?}");
    assert!(report.char_coverage < 0.98);
}
