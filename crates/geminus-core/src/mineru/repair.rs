//! MinerU 输出 U+FFFD 回填修复（v2.8）。
//!
//! 背景：MinerU 云端解析对数学斜体字体（Unicode 数学字母数字符号区，星面字符）
//! 的字形→Unicode 映射有缺陷，正文 text 项中出现 U+FFFD（实测一份 SSM 课件 22 处，
//! 2026-09-21），LLM 会原样保留替换符导致译文乱码。内置解析器经 ToUnicode 提取
//! 同一文件得到完好的星面字符，可作为修复参考源。
//!
//! 算法（真实数据验证 22/22 全修复）——逐 FFFD 连续段局部回填：
//! 1. 窗口：FFFD 两侧各 60 字符 → 剥 `<sub>/<sup>` 等标签 → 截断到最近 FFFD
//!    → 跳过跨窗口边界的 `$…$` 公式段（MinerU 会把行内公式以 LaTeX 混入文本）；
//! 2. 定位：字面窗口（剥离全部空白）在内置参考全文上做字面查找，FFFD 段对应
//!    1..=12 个字母数字字符的捕获；双侧锚窗口 60/40/24/12 梯度退化，再单侧锚 60/24；
//! 3. 校验：所有匹配的捕获完全一致且全为字母才回填
//!    （"唯一一致"防误配，"全字母"防跨标点误捕）；
//! 4. 迭代：最多 4 轮，已回填的邻居字符在后续轮次成为锚点，直到不动点。
//!
//! 不可修复项保留 FFFD 原样；调用方（server）取 [`RepairStats::residual_contexts`]
//! 转储上下文供持续分析。

use crate::model::{Block, Document, InlineContent};

pub const FFFD: char = '\u{FFFD}';

/// FFFD 段两侧各取的窗口字符数
const WINDOW: usize = 60;
/// 变量段（捕获组）最大字符数
const MAX_CAP: usize = 12;
/// 双侧锚窗口梯度
const BOTH_SIDE_STEPS: [usize; 4] = [60, 40, 24, 12];
/// 单侧锚窗口梯度
const ONE_SIDE_STEPS: [usize; 2] = [60, 24];
/// 锚点最小合计字面长度（低于此不定位，防误配）
const MIN_ANCHOR: usize = 8;
/// 最大迭代轮数
const MAX_PASSES: usize = 4;
/// 残余上下文每侧保留的字符数
const CONTEXT: usize = 30;

/// 修复统计（residual_contexts 随 dump 落盘）
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RepairStats {
    /// 成功回填的 FFFD 连续段数
    pub filled_runs: usize,
    /// 实际执行的轮数
    pub passes: usize,
    /// 修复后仍残余的 FFFD 连续段数
    pub residual_runs: usize,
    /// 每处残余的上下文（±30 字符，FFFD 原样保留）
    pub residual_contexts: Vec<String>,
    /// 因多匹配捕获不一致而放弃的段数
    pub ambiguous: usize,
    /// 捕获校验失败（含非字母字符）的段数
    pub bad_capture: usize,
    /// 所有候选窗口都未能定位的段数
    pub unanchored: usize,
}

/// 统计文档段落文本中的 FFFD 字符数
pub fn count_fffd(doc: &Document) -> usize {
    let mut n = 0;
    for block in &doc.blocks {
        if let Block::Paragraph { content, .. } = block {
            for item in content {
                if let InlineContent::Text { text, .. } = item {
                    n += text.chars().filter(|c| *c == FFFD).count();
                }
            }
        }
    }
    n
}

/// 用内置解析参考文档回填 `doc` 中的 FFFD（就地修改 Paragraph 文本）。
/// 参考文档通常是对同一 PDF 跑内置解析（`crate::parse_pdf`）的产物。
pub fn repair_document(doc: &mut Document, reference: &Document) -> RepairStats {
    let mut ref_chars: Vec<char> = Vec::new();
    for t in paragraph_texts(reference) {
        ref_chars.extend(t.chars().filter(|c| !c.is_whitespace()));
    }

    let mut st = RepairStats::default();
    for _ in 0..MAX_PASSES {
        st.passes += 1;
        let mut changed = false;
        for block in doc.blocks.iter_mut() {
            let Block::Paragraph { content, .. } = block else { continue };
            for item in content.iter_mut() {
                let InlineContent::Text { text, .. } = item else { continue };
                if text.contains(FFFD) {
                    changed |= repair_str(text, &ref_chars, &mut st);
                }
            }
        }
        if !changed {
            break;
        }
    }
    st.residual_contexts = collect_residual(doc);
    st.residual_runs = st.residual_contexts.len();
    st
}

