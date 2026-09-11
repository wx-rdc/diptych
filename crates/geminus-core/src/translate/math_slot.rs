//! 行内公式占位符化（v2.2 T5）：公式根本不经过模型。
//!
//! 翻译前：`$...$` / `\(...\)` 提取为 `{{M<n>}}` 纯 ASCII 令牌（不含 `$` 字面量，
//! 天然免疫 v2.1 retry_prompt 的回声假平衡污染）；翻译后校验令牌集合恰好；
//! 回填用原文公式源码逐位替换——模型无任何机会改写公式。
//! 与 v2.1 计数配平（math_guard）的关系：令牌校验是严格更强的防线（集合相等 ⊃ 计数相等），
//! 配平降级为回填后的日志告警。

/// 提取行内公式为 `{{M<n>}}` 令牌。规则与 math_guard::count_inline_math 一致：
/// `$...$`（单行、`$` 前无反斜杠、内容非空）与 `\(...\)`（单行）。
pub fn extract(s: &str) -> (String, Vec<String>) {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut slots: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let (end, hit) = crate::translate::math_guard::scan_math_span(s, i);
        if hit {
            out.push_str(&format!("{{{{M{}}}}}", slots.len()));
            slots.push(s[i..end].to_string());
            i = end;
        } else {
            let ch_len = utf8_len(bytes[i]);
            out.push_str(&s[i..i + ch_len]);
            i += ch_len;
        }
    }
    (out, slots)
}

/// 令牌集合恰好校验：slotted 与 translated 的令牌多重集完全相等
/// （无缺失/重复/多余）。slotted 无令牌（纯文本）→ 恒 true。
/// 支持任意编号（chunk 路径经 renumber_tokens 全局偏移后编号不从 0 开始）。
pub fn verify(slotted: &str, translated: &str) -> bool {
    let mut src: Vec<usize> = token_iter(slotted).collect();
    if src.is_empty() {
        return true;
    }
    src.sort();
    if src.windows(2).any(|w| w[0] == w[1]) {
        return false; // slotted 自身重复（异常产物，防御）
    }
    let mut tgt: Vec<usize> = token_iter(translated).collect();
    tgt.sort();
    src == tgt
}

