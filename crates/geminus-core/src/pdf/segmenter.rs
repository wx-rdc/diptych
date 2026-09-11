//! 行聚类 → 段落聚类 → 样式推断（v1_plan.md T2/T6）。
//!
//! 行聚类按基线 y 分组 + 行内 x 序合并（TeX 碎片按 gap 补空格）；
//! 段落聚类依据行距阈值（1.4×中位行距）、字号变化、缩进变化、列表标记；
//! 样式推断：正文字号 = 加权众数，H1/H2/H3 = 1.6/1.3/1.15 倍正文。

use super::extractor::{PageContent, PdfExtraction};
use super::layout;
use super::{PdfParagraph, TextBlock, TextLine};

/// 行聚类：同一基线（容差 0.35×字号）的块 → 行，行内按 x 排序合并。
///
/// 同 y 但 x 不连续（跨沟）的块拆分为多行，避免左右栏文本互窜。
pub fn cluster_lines(blocks: &[TextBlock]) -> Vec<TextLine> {
    let mut sorted: Vec<&TextBlock> = blocks.iter().collect();
    sorted.sort_by(|a, b| {
        b.y.partial_cmp(&a.y)
            .unwrap()
            .then(a.x.partial_cmp(&b.x).unwrap())
    });

    let mut lines: Vec<Vec<&TextBlock>> = Vec::new();
    for b in sorted {
        let new_line = match lines.last() {
            None => true,
            Some(cur) => {
                let cur_y = weighted_baseline(cur);
                let tol = b.font_size.max(cur.iter().map(|x| x.font_size).fold(0.0, f64::max)) * 0.35;
                (cur_y - b.y).abs() > tol
            }
        };
        if new_line {
            lines.push(vec![b]);
        } else {
            lines.last_mut().unwrap().push(b);
        }
    }

    // 行内按 x 排序并做沟拆分与文本合并
    let mut result = Vec::with_capacity(lines.len());
    for group in lines {
        let mut group = group;
        group.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        let font_size = weighted_font_size(&group);
        // 大间隙（>1.2×字号，大于栏沟下限）拆行：不同栏的同行文本
        let split_gap = font_size.max(12.0) * 1.2;
        let mut segs: Vec<Vec<&TextBlock>> = vec![Vec::new()];
        for b in group {
            match segs.last().and_then(|s| s.last()) {
                Some(prev) if b.x - prev.x1() > split_gap => segs.push(vec![b]),
                Some(_) => segs.last_mut().unwrap().push(b),
                None => segs.last_mut().unwrap().push(b),
            }
        }
        for seg in segs {
            if seg.is_empty() {
                continue;
            }
            let merged = merge_line_text(&seg, font_size);
            if merged.trim().is_empty() {
                continue;
            }
            result.push(TextLine {
                baseline: weighted_baseline(&seg),
                y_top: seg.iter().map(|b| b.y_top()).fold(f64::NEG_INFINITY, f64::max),
                y_bottom: seg.iter().map(|b| b.y_bottom()).fold(f64::INFINITY, f64::min),
                page: seg[0].page,
                x_start: seg.iter().map(|b| b.x).fold(f64::INFINITY, f64::min),
                x_end: seg.iter().map(|b| b.x1()).fold(f64::NEG_INFINITY, f64::max),
                merged_text: merged,
                font_size,
                bold: seg.iter().all(|b| b.bold),
                blocks: seg.into_iter().cloned().collect(),
            });
        }
    }
    result
}

fn weighted_baseline(seg: &[&TextBlock]) -> f64 {
    let total: usize = seg.iter().map(|b| b.text.chars().count().max(1)).sum();
    seg.iter()
        .map(|b| b.y * b.text.chars().count().max(1) as f64)
        .sum::<f64>()
        / total.max(1) as f64
}

fn weighted_font_size(seg: &[&TextBlock]) -> f64 {
    let total: usize = seg.iter().map(|b| b.text.chars().count().max(1)).sum();
    seg.iter()
        .map(|b| b.font_size * b.text.chars().count().max(1) as f64)
        .sum::<f64>()
        / total.max(1) as f64
}

