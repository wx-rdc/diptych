//! OMML → LaTeX 转换器（plan.md PoC-⑥ 方案 B：Rust 端规则法直接提取）。
//!
//! 覆盖常见结构：分数/上下标/根号/求和积分/定界符/矩阵；未识别节点降级为文本拼接；
//! 整体失败返回 None → 前端显示占位框。

use crate::parser::xml_tree::{parse_xml_to_tree, El, Node};

/// 将 OMML XML 转换为 LaTeX；无法解析或结果为空时返回 None
pub fn omml_to_latex(omml_xml: &str) -> Option<String> {
    let root = parse_xml_to_tree(omml_xml).ok()?;
    let latex = convert_el(&root);
    let trimmed = latex.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn convert_el(el: &El) -> String {
    let children_latex = |e: &El| -> String {
        e.children
            .iter()
            .filter_map(|n| match n {
                Node::El(c) => Some(convert_el(c)),
                Node::Text(_) => None,
            })
            .collect::<String>()
    };

    match el.name.as_str() {
        // 数学 run：拼接 m:t 文本
        "m:r" => run_text(el),
        // 分数
        "m:f" => {
            let num = child_content(el, "m:num");
            let den = child_content(el, "m:den");
            format!("\\frac{{{num}}}{{{den}}}")
        }
        "m:sSup" => {
            let base = child_content(el, "m:e");
            let sup = child_content(el, "m:sup");
            format!("{{{base}}}^{{{sup}}}")
        }
        "m:sSub" => {
            let base = child_content(el, "m:e");
            let sub = child_content(el, "m:sub");
            format!("{{{base}}}_{{{sub}}}")
        }
        "m:sSubSup" => {
            let base = child_content(el, "m:e");
            let sub = child_content(el, "m:sub");
            let sup = child_content(el, "m:sup");
            format!("{{{base}}}_{{{sub}}}^{{{sup}}}")
        }
        "m:rad" => {
            let deg = child_content(el, "m:deg");
            let e = child_content(el, "m:e");
            if deg.is_empty() {
                format!("\\sqrt{{{e}}}")
            } else {
                format!("\\sqrt[{deg}]{{{e}}}")
            }
        }
        "m:d" => {
            let (beg, end) = delimiter_chars(el);
            let body = children_latex(el);
            format!("\\left{beg}{body}\\right{end}")
        }
        "m:nary" => {
            let chr = el
                .child("m:naryPr")
                .and_then(|p| p.child("m:chr"))
                .and_then(|c| c.attr("m:val").or_else(|| c.attr("val")))
                .unwrap_or("∫");
            let op = match chr {
                "∑" => "\\sum",
                "∏" => "\\prod",
                "∮" => "\\oint",
                _ => "\\int",
            };
            let sub = child_content(el, "m:sub");
            let sup = child_content(el, "m:sup");
            let e = child_content(el, "m:e");
            let mut out = String::from(op);
            if !sub.is_empty() {
                out.push_str(&format!("_{{{sub}}}"));
            }
            if !sup.is_empty() {
                out.push_str(&format!("^{{{sup}}}"));
            }
            out.push(' ');
            out.push_str(&e);
            out
        }
        "m:m" => {
            let rows: Vec<String> = el
                .children_named("m:mr")
                .iter()
                .map(|r| {
                    r.children_named("m:e")
                        .iter()
                        .map(|c| children_latex(c))
                        .collect::<Vec<_>>()
                        .join(" & ")
                })
                .collect();
            format!("\\begin{{matrix}}{}\\end{{matrix}}", rows.join(" \\\\ "))
        }
        "m:t" => text_to_latex(&el.text_content()),
        // 容器（m:oMath/m:oMathPara/m:e/m:num/...）与未知节点：递归子元素
        _ => children_latex(el),
    }
}

fn child_content(el: &El, name: &str) -> String {
    el.child(name).map(convert_el).unwrap_or_default()
}

/// m:r 的文本（多个 m:t 拼接）
fn run_text(r: &El) -> String {
    let text: String = r.children_named("m:t").iter().map(|t| t.text_content()).collect();
    text_to_latex(&text)
}

/// 原子文本 → LaTeX：
/// - 含空白/中文 → \text{...}（直立文本）
/// - 否则原样（转义注释与对齐等特殊字符）
fn text_to_latex(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let needs_text_mode = text.chars().any(|c| c.is_whitespace() || !c.is_ascii());
    let escaped: String = text
        .chars()
        .map(|c| match c {
            '%' | '&' | '#' | '$' | '_' | '{' | '}' => format!("\\{c}"),
            _ => c.to_string(),
        })
        .collect();
    if needs_text_mode {
        format!("\\text{{{escaped}}}")
    } else {
        escaped
    }
}

fn delimiter_chars(d: &El) -> (String, String) {
    let val_of = |el: &El, tag: &str| -> Option<String> {
        el.child("m:dPr")
            .and_then(|p| p.child(tag))
            .and_then(|c| c.attr("m:val").or_else(|| c.attr("val")))
            .map(|s| s.to_string())
    };
    let beg = val_of(d, "m:begChr").unwrap_or_else(|| "(".to_string());
    let end = val_of(d, "m:endChr").unwrap_or_else(|| ")".to_string());
    // 其他罕见符号保守映射为尖括号
    let map = |c: &str| -> String {
        match c {
            "(" => "\\left(".into(),
            "[" => "\\left[".into(),
            "{" => "\\left\\{".into(),
            "|" => "\\left|".into(),
            "" => "\\left.".into(),
            _ => "\\left\\langle ".into(),
        }
    };
    let map_end = |c: &str| -> String {
        match c {
            ")" => "\\right)".into(),
            "]" => "\\right]".into(),
            "}" => "\\right\\}".into(),
            "|" => "\\right|".into(),
            "" => "\\right.".into(),
            _ => "\\right\\rangle ".into(),
        }
    };
    (map(&beg), map_end(&end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const M_NS: &str = "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"";

    fn conv(inner: &str) -> Option<String> {
        omml_to_latex(&format!("<m:oMath {M_NS}>{inner}</m:oMath>"))
    }

    #[test]
    fn superscript() {
        let latex = conv(
            "<m:sSup><m:e><m:r><m:t>E=mc</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>",
        )
        .unwrap();
        assert!(latex.contains("E=mc"), "got {latex}");
        assert!(latex.contains("^{2}"), "got {latex}");
    }

    #[test]
    fn fraction() {
        let latex = conv(
            "<m:f><m:num><m:r><m:t>a+b</m:t></m:r></m:num><m:den><m:r><m:t>c+d</m:t></m:r></m:den></m:f>",
        )
        .unwrap();
        assert_eq!(latex, "\\frac{a+b}{c+d}");
    }

    #[test]
    fn subscript_and_both() {
        let latex = conv(
            "<m:sSub><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub>",
        )
        .unwrap();
        assert_eq!(latex, "{x}_{i}");

        let both = conv(
            "<m:sSubSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSubSup>",
        )
        .unwrap();
        assert_eq!(both, "{x}_{i}^{2}");
    }

    #[test]
    fn radical() {
        let latex = conv(
            "<m:rad><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>",
        )
        .unwrap();
        assert_eq!(latex, "\\sqrt{x}");

        let cube = conv(
            "<m:rad><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>",
        )
        .unwrap();
        assert_eq!(cube, "\\sqrt[3]{x}");
    }

    #[test]
    fn nary_sum() {
        let latex = conv(
            "<m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr><m:sub><m:r><m:t>i=1</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:r><m:t>i</m:t></m:r></m:e></m:nary>",
        )
        .unwrap();
        assert!(latex.contains("\\sum"), "got {latex}");
        assert!(latex.contains("_{i=1}"), "got {latex}");
        assert!(latex.contains("^{n}"), "got {latex}");
    }

    #[test]
    fn nary_default_integral() {
        let latex = conv(
            "<m:nary><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary>",
        )
        .unwrap();
        assert!(latex.contains("\\int"), "got {latex}");
    }

    #[test]
    fn delimiters() {
        let latex = conv(
            "<m:d><m:e><m:r><m:t>a</m:t></m:r></m:e></m:d>",
        )
        .unwrap();
        assert!(latex.contains("\\left("), "got {latex}");
        assert!(latex.contains("\\right)"), "got {latex}");

        let brackets = conv(
            "<m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"]\"/></m:dPr><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d>",
        )
        .unwrap();
        assert!(brackets.contains("\\left["), "got {brackets}");
        assert!(brackets.contains("\\right]"), "got {brackets}");
    }

    #[test]
    fn matrix_does_not_crash() {
        let latex = conv(
            "<m:m><m:mr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:mr><m:mr><m:e><m:r><m:t>c</m:t></m:r></m:e><m:e><m:r><m:t>d</m:t></m:r></m:e></m:mr></m:m>",
        )
        .unwrap();
        assert!(latex.contains("\\begin{matrix}"), "got {latex}");
        assert!(latex.contains("a & b"), "got {latex}");
        assert!(latex.contains("\\\\"), "got {latex}");
    }

    #[test]
    fn text_with_space_uses_text_mode() {
        let latex = conv("<m:r><m:t>hello world</m:t></m:r>").unwrap();
        assert_eq!(latex, "\\text{hello world}");
    }

    #[test]
    fn special_chars_escaped() {
        let latex = conv("<m:r><m:t>50%</m:t></m:r>").unwrap();
        assert_eq!(latex, "50\\%");
    }

    #[test]
    fn invalid_xml_returns_none() {
        assert!(omml_to_latex("<m:oMath><m:f>").is_none());
    }

    #[test]
    fn empty_returns_none() {
        assert!(omml_to_latex("<m:oMath></m:oMath>").is_none());
    }
}