/// 回填：`{{M<n>}}` → 原文公式源码（未知令牌原样保留；verify 已拦截，此处兜底不 panic）
pub fn restore(translated: &str, slots: &[String]) -> String {
    let bytes = translated.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' && translated[i..].starts_with("{{M") {
            let mut j = i + 3;
            let mut num = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num.push(bytes[j] as char);
                j += 1;
            }
            if !num.is_empty() && translated[j..].starts_with("}}") {
                if let Ok(idx) = num.parse::<usize>() {
                    if idx < slots.len() {
                        out.push_str(&slots[idx]);
                        i = j + 2;
                        continue;
                    }
                }
            }
        }
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&translated[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// chunk 合并交互（v2.2 T5.2）：组内 M 令牌全局偏移重编号，避免跨段冲突。
/// `{{M3}}` + offset 5 → `{{M8}}`。
pub fn renumber_tokens(slotted: &str, offset: usize) -> String {
    if offset == 0 {
        return slotted.to_string();
    }
    let bytes = slotted.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' && slotted[i..].starts_with("{{M") {
            let mut j = i + 3;
            let mut num = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num.push(bytes[j] as char);
                j += 1;
            }
            if !num.is_empty() && slotted[j..].starts_with("}}") {
                let n: usize = num.parse().unwrap_or(0);
                out.push_str(&format!("{{{{M{}}}}}", n + offset));
                i = j + 2;
                continue;
            }
        }
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&slotted[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// slots 前插 offset 个占位（renumber 后索引对齐 restore 的直接索引）
pub fn padded_slots(slots: &[String], offset: usize) -> Vec<String> {
    (0..offset).map(|_| String::new()).chain(slots.iter().cloned()).collect()
}

/// slotted 的源令牌中在 translated 至少出现一次的去重计数（v2.3 B4 候选择优用）
pub fn present_tokens(slotted: &str, translated: &str) -> usize {
    let mut src: Vec<usize> = token_iter(slotted).collect();
    src.sort();
    src.dedup();
    let tgt: Vec<usize> = token_iter(translated).collect();
    src.into_iter().filter(|n| tgt.contains(n)).count()
}

/// 强制回填（v2.3 B4）：两次校验失败后的兜底——按位换回可对上的令牌，
/// 重复/未知令牌剥除，缺失槽位的公式按原序追加到译文末尾。
/// 与 restore 的区别：不要求令牌集合恰好；保公式内容，牺牲缺失项位置精度。
/// 返回 (回填后文本, 缺失槽位下标)。
pub fn force_restore(translated: &str, slots: &[String]) -> (String, Vec<usize>) {
    let mut used = vec![false; slots.len()];
    let bytes = translated.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' && translated[i..].starts_with("{{M") {
            let mut j = i + 3;
            let mut num = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num.push(bytes[j] as char);
                j += 1;
            }
            if !num.is_empty() && translated[j..].starts_with("}}") {
                if let Ok(idx) = num.parse::<usize>() {
                    // 一切形态完整的 {{Mn}} 令牌都被消费：有效且未用 → 换回公式；
                    // 重复（已用）/越界（伪造）→ 剥除
                    if idx < slots.len() && !used[idx] {
                        out.push_str(&slots[idx]);
                        used[idx] = true;
                    }
                    i = j + 2;
                    continue;
                }
            }
        }
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&translated[i..i + ch_len]);
        i += ch_len;
    }
    let missing: Vec<usize> = used
        .iter()
        .enumerate()
        .filter(|(_, u)| !**u)
        .map(|(n, _)| n)
        .collect();
    for &n in &missing {
        out.push(' ');
        out.push_str(&slots[n]);
    }
    (out, missing)
}