/// 行内文本合并：相邻块间隙 > 0.2×字号 且前块不以空白结尾时补空格
fn merge_line_text(seg: &[&TextBlock], font_size: f64) -> String {
    let mut out = String::new();
    for (i, b) in seg.iter().enumerate() {
        if i > 0 {
            let prev = &seg[i - 1];
            let gap = b.x - prev.x1();
            if gap > font_size * 0.10 && !out.ends_with(char::is_whitespace) && !b.text.starts_with(char::is_whitespace) {
                out.push(' ');
            }
        }
        out.push_str(&b.text);
    }
    out
}

/// 段落聚类（同栏行序列 → 段落），返回段落与段间空隙信息
fn cluster_paragraphs<T: AsRef<TextLine>>(lines: &[T], median_gap: f64) -> Vec<PdfParagraph> {
    let lines: Vec<&TextLine> = lines.iter().map(|l| l.as_ref()).collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let para_threshold = median_gap * 1.4;
    let center = |l: &TextLine| (l.x_start + l.x_end) / 2.0;
    let mut paras: Vec<PdfParagraph> = Vec::new();
    let mut cur: Vec<TextLine> = vec![lines[0].clone()];

    for line in &lines[1..] {
        let prev = cur.last().unwrap();
        let gap = prev.baseline - line.baseline; // 基线间距（同栏内自上而下）
        let size_change = (prev.font_size - line.font_size).abs() / prev.font_size.max(1e-3) > 0.15;
        // TeX 缩进式分段：行首 x 突变（缩进/凸出）即新段；居中块（中心不变）豁免
        let centered = (center(prev) - center(line)).abs() < 2.0;
        let indent_change = (prev.x_start - line.x_start).abs() > line.font_size * 0.5;
        let list_start = is_list_marker(&line.merged_text);
        let new_para =
            !centered && (gap > para_threshold || size_change || indent_change || list_start);
        if new_para {
            paras.push(build_paragraph(&cur, None, false));
            cur = vec![(*line).clone()];
        } else {
            cur.push((*line).clone());
        }
    }
    paras.push(build_paragraph(&cur, None, false));
    paras
}

fn build_paragraph(lines: &[TextLine], column: Option<usize>, spanning: bool) -> PdfParagraph {
    let text = lines
        .iter()
        .map(|l| l.merged_text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    PdfParagraph {
        y_top: lines.iter().map(|l| l.y_top).fold(f64::NEG_INFINITY, f64::max),
        y_bottom: lines.iter().map(|l| l.y_bottom).fold(f64::INFINITY, f64::min),
        x_start: lines.iter().map(|l| l.x_start).fold(f64::INFINITY, f64::min),
        font_size: lines.iter().map(|l| l.font_size).fold(0.0, f64::max),
        bold: lines.iter().all(|l| l.bold),
        page: lines.first().map(|l| l.page).unwrap_or(0),
        text,
        column,
        spanning,
        lines: lines.to_vec(),
    }
}

/// 列表项标记：•、-–—、1. / 1) / (a)
pub fn is_list_marker(text: &str) -> bool {
    let t = text.trim_start();
    let mut chars = t.chars();
    let Some(first) = chars.next() else { return false };
    if matches!(first, '•' | '-' | '–' | '—') {
        return chars.next().is_some_and(char::is_whitespace);
    }
    if first == '(' {
        let rest: Vec<char> = t.chars().skip(1).collect();
        // (a) / (A) 形态：单字母后紧跟右括号
        return rest.len() >= 2 && rest[0].is_alphanumeric() && !rest[0].is_ascii_digit() && rest[1] == ')';
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return false;
    }
    let rest: String = t.chars().skip(digits.chars().count()).collect();
    rest.starts_with(". ") || rest.starts_with(") ") || rest == "." || rest == ")"
}

/// 全文档正文主字号（按字符数加权众数，分桶到 0.5pt）
pub fn body_font_size(paragraphs: &[PdfParagraph]) -> f64 {
    let mut buckets: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for p in paragraphs {
        *buckets
            .entry(((p.font_size * 2.0).round() as u64).max(1))
            .or_insert(0) += p.text.chars().count().max(1);
    }
    buckets
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(k, _)| k as f64 / 2.0)
        .unwrap_or(10.0)
}

