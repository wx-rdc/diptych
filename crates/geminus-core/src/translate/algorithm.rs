//! 算法块翻译（v2.2 T4.3）：仅译 Stmt/Comment 行的文本段；
//! Label 行（Input:/Output:/for/end for 等结构标签）与行内公式原样保留。
//! 行级 [L n] 编号一次请求，拆分回填；任一行失败 → 整块回退原文（fallback）。

use crate::model::Block;
use crate::translate::PlaceholderRef;

/// 一条可翻译行（source = 该行 Text 段拼接，公式段已 {{Pn}} 令牌化）
pub struct AlgoTransLine {
    pub line_idx: usize,
    pub source: String,
    pub refs: Vec<PlaceholderRef>,
}

/// 收集可翻译行（Stmt/Comment；跳过 Label 行与空文本行）
pub fn extract_translatable_lines(block: &Block) -> Vec<AlgoTransLine> {
    let Block::Algorithm { lines, .. } = block else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !matches!(
            line.kind,
            crate::model::AlgoLineKind::Stmt | crate::model::AlgoLineKind::Comment
        ) {
            continue;
        }
        let (source, refs) = crate::translate::prompt::extract_source_text(&line.segments);
        if source.trim().is_empty() {
            continue;
        }
        out.push(AlgoTransLine { line_idx: i, source, refs });
    }
    out
}

/// 翻译回填：行数不匹配或任一行令牌损坏 → false（调用方回退整块）。
/// Label 行与未翻译行原样保留。
pub fn rebuild_algorithm_lines(
    block: &mut Block,
    extracted: &[AlgoTransLine],
    translated: &[String],
) -> bool {
    if extracted.len() != translated.len() {
        return false;
    }
    let Block::Algorithm { lines, .. } = block else {
        return false;
    };
    for (line, text) in extracted.iter().zip(translated) {
        if text.trim().is_empty() {
            return false;
        }
        let tmp = Block::Paragraph {
            id: "tmp".into(),
            style: Default::default(),
            content: std::mem::take(&mut lines[line.line_idx].segments),
        };
        let (rebuilt, degraded) =
            crate::translate::rebuild::rebuild_paragraph(&tmp, text, &line.refs);
        if degraded {
            // 还原已被 take 的原段（避免半改状态）
            if let Block::Paragraph { content, .. } = rebuilt {
                lines[line.line_idx].segments = content;
            }
            return false;
        }
        if let Block::Paragraph { content, .. } = rebuilt {
            lines[line.line_idx].segments = content;
        }
    }
    true
}

/// 算法翻译 prompt（行级 [L n] 编号 + 结构约束）
pub fn build_algorithm_prompt(lines: &[&str], source_lang: &str, target_lang: &str) -> String {
    let lang_names: &[(&str, &str)] = &[
        ("en", "英语"),
        ("zh", "中文"),
        ("ja", "日语"),
        ("fr", "法语"),
        ("de", "德语"),
    ];
    let name = |code: &str| {
        lang_names
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| code.to_string())
    };
    let mut p = format!(
        "你是一个专业文档翻译器。以下是算法伪代码的编号行，请从{}翻译为{}。\n\n要求：\n\
         1. 保持行编号 [L1] [L2] … 不变，每个编号对应一行的译文\n\
         2. 只翻译自然语言文本；数学表达式、变量名、结构关键字（for/do/end/if/return/Input/Output）必须原样保留\n\
         3. 不要合并、拆分或调换行\n\
         4. 双花括号占位标记（如 {{{{P0}}}}）必须原样保留，不要翻译或移动\n\n",
        name(source_lang),
        name(target_lang),
    );
    for (i, l) in lines.iter().enumerate() {
        p.push_str(&format!("[L{}] {}\n", i + 1, l));
    }
    p
}

