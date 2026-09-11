//! 多栏检测与阅读顺序（v1_plan.md T5，算法验证 T2.3）。
//!
//! 栏检测两段式（覆盖直方图 → 起点聚类兜底，兼顾宽沟 reportlab 与窄沟 TeX）；
//! 阅读顺序：双栏时跨栏元素作分节锚点，节内先左栏后右栏、栏内自上而下；
//! 单栏退化为纯 y 降序。

use super::{ImageObject, PdfParagraph, TextLine};

/// 栏的 x 范围（页面坐标）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnRange {
    pub x_start: f64,
    pub x_end: f64,
}

impl ColumnRange {
    pub fn contains_x(&self, x: f64) -> bool {
        x >= self.x_start && x <= self.x_end
    }
}

/// 阅读顺序中的页面元素（段落下标 / 图片下标）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageElement {
    Paragraph(usize),
    Image(usize),
}

/// 版心 x 范围
pub fn text_area(lines: &[TextLine]) -> (f64, f64) {
    let x0 = lines.iter().map(|l| l.x_start).fold(f64::INFINITY, f64::min);
    let x1 = lines.iter().map(|l| l.x_end).fold(f64::NEG_INFINITY, f64::max);
    if !x0.is_finite() || !x1.is_finite() || x1 <= x0 {
        return (0.0, 0.0);
    }
    (x0, x1)
}

fn is_wide(x_start: f64, x_end: f64, area: (f64, f64)) -> bool {
    (x_end - x_start) > (area.1 - area.0) * 0.70
}

/// 栏检测：返回 1 或 2 个栏（MVP 只支持双栏，见 v1_plan §七）
pub fn detect_columns(lines: &[TextLine]) -> Vec<ColumnRange> {
    let area = text_area(lines);
    if area == (0.0, 0.0) || area.1 <= area.0 {
        return vec![ColumnRange { x_start: 0.0, x_end: 0.0 }];
    }
    let (a0, a1) = area;
    // 跨栏行（宽度>70%版心）不参与正文栏形分析；单栏文档正文也普遍"宽"，
    // body 不足时直接判单栏
    let body: Vec<&TextLine> = lines.iter().filter(|l| !is_wide(l.x_start, l.x_end, area)).collect();
    if body.len() < 8 {
        return vec![ColumnRange { x_start: a0, x_end: a1 }];
    }

    // ── 策略一：覆盖直方图零带（宽沟，如 reportlab 夹具 24pt 沟）──
    let n_bins = 50usize;
    let bin_w = (a1 - a0) / n_bins as f64;
    let mut hist = vec![0usize; n_bins];
    for l in &body {
        let s = (((l.x_start - a0) / bin_w).floor() as usize).min(n_bins - 1);
        let e = (((l.x_end - a0) / bin_w).floor() as usize).min(n_bins - 1);
        for h in hist.iter_mut().take(e + 1).skip(s) {
            *h += 1;
        }
    }
    let lo = n_bins / 7;                 // 版心两侧各 ~14% 不算沟
    let hi = n_bins - n_bins / 7;
    let (mut best_len, mut best_start, mut cur_len, mut cur_start) = (0usize, 0usize, 0usize, 0usize);
    let mut in_run = false;
    for (i, h) in hist.iter().enumerate().take(hi).skip(lo) {
        if *h == 0 {
            if !in_run {
                cur_start = i;
                in_run = true;
            }
            cur_len += 1;
            if cur_len > best_len {
                best_len = cur_len;
                best_start = cur_start;
            }
        } else {
            cur_len = 0;
            in_run = false;
        }
    }
    if best_len >= 3 && best_len as f64 * bin_w >= (a1 - a0) * 0.05 {
        let gap_mid = a0 + (best_start as f64 + best_len as f64 / 2.0) * bin_w;
        if let Some(cols) = validated_split(&body, a0, a1, gap_mid) {
            return cols;
        }
    }

    // ── 策略二：起点聚类 + 不重叠验证（窄沟，如 NeurIPS 12pt 沟）──
    if let Some(cols) = split_by_start_clusters(&body, a0, a1) {
        return cols;
    }

    vec![ColumnRange { x_start: a0, x_end: a1 }]
}