fn paragraph_texts(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for block in &doc.blocks {
        if let Block::Paragraph { content, .. } = block {
            let mut t = String::new();
            for item in content {
                if let InlineContent::Text { text, .. } = item {
                    t.push_str(text);
                }
            }
            if !t.is_empty() {
                out.push(t);
            }
        }
    }
    out
}

/// 修复单个 Text run；返回是否发生回填
fn repair_str(text: &mut String, ref_chars: &[char], st: &mut RepairStats) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == FFFD {
            let s = i;
            while i < chars.len() && chars[i] == FFFD {
                i += 1;
            }
            match fill_one(&chars, s, i, ref_chars, st) {
                Some(cap) => {
                    out.extend(cap);
                    st.filled_runs += 1;
                    changed = true;
                }
                None => out.extend(chars[s..i].iter()),
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    if changed {
        *text = out.into_iter().collect();
    }
    changed
}

/// 为一个 FFFD 段 [s, e) 找回填串；失败返回 None（保留原样）
fn fill_one(chars: &[char], s: usize, e: usize, ref_chars: &[char], st: &mut RepairStats) -> Option<Vec<char>> {
    let left_src: String = chars[s.saturating_sub(WINDOW)..s].iter().collect();
    let right_src: String = chars[e..(e + WINDOW).min(chars.len())].iter().collect();
    let left = cut_safe(&strip_tags(&left_src), true);
    let right = cut_safe(&strip_tags(&right_src), false);
    let lws: Vec<char> = left.chars().filter(|c| !c.is_whitespace()).collect();
    let rws: Vec<char> = right.chars().filter(|c| !c.is_whitespace()).collect();

    for n in BOTH_SIDE_STEPS {
        let lw = tail(&lws, n);
        let rw = head(&rws, n);
        if lw.len() + rw.len() < MIN_ANCHOR {
            continue;
        }
        if let Some(cap) = try_anchor(ref_chars, lw, rw, st) {
            return Some(cap);
        }
    }
    for n in ONE_SIDE_STEPS {
        let lw = tail(&lws, n);
        if lw.len() >= MIN_ANCHOR {
            if let Some(cap) = try_anchor(ref_chars, lw, &[], st) {
                return Some(cap);
            }
        }
        let rw = head(&rws, n);
        if rw.len() >= MIN_ANCHOR {
            if let Some(cap) = try_anchor(ref_chars, &[], rw, st) {
                return Some(cap);
            }
        }
    }
    st.unanchored += 1;
    None
}

/// 在参考全文上以 (左字面, 右字面) 定位并提取捕获；所有匹配一致才返回
fn try_anchor(ref_c: &[char], lw: &[char], rw: &[char], st: &mut RepairStats) -> Option<Vec<char>> {
    let mut first: Option<Vec<char>> = None;
    for cap in candidate_captures(ref_c, lw, rw) {
        match &first {
            None => first = Some(cap),
            Some(prev) if *prev == cap => {}
            Some(_) => {
                st.ambiguous += 1;
                return None;
            }
        }
    }
    match first {
        Some(cap) if cap.iter().all(|c| c.is_alphabetic()) => Some(cap),
        Some(_) => {
            st.bad_capture += 1;
            None
        }
        None => None,
    }
}

/// 枚举模式在全参考文本上的捕获（等价于 非贪婪 字母数字{1,12} + 右字面 的 finditer）
fn candidate_captures(ref_c: &[char], lw: &[char], rw: &[char]) -> Vec<Vec<char>> {
    if lw.is_empty() && rw.is_empty() {
        return Vec::new();
    }
    let mut caps = Vec::new();
    if lw.is_empty() {
        // 仅右锚：对每个右字面出现位置，取其前 1..=12 个字母数字字符（最小 k 优先）
        for ro in find_all(ref_c, rw) {
            if let Some(k) = (1..=MAX_CAP).find(|k| {
                ro >= *k && ref_c[ro - k..ro].iter().all(|c| *c != FFFD && c.is_alphanumeric())
            }) {
                caps.push(ref_c[ro - k..ro].to_vec());
            }
        }
    } else {
        for p in find_all(ref_c, lw) {
            let ce = p + lw.len();
            let cap = if rw.is_empty() {
                // 仅左锚：非贪婪最小捕获 = 1 个字符
                ref_c.get(ce).filter(|c| **c != FFFD && c.is_alphanumeric()).map(|c| vec![*c])
            } else {
                (1..=MAX_CAP).find_map(|k| {
                    let cs = ce + k;
                    let ce2 = cs + rw.len();
                    (ce2 <= ref_c.len()
                        && ref_c[cs..ce2] == *rw
                        && ref_c[ce..cs].iter().all(|c| *c != FFFD && c.is_alphanumeric()))
                    .then(|| ref_c[ce..ce + k].to_vec())
                })
            };
            if let Some(cap) = cap {
                caps.push(cap);
            }
        }
    }
    caps
}

/// 子序列全部出现位置（首字符预筛）
fn find_all(hay: &[char], needle: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return out;
    }
    let first = needle[0];
    for i in 0..=(hay.len() - needle.len()) {
        if hay[i] == first && hay[i..i + needle.len()] == *needle {
            out.push(i);
        }
    }
    out
}