/// 按 [L n] 拆分译文（复用 chunk 的最紧凑覆盖窗口算法，前缀参数化为 L）
pub fn split_l_numbered(response: &str, line_count: usize) -> Option<Vec<String>> {
    crate::translate::chunk::split_numbered_response_with_prefix(response, line_count, Some(b'L'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AlgoLine, AlgoLineKind, InlineContent};

    fn t(s: &str) -> InlineContent {
        InlineContent::Text { text: s.into(), bold: false, italic: false, underline: false }
    }

    fn algo() -> Block {
        Block::Algorithm {
            id: "b_0".into(),
            header: String::new(),
            lines: vec![
                AlgoLine { kind: AlgoLineKind::Label, segments: vec![t("Output: The loss")] },
                AlgoLine { kind: AlgoLineKind::Stmt, segments: vec![t("Select class indices")] },
                AlgoLine { kind: AlgoLineKind::Comment, segments: vec![t("Initialize loss")] },
            ],
        }
    }

    #[test]
    fn extract_translatable_lines_skips_labels() {
        let lines = extract_translatable_lines(&algo());
        assert_eq!(lines.len(), 2); // Stmt + Comment，Label 不译
        assert_eq!(lines[0].source, "Select class indices");
        assert_eq!(lines[0].line_idx, 1);
    }

    #[test]
    fn rebuild_lines_preserves_labels_and_math() {
        let mut block = algo();
        let extracted = extract_translatable_lines(&block);
        let translated = vec!["选择类别索引".to_string(), "初始化损失".to_string()];
        assert!(rebuild_algorithm_lines(&mut block, &extracted, &translated));
        let Block::Algorithm { lines, .. } = &block else { panic!() };
        assert!(matches!(&lines[0].segments[0],
            InlineContent::Text { text, .. } if text == "Output: The loss"));
        assert!(matches!(&lines[1].segments[0],
            InlineContent::Text { text, .. } if text == "选择类别索引"));
        assert!(matches!(&lines[2].segments[0],
            InlineContent::Text { text, .. } if text == "初始化损失"));
    }

    #[test]
    fn line_count_mismatch_rejected() {
        let mut block = algo();
        let extracted = extract_translatable_lines(&block);
        assert!(!rebuild_algorithm_lines(&mut block, &extracted, &["仅一行".to_string()]));
        // 失败时原段不被破坏
        let Block::Algorithm { lines, .. } = &block else { panic!() };
        assert!(matches!(&lines[1].segments[0],
            InlineContent::Text { text, .. } if text == "Select class indices"));
    }

    #[test]
    fn token_damage_rejected_and_restored() {
        // 行内公式令牌被模型吞掉 → 整块拒绝
        let mut block = Block::Algorithm {
            id: "b".into(),
            header: String::new(),
            lines: vec![AlgoLine {
                kind: AlgoLineKind::Stmt,
                segments: vec![
                    t("pick "),
                    InlineContent::InlineFormula { resource_id: "r_eq".into() },
                ],
            }],
        };
        let extracted = extract_translatable_lines(&block);
        assert_eq!(extracted[0].source, "pick {{P0}}");
        assert!(!rebuild_algorithm_lines(&mut block, &extracted, &["选取".to_string()]));
    }

    #[test]
    fn prompt_numbered_and_constrained() {
        let p = build_algorithm_prompt(&["Output: loss", "for k do"], "en", "zh");
        assert!(p.contains("[L1] Output: loss"), "{p}");
        assert!(p.contains("[L2] for k do"), "{p}");
        assert!(p.contains("结构关键字"), "{p}");
        assert!(p.contains("{{P0}}"), "{p}");
    }

    #[test]
    fn split_l_numbered_basic_and_failure() {
        let parts = split_l_numbered("[L1] 甲\n[L2] 乙", 2).unwrap();
        assert_eq!(parts, vec!["甲", "乙"]);
        // 普通数字编号不匹配 [L n] 形态 → None（防与段落 chunk 串台）
        assert!(split_l_numbered("[1] 甲 [2] 乙", 2).is_none());
        assert!(split_l_numbered("无编号", 2).is_none());
    }
}