/// 验证以 gap_mid 分栏后两侧各有足够行且互不越界
fn validated_split(body: &[&TextLine], a0: f64, a1: f64, gap_mid: f64) -> Option<Vec<ColumnRange>> {
    let left = body.iter().filter(|l| (l.x_start + l.x_end) / 2.0 < gap_mid).count();
    let right = body.len() - left;
    if left >= 4 && right >= 4 {
        Some(vec![
            ColumnRange { x_start: a0, x_end: gap_mid },
            ColumnRange { x_start: gap_mid, x_end: a1 },
        ])
    } else {
        None
    }
}

/// x_start 聚类（容差 40pt 合并首行缩进变体），要求两大簇且列区不重叠
fn split_by_start_clusters(body: &[&TextLine], a0: f64, a1: f64) -> Option<Vec<ColumnRange>> {
    let mut starts: Vec<f64> = body.iter().map(|l| l.x_start).collect();
    starts.sort_by(|x, y| x.partial_cmp(y).unwrap());
    const GAP: f64 = 40.0;
    let mut clusters: Vec<(f64, f64, usize)> = Vec::new(); // (min, max, count)
    for s in starts {
        match clusters.last_mut() {
            Some((_, max, count)) if s - *max <= GAP => {
                *max = s;
                *count += 1;
            }
            _ => clusters.push((s, s, 1)),
        }
    }
    if clusters.len() != 2 {
        return None;
    }
    let (c0, c1) = (clusters[0], clusters[1]);
    // 两大簇（各 ≥25% 行）且分界 ≥ 版心 5%
    let n = body.len();
    if c0.2 * 4 < n || c1.2 * 4 < n || c1.0 - c0.1 < (a1 - a0) * 0.05 {
        return None;
    }
    // 不重叠验证：左簇行不越过右簇行起点
    let left_max_end = body
        .iter()
        .filter(|l| l.x_start <= c0.1)
        .map(|l| l.x_end)
        .fold(f64::NEG_INFINITY, f64::max);
    let right_min_start = body
        .iter()
        .filter(|l| l.x_start >= c1.0)
        .map(|l| l.x_start)
        .fold(f64::INFINITY, f64::min);
    if left_max_end >= right_min_start - 2.0 {
        return None; // 左"簇"行延伸进右区 → 只是缩进/引用，不是双栏
    }
    let gap_mid = (left_max_end + right_min_start) / 2.0;
    validated_split(body, a0, a1, gap_mid)
}

/// 栏归属：元素中心落在哪个栏
pub fn column_of_x(center: f64, columns: &[ColumnRange]) -> usize {
    columns
        .iter()
        .position(|c| c.contains_x(center))
        .unwrap_or(if center < columns.first().map(|c| c.x_start).unwrap_or(0.0) { 0 } else { columns.len() - 1 })
}