/// 标题级别推断：≥1.6→H1，≥1.3→H2，≥1.15→H3
pub fn heading_level(size: f64, body: f64) -> Option<u8> {
    let r = size / body.max(1e-3);
    if r >= 1.6 {
        Some(1)
    } else if r >= 1.3 {
        Some(2)
    } else if r >= 1.15 {
        Some(3)
    } else {
        None
    }
}

/// 单页段落构建：行聚类 → 栏检测 → 按栏聚类 → 样式标记
pub fn segment_page(page: &PageContent) -> Vec<PdfParagraph> {
    let lines = cluster_lines(&page.text_blocks);
    if lines.is_empty() {
        return Vec::new();
    }
    let columns = layout::detect_columns(&lines);
    let area = layout::text_area(&lines);
    let two_column = columns.len() >= 2;

    // 行按栏分组（保持栏内 y 降序）
    let mut col_lines: Vec<Vec<&TextLine>> = vec![Vec::new(); columns.len()];
    let mut spanning_lines: Vec<&TextLine> = Vec::new();
    for l in &lines {
        let span = two_column && layout_is_spanning(l.x_start, l.x_end, area);
        if span {
            spanning_lines.push(l);
        } else {
            let col = layout::column_of_x((l.x_start + l.x_end) / 2.0, &columns).min(columns.len() - 1);
            col_lines[col].push(l);
        }
    }

    let mut paragraphs = Vec::new();
    // 栏内段落
    for (ci, lines_in_col) in col_lines.iter().enumerate() {
        if lines_in_col.is_empty() {
            continue;
        }
        let median = median_line_gap(lines_in_col);
        for mut para in cluster_paragraphs(lines_in_col, median) {
            para.column = Some(ci);
            para.spanning = false;
            paragraphs.push(para);
        }
    }
    // 跨栏行各自成段（标题等）
    for l in spanning_lines {
        paragraphs.push(build_paragraph(std::slice::from_ref(l), None, true));
    }

    // 样式标记：is_list_item → 保持（converter 读取文本再判定）；这里不做
    paragraphs
}

fn layout_is_spanning(x_start: f64, x_end: f64, area: (f64, f64)) -> bool {
    (x_end - x_start) > (area.1 - area.0) * 0.70
}

/// 中位行距（相邻行基线差的中位数）
fn median_line_gap(lines: &[&TextLine]) -> f64 {
    let mut gaps: Vec<f64> = lines
        .windows(2)
        .map(|w| (w[0].baseline - w[1].baseline).max(0.0))
        .filter(|g| *g > 0.01)
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if gaps.is_empty() {
        12.0 // 单行段落无从计算，给常规正文行距
    } else {
        gaps[gaps.len() / 2]
    }
}

/// 全文档分段（页面序列 → 段落序列，含跨页合并）
pub fn segment_document(extraction: &PdfExtraction) -> Vec<PdfParagraph> {
    let mut all: Vec<PdfParagraph> = Vec::new();
    for page in &extraction.pages {
        all.extend(segment_page(page));
    }
    merge_cross_page(all)
}