fn token_iter(s: &str) -> impl Iterator<Item = usize> {
    let b = s.as_bytes();
    let mut v = Vec::new();
    let mut i = 0usize;
    while i + 3 < b.len() {
        if b[i] == b'{' && &s[i..i + 3] == "{{M" {
            let mut j = i + 3;
            let mut num = String::new();
            while j < b.len() && b[j].is_ascii_digit() {
                num.push(b[j] as char);
                j += 1;
            }
            if !num.is_empty() && s[j..].starts_with("}}") {
                if let Ok(n) = num.parse() {
                    v.push(n);
                }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    v.into_iter()
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_and_restore_roundtrip() {
        let src = "loss is $J(\\phi)$ and rate \\(r = 0.1\\) ok";
        let (slotted, slots) = extract(src);
        assert_eq!(slotted, "loss is {{M0}} and rate {{M1}} ok");
        assert_eq!(slots, vec!["$J(\\phi)$".to_string(), r"\(r = 0.1\)".to_string()]);
        assert_eq!(
            restore("损失为 {{M0}} 且速率 {{M1}} 完成", &slots),
            "损失为 $J(\\phi)$ 且速率 \\(r = 0.1\\) 完成"
        );
        // 完整往返：restore(verify 通过的译文) 中公式逐位 == 原文
        assert!(restore(&slotted, &slots) == src);
    }

    #[test]
    fn extract_none_for_plain_text() {
        let (s, slots) = extract("纯文本 no math");
        assert_eq!(s, "纯文本 no math");
        assert!(slots.is_empty());
    }

    #[test]
    fn extract_rules_match_math_guard() {
        // 转义 \$ 不算；跨行 $ 不算；空 $$ 不算——与 count_inline_math 一致
        let (s, slots) = extract(r"价格 \$5 与 $x$ 和 $$ b");
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0], "$x$");
        assert!(s.contains("{{M0}}"));
        assert!(!s.contains("$x$"));
        let (s2, slots2) = extract("a $b\nc d");
        assert!(slots2.is_empty());
        assert_eq!(s2, "a $b\nc d");
    }

    #[test]
    fn verify_detects_missing_and_extra() {
        let (slotted, _) = extract("a $x$ b $y$ c");
        assert!(verify(&slotted, "甲 {{M0}} 乙 {{M1}} 丙"));
        assert!(!verify(&slotted, "甲 {{M0}} 乙")); // 缺 M1
        assert!(!verify(&slotted, "甲 {{M0}} {{M0}} 乙 {{M1}}")); // 重复 M0
        assert!(!verify(&slotted, "甲 {{M2}} 乙 {{M0}} 丙 {{M1}}")); // 多余 M2
        assert!(verify("纯文本", "译文")); // 无令牌恒过
    }

    #[test]
    fn restore_ignores_unknown_tokens() {
        let (_, slots) = extract("a $x$ b");
        assert_eq!(restore("甲 {{M0}} 乙 {{M9}}", &slots), "甲 $x$ 乙 {{M9}}");
    }

    #[test]
    fn model_rewrite_never_reaches_output() {
        // Δ 注入模拟：模型只要保留令牌，回填后公式必为原文源码
        let src = "能量 $E = mc^2$ 守恒";
        let (slotted, slots) = extract(src);
        let resp = "能量 {{M0}} 守恒定律";
        assert!(verify(&slotted, resp));
        assert!(restore(resp, &slots).contains("$E = mc^2$"));
        assert_eq!(restore(resp, &slots), "能量 $E = mc^2$ 守恒定律");
    }

    #[test]
    fn renumber_and_padded_roundtrip() {
        let (slotted, slots) = extract("a $x$ b $y$");
        let renumbered = renumber_tokens(&slotted, 2);
        assert_eq!(renumbered, "a {{M2}} b {{M3}}");
        // 偏移后校验 + 回填仍正确（chunk 路径）
        assert!(verify(&renumbered, "甲 {{M2}} 乙 {{M3}}"));
        let padded = padded_slots(&slots, 2);
        assert_eq!(restore("甲 {{M2}} 乙 {{M3}}", &padded), "甲 $x$ 乙 $y$");
    }

    #[test]
    fn renumber_zero_offset_is_identity() {
        assert_eq!(renumber_tokens("x {{M0}} y", 0), "x {{M0}} y");
    }

    #[test]
    fn present_tokens_counts_distinct_hits() {
        let (slotted, _) = extract("a $x$ b $y$ c $z$");
        assert_eq!(present_tokens(&slotted, "甲 {{M0}} 乙 {{M2}}"), 2);
        assert_eq!(present_tokens(&slotted, "甲 {{M0}} {{M0}} 乙"), 1); // 重复算 1
        assert_eq!(present_tokens(&slotted, "无令牌"), 0);
    }

    #[test]
    fn force_restore_replaces_strips_and_appends() {
        let (_, slots) = extract("a $x$ b $y$ c $z$");
        // M1 被模型吞掉、M0 重复、M9 伪造
        let (out, missing) = force_restore("甲 {{M0}} {{M0}} 丙 {{M9}}", &slots);
        assert_eq!(out, "甲 $x$  丙  $y$ $z$");
        assert_eq!(missing, vec![1, 2]);
    }

    #[test]
    fn force_restore_all_present_equals_restore() {
        let (_slotted, slots) = extract("a $x$ b $y$");
        let (out, missing) = force_restore("甲 {{M0}} 乙 {{M1}}", &slots);
        assert_eq!(out, restore("甲 {{M0}} 乙 {{M1}}", &slots));
        assert!(missing.is_empty());
    }

    #[test]
    fn force_restore_empty_slots_is_passthrough() {
        let (out, missing) = force_restore("纯译文", &[]);
        assert_eq!(out, "纯译文");
        assert!(missing.is_empty());
    }
}