/// 阅读顺序键：跨栏元素为分节锚点（组 2k+1），普通内容按锚点分组（组 2k）
///
/// positions: (y_top, 栏下标, 是否跨栏)。单栏（two_column=false）时跨栏一律视为普通。
/// 节内先左栏后右栏，栏内自上而下。
pub fn reading_order(positions: &[(f64, usize, bool)], two_column: bool) -> Vec<usize> {
    let mut anchors: Vec<f64> = if two_column {
        let mut a: Vec<f64> = positions
            .iter()
            .filter(|(_, _, sp)| *sp)
            .map(|(y, _, _)| *y)
            .collect();
        a.sort_by(|x, y| y.partial_cmp(x).unwrap());
        a
    } else {
        Vec::new()
    };
    anchors.dedup_by(|a, b| (*a - *b).abs() < 1.0);

    // 组号：普通元素 = 2×(严格在其上方的锚点数)；锚点自身 = 2×序 + 1。
    // 例：锚点上方内容组 0 → 锚点组 1 → 锚点下方内容组 2 ……
    let group_of = |y: f64, is_span: bool| -> usize {
        if is_span {
            let idx = anchors.iter().position(|&ay| (ay - y).abs() < 1.0);
            2 * idx.unwrap_or(0) + 1
        } else {
            2 * anchors.iter().filter(|&&ay| ay > y + 1.0).count()
        }
    };

    let mut idx: Vec<usize> = (0..positions.len()).collect();
    idx.sort_by(|&a, &b| {
        let (ya, ca, spa) = positions[a];
        let (yb, cb, spb) = positions[b];
        group_of(ya, spa && two_column)
            .cmp(&group_of(yb, spb && two_column))
            .then(ca.cmp(&cb))
            .then(yb.partial_cmp(&ya).unwrap()) // y 大者在前
            .then(a.cmp(&b))                    // 稳定
    });
    idx
}

