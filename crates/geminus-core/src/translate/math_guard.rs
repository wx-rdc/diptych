//! 行内公式配平防线（v2.1 T4.3）：翻译前后 $...$ / \(...\) 计数必须相等，
//! 防止模型增删/合并公式。不等 → 上层带强调重试 1 次 → 仍不等回退原文（fallback=true）。

/// 从 `i` 位置扫描一个行内公式 span：`$...$`（单行、内容非空、`$` 前无反斜杠）
/// 或 `\(...\)`（单行）。命中返回 (闭合定界符之后的下一字节位置, true)；
/// 未命中返回 (i+1 的字符推进量, false)。
/// math_slot::extract 与 count_inline_math 共用同一套规则（v2.2 T5.1）。
pub fn scan_math_span(s: &str, i: usize) -> (usize, bool) {
    let bytes = s.as_bytes();
    if i >= bytes.len() {
        return (i, false);
    }
    match bytes[i] {
        b'$' if i == 0 || bytes[i - 1] != b'\\' => {
            // 找同行闭合 $（非转义、内容非空）
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'\n' {
                    break;
                }
                if bytes[j] == b'$' && bytes[j - 1] != b'\\' {
                    if j > i + 1 {
                        return (j + 1, true);
                    }
                    break;
                }
                j += 1;
            }
            (i + 1, false)
        }
        b'\\' if i + 1 < bytes.len() && bytes[i + 1] == b'(' => {
            let mut j = i + 2;
            while j < bytes.len() {
                if bytes[j] == b'\n' {
                    break;
                }
                if bytes[j] == b'\\' && j + 1 < bytes.len() && bytes[j + 1] == b')' {
                    return (j + 2, true);
                }
                j += 1;
            }
            (i + 1, false)
        }
        _ => (i + 1, false),
    }
}

/// 统计行内公式数：`$...$`（单行、内容非空、`$` 前无反斜杠）或 `\(...\)`（单行）。
/// 与前端 app/src/utils/inlineMath.ts 的 MATH_RE 规则保持一致。
pub fn count_inline_math(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let (next, hit) = scan_math_span(s, i);
        if hit {
            count += 1;
        }
        i = next.max(i + 1);
    }
    count
}

/// 源/译文行内公式数相等（配平）。0==0 也算配平（无公式段不设防）。
pub fn balanced(source: &str, target: &str) -> bool {
    count_inline_math(source) == count_inline_math(target)
}

/// 配平重试的强调 user content（前置警示，原文附后）。
/// 警示语刻意不含公式定界符字面量（美元符/反斜杠圆括号）——
/// 避免回声场景下污染 count_inline_math 的配平计数。
pub fn retry_prompt(source_text: &str) -> String {
    format!(
        "注意：上一次译文损坏了行内公式。请将下面的文本翻译为中文，\
         其中所有行内数学公式（无论是美元定界符还是圆括号定界符的形式）\
         必须逐字符原样保留（禁止翻译、增删、移动），只翻译公式之外的普通文本：\n\n{source_text}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_dollar_and_paren_forms() {
        assert_eq!(count_inline_math("a $x$ b \\(y\\) $z$"), 3);
        assert_eq!(count_inline_math("no math here"), 0);
    }

    #[test]
    fn escaped_dollar_not_counted() {
        assert_eq!(count_inline_math(r"价格 \$5 与 $x$"), 1);
    }

    #[test]
    fn newline_inside_dollar_not_matched() {
        assert_eq!(count_inline_math("a $b\nc"), 0);
    }

    #[test]
    fn empty_dollar_not_counted() {
        assert_eq!(count_inline_math("a $$ b"), 0);
    }

    #[test]
    fn balanced_rules() {
        assert!(balanced("has $x$ here", "有 $x$ 在这"));
        assert!(!balanced("has $x$ and $y$", "只有 $x$"));
        assert!(balanced("plain", "纯文本"));
        // 中文内联 $$：空公式两边都不计
        assert!(balanced("a $$ b", "甲 $$ 乙"));
    }
}