/// 剥内联标签（`<sub>`、`<sup>`、`</b>` 等）；`<` 后非字母/斜杠视为字面量
fn strip_tags(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '<' && i + 1 < cs.len() && (cs[i + 1].is_ascii_alphabetic() || cs[i + 1] == '/') {
            let mut j = i + 1;
            while j < cs.len() && cs[j] != '>' {
                j += 1;
            }
            if j < cs.len() {
                i = j + 1;
                continue;
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

/// 窗口截断：left 取最后一个 FFFD 之后、right 取下一个 FFFD 之前；
/// 两端悬空的 `$` 视为公式段跨越窗口边界，丢弃该侧越界部分
fn cut_safe(s: &str, left: bool) -> String {
    let s = if left { s.rsplit(FFFD).next().unwrap_or(s) } else { s.split(FFFD).next().unwrap_or(s) };
    if s.matches('$').count() % 2 == 1 {
        if left { s.rsplit('$').next().unwrap_or(s).to_string() } else { s.split('$').next().unwrap_or(s).to_string() }
    } else {
        s.to_string()
    }
}

fn tail(v: &[char], n: usize) -> &[char] {
    &v[v.len().saturating_sub(n)..]
}

fn head(v: &[char], n: usize) -> &[char] {
    &v[..n.min(v.len())]
}

/// 残余 FFFD 的上下文快照（±CONTEXT 字符）
fn collect_residual(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for block in &doc.blocks {
        let Block::Paragraph { content, .. } = block else { continue };
        for item in content {
            let InlineContent::Text { text, .. } = item else { continue };
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == FFFD {
                    let s = i;
                    while i < chars.len() && chars[i] == FFFD {
                        i += 1;
                    }
                    let lo = s.saturating_sub(CONTEXT);
                    let hi = (i + CONTEXT).min(chars.len());
                    out.push(chars[lo..hi].iter().collect());
                } else {
                    i += 1;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParagraphStyle;

    fn para_doc(texts: &[&str]) -> Document {
        Document {
            id: "t".into(),
            title: "t".into(),
            blocks: texts
                .iter()
                .map(|t| Block::Paragraph {
                    id: format!("b_{}", t.len()),
                    style: ParagraphStyle::default(),
                    content: vec![InlineContent::Text {
                        text: t.to_string(),
                        bold: false,
                        italic: false,
                        underline: false,
                    }],
                })
                .collect(),
            resources: Default::default(),
            render_meta: Default::default(),
            pdf_meta: None,
        }
    }

    fn text_of(doc: &Document, i: usize) -> String {
        let Block::Paragraph { content, .. } = &doc.blocks[i] else { panic!("非段落") };
        content
            .iter()
            .filter_map(|c| match c {
                InlineContent::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn fills_math_italic_from_reference() {
        let mut doc = para_doc(&["matrices can all be represented by \u{FFFD} numbers."]);
        let reference = para_doc(&[
            "In this case, the matrices can all be represented by 𝑁 numbers. To operate over an input.",
        ]);
        let st = repair_document(&mut doc, &reference);
        assert_eq!(text_of(&doc, 0), "matrices can all be represented by 𝑁 numbers.");
        assert_eq!(st.filled_runs, 1);
        assert_eq!(st.residual_runs, 0);
    }

    #[test]
    fn multipass_fills_neighbor_anchored_gaps() {
        // 中间的 FFFD 两侧紧邻其他 FFFD，第 1 轮不可锚定，第 2 轮借已回填邻居补上
        let mut doc = para_doc(&["words (Δ, \u{FFFD}, \u{FFFD}, \u{FFFD}) end of line"]);
        let reference = para_doc(&["words (Δ, 𝑨, 𝑩, 𝑪) end of line and some more text here"]);
        let st = repair_document(&mut doc, &reference);
        assert_eq!(text_of(&doc, 0), "words (Δ, 𝑨, 𝑩, 𝑪) end of line");
        assert_eq!(st.filled_runs, 3);
        assert!(st.passes >= 2, "应需多轮: {}", st.passes);
    }

    #[test]
    fn ambiguous_match_left_unfilled() {
        let mut doc = para_doc(&["key \u{FFFD} lock"]);
        let reference = para_doc(&["key 𝑨 lock and key 𝑩 lock again here"]);
        let st = repair_document(&mut doc, &reference);
        assert_eq!(count_fffd(&doc), 1);
        assert_eq!(st.residual_runs, 1);
        assert!(st.residual_contexts[0].contains(FFFD));
    }

    #[test]
    fn capture_must_be_letters_only() {
        // 参考中变量后紧跟数字：捕获 "𝑨3" 含非字母 → 拒绝回填
        let mut doc = para_doc(&["value \u{FFFD}, more text"]);
        let reference = para_doc(&["value 𝑨3, more text follows here now"]);
        let st = repair_document(&mut doc, &reference);
        assert_eq!(count_fffd(&doc), 1);
        assert!(st.bad_capture >= 1);
    }

    #[test]
    fn balanced_formula_in_window_falls_back_to_shorter_anchor() {
        // MinerU 文本含行内 LaTeX；完整窗口对不上，短窗口绕开公式段
        let mut doc = para_doc(&["choose $f(x)$ and then value \u{FFFD} matrix. The most popular"]);
        let reference = para_doc(&[
            "choose f(x) and then value 𝑨 matrix. The most popular form of structure is diagonal",
        ]);
        let st = repair_document(&mut doc, &reference);
        assert!(text_of(&doc, 0).contains("value 𝑨 matrix"));
        assert_eq!(st.residual_runs, 0);
    }

    #[test]
    fn tags_stripped_from_window() {
        let mut doc = para_doc(&["re<sub>p</sub>resented by \u{FFFD} numbers right now"]);
        let reference = para_doc(&["represented by 𝑁 numbers right now in the paper"]);
        let st = repair_document(&mut doc, &reference);
        assert!(text_of(&doc, 0).contains("by 𝑁 numbers"));
        assert_eq!(st.filled_runs, 1);
    }

    #[test]
    fn dangling_dollar_span_is_cut_from_window() {
        // 左窗口尾部是未闭合的 $ 公式段：截掉后以右字面锚定
        let mut doc = para_doc(&["choose $f(x) + \u{FFFD} ok tail of sentence indeed very long"]);
        let reference = para_doc(&["choose 𝑨 ok tail of sentence indeed very long text"]);
        let st = repair_document(&mut doc, &reference);
        assert!(text_of(&doc, 0).contains("$f(x) + 𝑨 ok tail"));
        assert_eq!(st.filled_runs, 1);
    }

    #[test]
    fn clean_document_is_noop() {
        let mut doc = para_doc(&["plain text without replacement chars"]);
        assert_eq!(count_fffd(&doc), 0);
        let reference = para_doc(&["whatever"]);
        let st = repair_document(&mut doc, &reference);
        assert_eq!(st.filled_runs, 0);
        assert_eq!(st.passes, 1);
    }

    #[test]
    fn residual_contexts_reported() {
        let mut doc = para_doc(&["prefix \u{FFFD} suffix and quite a lot of surrounding text for context"]);
        let reference = para_doc(&["nothing matching here at all in this reference document"]);
        let st = repair_document(&mut doc, &reference);
        assert_eq!(st.residual_runs, 1);
        assert!(st.residual_contexts[0].starts_with("prefix "));
        assert!(st.residual_contexts[0].contains(FFFD));
    }
}