/// 页级编排：段落（含栏/跨栏信息）与图片按阅读顺序交错输出
pub fn order_page(
    paragraphs: &[PdfParagraph],
    images: &[ImageObject],
    columns: &[ColumnRange],
) -> Vec<PageElement> {
    let two_column = columns.len() >= 2;
    let mut positions: Vec<(f64, usize, bool)> = Vec::with_capacity(paragraphs.len() + images.len());
    for p in paragraphs {
        let spanning = two_column && p.spanning;
        positions.push((p.y_top, p.column.unwrap_or(0), spanning));
    }
    for im in images {
        let center = im.x + im.width / 2.0;
        let col = column_of_x(center, columns);
        let spanning = two_column && {
            let (a0, a1) = (
                columns.first().map(|c| c.x_start).unwrap_or(0.0),
                columns.last().map(|c| c.x_end).unwrap_or(0.0),
            );
            let gutter_mid =
                (columns[0].x_end + columns.get(1).map(|c| c.x_start).unwrap_or(columns[0].x_end)) / 2.0;
            (im.x..im.x + im.width).contains(&gutter_mid) || im.width > (a1 - a0) * 0.7
        };
        positions.push((im.y + im.height, col, spanning));
    }
    reading_order(&positions, two_column)
        .into_iter()
        .map(|i| {
            if i < paragraphs.len() {
                PageElement::Paragraph(i)
            } else {
                PageElement::Image(i - paragraphs.len())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x_start: f64, x_end: f64, y: f64) -> TextLine {
        TextLine {
            blocks: vec![],
            page: 0,
            baseline: y,
            y_top: y,
            y_bottom: y - 10.0,
            merged_text: format!("l{x_start:.0}-{y:.0}"),
            x_start,
            x_end,
            font_size: 10.0,
            bold: false,
        }
    }

    #[test]
    fn single_column_no_gutter() {
        let lines: Vec<TextLine> = (0..10)
            .map(|i| line(54.0, 540.0 - (i as f64 % 3.0) * 30.0, 700.0 - i as f64 * 15.0))
            .collect();
        let cols = detect_columns(&lines);
        assert_eq!(cols.len(), 1);
    }

    #[test]
    fn two_column_wide_gutter_detected() {
        let mut lines = Vec::new();
        for i in 0..12 {
            lines.push(line(54.0, 290.0, 700.0 - i as f64 * 15.0));
            lines.push(line(318.0, 554.0, 700.0 - i as f64 * 15.0));
        }
        let cols = detect_columns(&lines);
        assert_eq!(cols.len(), 2, "应检出双栏: {cols:?}");
        assert!(cols[0].x_end > 290.0 && cols[0].x_end < 318.0);
        assert_eq!(cols[1].x_start, cols[0].x_end);
    }

    #[test]
    fn two_column_narrow_gutter_by_start_clusters() {
        // NeurIPS 风格：栏 [108,300] 与 [312,504]，沟仅 12pt（<3 bin）
        let mut lines = Vec::new();
        for i in 0..15 {
            lines.push(line(108.0, 300.0 - i as f64, 700.0 - i as f64 * 12.0));
            lines.push(line(312.0, 504.0 - i as f64 * 0.5, 700.0 - i as f64 * 12.0));
        }
        let cols = detect_columns(&lines);
        assert_eq!(cols.len(), 2, "窄沟应经起点聚类检出: {cols:?}");
        assert!(cols[0].x_end >= 300.0 && cols[0].x_end <= 312.0);
    }

    #[test]
    fn indented_blockquote_not_mistaken_as_columns() {
        // 单栏 + 深缩进引用块：左簇行延伸到版心右缘 → 拒绝分栏
        let mut lines = Vec::new();
        for i in 0..10 {
            lines.push(line(54.0, 554.0, 700.0 - i as f64 * 15.0));
        }
        for i in 0..6 {
            lines.push(line(114.0, 554.0, 450.0 - i as f64 * 15.0));
        }
        let cols = detect_columns(&lines);
        assert_eq!(cols.len(), 1, "缩进引用不应误判双栏: {cols:?}");
    }

    #[test]
    fn spanning_title_does_not_break_detection() {
        let mut lines = vec![line(54.0, 558.0, 740.0)];
        for i in 0..12 {
            lines.push(line(54.0, 290.0, 690.0 - i as f64 * 15.0));
            lines.push(line(318.0, 554.0, 690.0 - i as f64 * 15.0));
        }
        let cols = detect_columns(&lines);
        assert_eq!(cols.len(), 2, "跨栏标题不应阻碍双栏检测: {cols:?}");
    }

    #[test]
    fn reading_order_two_columns_left_first() {
        // 左栏两行（y 700/685），右栏两行（y 700/685）
        let positions = vec![
            (700.0_f64, 0_usize, false),
            (685.0, 0, false),
            (700.0, 1, false),
            (685.0, 1, false),
        ];
        let order = reading_order(&positions, true);
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn reading_order_spanning_title_comes_first() {
        let positions = vec![
            (690.0_f64, 0_usize, false), // 左栏
            (690.0, 1, false),           // 右栏
            (740.0, 0, true),            // 跨栏标题
            (650.0, 0, false),           // 左栏下一行
        ];
        let order = reading_order(&positions, true);
        // 栏优先：标题(2) → 左栏全部（690(0)、650(3)）→ 右栏 690(1)
        assert_eq!(order, vec![2, 0, 3, 1]);
    }

    #[test]
    fn reading_order_second_spanning_starts_new_section() {
        // 跨栏节标题 y=400 将下半页分节
        let positions = vec![
            (690.0_f64, 0_usize, false),
            (690.0, 1, false),
            (400.0, 0, true), // 跨栏节标题
            (380.0, 0, false),
            (380.0, 1, false),
        ];
        let order = reading_order(&positions, true);
        assert_eq!(order, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn reading_order_single_column_is_y_desc() {
        let positions = vec![
            (100.0_f64, 0_usize, true), // 单栏下 spanning 无效
            (300.0, 0, false),
            (200.0, 0, false),
        ];
        let order = reading_order(&positions, false);
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn column_of_x_clamps() {
        let cols = vec![
            ColumnRange { x_start: 54.0, x_end: 306.0 },
            ColumnRange { x_start: 306.0, x_end: 558.0 },
        ];
        assert_eq!(column_of_x(175.0, &cols), 0);
        assert_eq!(column_of_x(436.0, &cols), 1);
        assert_eq!(column_of_x(10.0, &cols), 0);
        assert_eq!(column_of_x(600.0, &cols), 1);
    }
}