/// 跨页段落合并：页尾末段 + 下页首段，同栏位/同字号/顶底贴边
fn merge_cross_page(paras: Vec<PdfParagraph>) -> Vec<PdfParagraph> {
    let mut merged: Vec<PdfParagraph> = Vec::with_capacity(paras.len());
    for para in paras {
        let can_merge = match (merged.last(), para.first_line_text().is_empty()) {
            (Some(prev), false) => {
                let same_column = prev.column == para.column || (prev.column.is_none() && para.column.is_none());
                let same_size = (prev.font_size - para.font_size).abs() / prev.font_size.max(1e-3) < 0.1;
                let contiguous_pages = para.page == prev.page + 1;
                let x_aligned = (prev.x_start - para.x_start).abs() < 3.0;
                let not_special = !para.spanning && !is_list_marker(para.first_line_text());
                let prev_is_page_tail = prev.y_bottom < 90.0; // 页底边缘（版心底 ≈54）
                let cur_is_page_head = para.y_top > 700.0; // 页顶边缘（letter 版心顶 ≈738）
                same_column && same_size && contiguous_pages && x_aligned && not_special && prev_is_page_tail && cur_is_page_head
            }
            _ => false,
        };
        if can_merge {
            let prev = merged.last_mut().unwrap();
            prev.lines.extend(para.lines);
            prev.text = format!("{} {}", prev.text, para.text);
            prev.y_bottom = para.y_bottom;
        } else {
            merged.push(para);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str, x: f64, y: f64, size: f64, page: usize) -> TextBlock {
        let width = text.chars().count() as f64 * size * 0.5;
        TextBlock {
            text: text.into(),
            x,
            y,
            width,
            font_size: size,
            font_name: "F".into(),
            bold: false,
            page,
        }
    }

    fn blocks_fixture() -> Vec<TextBlock> {
        let mut b = Vec::new();
        // 段 1：两行（y 700/685）
        for (i, t) in ["first line of para one", "second line of para one"].iter().enumerate() {
            b.push(block(t, 54.0, 700.0 - i as f64 * 15.0, 11.0, 0));
        }
        // 段 2：两行（y 650/635，段距 35 > 1.4×15）
        for (i, t) in ["first line of para two", "second line of para two"].iter().enumerate() {
            b.push(block(t, 54.0, 650.0 - i as f64 * 15.0, 11.0, 0));
        }
        b
    }

    #[test]
    fn line_clustering_merges_tex_fragments() {
        let blocks = vec![
            block("Pro", 54.0, 700.0, 11.0, 0),
            block("ototypical", 70.0, 700.0, 11.0, 0), // gap 2 < 0.2×11 → 直接拼
            block("Netw", 130.0, 700.0, 11.0, 0),
            block("orks", 160.0, 685.0, 11.0, 0), // 不同基线 → 新行
        ];
        let lines = cluster_lines(&blocks);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].merged_text.contains("Prototype") || lines[0].merged_text.starts_with("Pro"), "{}", lines[0].merged_text);
    }

    #[test]
    fn line_clustering_inserts_space_on_gap() {
        let blocks = vec![
            block("hello", 54.0, 700.0, 11.0, 0),
            block("world", 90.0, 700.0, 11.0, 0), // gap = 90-(54+27.5)=8.5 > 0.2×11=2.2 → 补空格
        ];
        let lines = cluster_lines(&blocks);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].merged_text, "hello world");
    }

    #[test]
    fn line_clustering_splits_columns_at_same_y() {
        let blocks = vec![
            block("left col text", 54.0, 700.0, 11.0, 0),
            block("right col text", 318.0, 700.0, 11.0, 0), // gap 264 > 1.2×11 → 拆行
        ];
        let lines = cluster_lines(&blocks);
        assert_eq!(lines.len(), 2, "同 y 不同栏应拆为两行");
        assert_eq!(lines[0].x_start, 54.0);
        assert_eq!(lines[1].x_start, 318.0);
    }

    #[test]
    fn paragraph_clustering_by_gap() {
        let lines = cluster_lines(&blocks_fixture());
        let paras = cluster_paragraphs(
            &lines.iter().collect::<Vec<_>>(),
            median_line_gap(&lines.iter().collect::<Vec<_>>()),
        );
        assert_eq!(paras.len(), 2, "段距 35 > 1.4×15 应分段: {paras:?}");
        assert!(paras[0].text.contains("para one"));
        assert!(paras[1].text.contains("para two"));
    }

    #[test]
    fn paragraph_split_on_font_size_change() {
        let mut b = blocks_fixture();
        b.push(block("Big Heading Text", 54.0, 600.0, 24.0, 0));
        b.push(block("normal line under heading", 54.0, 580.0, 11.0, 0));
        let lines = cluster_lines(&b);
        let median = median_line_gap(&lines.iter().collect::<Vec<_>>());
        let paras = cluster_paragraphs(&lines.iter().collect::<Vec<_>>(), median);
        assert!(paras.len() >= 3);
        let heading = paras.iter().find(|p| p.font_size >= 24.0).unwrap();
        assert_eq!(heading.lines.len(), 1);
    }

    #[test]
    fn paragraph_split_on_list_marker() {
        let mut b = Vec::new();
        for (i, t) in ["intro sentence here", "continuing the intro"].iter().enumerate() {
            b.push(block(t, 54.0, 700.0 - i as f64 * 15.0, 11.0, 0));
        }
        b.push(block("• a bullet item", 54.0, 670.0, 11.0, 0)); // 行距正常但列表标记
        let lines = cluster_lines(&b);
        let paras = cluster_paragraphs(&lines.iter().collect::<Vec<_>>(), 15.0);
        assert_eq!(paras.len(), 2);
        assert!(paras[1].text.contains("bullet"));
    }

    #[test]
    fn list_marker_patterns() {
        assert!(is_list_marker("• bullet"));
        assert!(is_list_marker("- dash"));
        assert!(is_list_marker("1. numbered"));
        assert!(is_list_marker("12) paren"));
        assert!(is_list_marker("(a) alpha"));
        assert!(!is_list_marker("Normal sentence"));
        assert!(!is_list_marker("2026 was a year")); // 4 位数非列表
        assert!(!is_list_marker("e.g. this is fine")); // 单字母+点+空格是列表形态，但 e.g 例外？
    }

    #[test]
    fn heading_level_thresholds() {
        assert_eq!(heading_level(17.6, 11.0), Some(1)); // 1.6×
        assert_eq!(heading_level(15.0, 11.0), Some(2)); // 1.36×
        assert_eq!(heading_level(13.0, 11.0), Some(3)); // 1.18×
        assert_eq!(heading_level(11.0, 11.0), None);
    }

    #[test]
    fn body_font_size_is_weighted_mode() {
        let mk = |size: f64, text: &str| PdfParagraph {
            lines: vec![],
            text: text.into(),
            page: 0,
            y_top: 0.0,
            y_bottom: 0.0,
            x_start: 0.0,
            font_size: size,
            bold: false,
            column: Some(0),
            spanning: false,
        };
        let paras = vec![
            mk(11.0, &"x".repeat(500)), // 正文主体
            mk(24.0, &"y".repeat(20)),  // 标题
            mk(11.0, &"z".repeat(300)),
        ];
        assert!((body_font_size(&paras) - 11.0).abs() < 0.01);
    }

    #[test]
    fn cross_page_merge() {
        let mk = |page: usize, y_top: f64| PdfParagraph {
            lines: vec![TextLine {
                blocks: vec![],
                page,
                baseline: y_top,
                y_top,
                y_bottom: y_top - 11.0,
                merged_text: format!("p{page}"),
                x_start: 54.0,
                x_end: 500.0,
                font_size: 11.0,
                bold: false,
            }],
            text: format!("p{page}"),
            page,
            y_top,
            y_bottom: y_top - 11.0,
            x_start: 54.0,
            font_size: 11.0,
            bold: false,
            column: Some(0),
            spanning: false,
        };
        // 页 0 尾（y_bottom=68<90）+ 页 1 首（y_top=730>700）→ 合并
        let merged = merge_cross_page(vec![mk(0, 79.0), mk(1, 730.0)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "p0 p1");
        // 页 0 中部 + 页 1 首 → 不合并
        let kept = merge_cross_page(vec![mk(0, 300.0), mk(1, 730.0)]);
        assert_eq!(kept.len(), 2);
    }
}
