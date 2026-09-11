//! 翻译提示词构建与行内资产令牌化（docs/design.md §5.2 令牌法）

use crate::model::InlineContent;

/// 一个被令牌化的行内资产（图片/公式）
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceholderRef {
    /// 令牌，如 "{{P0}}"
    pub token: String,
    /// 原行内内容（InlineImage / InlineFormula）
    pub inline: InlineContent,
}

/// 提取段落的翻译源文本：
/// - Text → 原文本
/// - LineBreak → 换行
/// - InlineImage / InlineFormula → `{{P0}}`、`{{P1}}` … 令牌
pub fn extract_source_text(content: &[InlineContent]) -> (String, Vec<PlaceholderRef>) {
    let mut text = String::new();
    let mut refs = Vec::new();
    for item in content {
        match item {
            InlineContent::Text { text: t, .. } => text.push_str(t),
            InlineContent::LineBreak => text.push('\n'),
            inline @ (InlineContent::InlineImage { .. } | InlineContent::InlineFormula { .. }) => {
                let token = format!("{{{{P{}}}}}", refs.len());
                text.push_str(&token);
                refs.push(PlaceholderRef {
                    token,
                    inline: inline.clone(),
                });
            }
        }
    }
    (text, refs)
}

fn lang_display(lang: &str) -> String {
    match lang.to_lowercase().as_str() {
        "en" | "english" => "英文".to_string(),
        "zh" | "cn" | "chinese" | "zh-cn" => "中文".to_string(),
        "ja" | "japanese" => "日文".to_string(),
        "fr" | "french" => "法文".to_string(),
        "de" | "german" => "德文".to_string(),
        other => other.to_string(),
    }
}

/// system 提示（plan.md §4.2，追加令牌保护与行内公式保护要求）
pub fn system_prompt(source_lang: &str, target_lang: &str) -> String {
    format!(
        "你是一个专业文档翻译器。请将用户提供的文本从{src}翻译为{tgt}。\n\n\
         要求：\n\
         1. 只翻译文本内容，不要添加任何解释\n\
         2. 保持原文的语气和专业术语\n\
         3. 直接输出译文，不要包含任何标记或前缀\n\
         4. 文本中的 {{{{P0}}}}、{{{{P1}}}} 等双花括号占位标记必须原样保留，不要翻译、移动或删除它们\n\
         5. 正文中的行内公式（$...$ 或 \\(...\\)）必须逐字符原样保留，禁止翻译、\
            禁止增删或修改其中的反斜杠与花括号\
            （v2.2 T5：公式已提取为 {{{{M0}}}}、{{{{M1}}}} 等占位令牌，如出现则必须原样保留，\
            每个恰好出现一次，禁止翻译或改写）",
        src = lang_display(source_lang),
        tgt = lang_display(target_lang),
    )
}

/// 组装 OpenAI 兼容 messages
pub fn build_messages(source_text: &str, source_lang: &str, target_lang: &str) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({ "role": "system", "content": system_prompt(source_lang, target_lang) }),
        serde_json::json!({ "role": "user", "content": source_text }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_and_tokens() {
        let content = vec![
            InlineContent::Text { text: "See ".into(), bold: false, italic: false, underline: false },
            InlineContent::InlineImage { resource_id: "r01_image".into() },
            InlineContent::Text { text: " and ".into(), bold: false, italic: false, underline: false },
            InlineContent::InlineFormula { resource_id: "r02_formula".into() },
            InlineContent::LineBreak,
            InlineContent::Text { text: "end".into(), bold: false, italic: false, underline: false },
        ];
        let (text, refs) = extract_source_text(&content);
        assert_eq!(text, "See {{P0}} and {{P1}}\nend");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].token, "{{P0}}");
        assert!(matches!(refs[0].inline, InlineContent::InlineImage { .. }));
        assert!(matches!(refs[1].inline, InlineContent::InlineFormula { .. }));
    }

    #[test]
    fn system_prompt_mentions_langs_and_tokens() {
        let p = system_prompt("en", "zh");
        assert!(p.contains("英文"));
        assert!(p.contains("中文"));
        assert!(p.contains("{{P0}}"));
        assert!(p.contains("原样保留"));
        // v2.1 T4.2：行内公式约束
        assert!(p.contains("行内公式"));
        assert!(p.contains("$...$"));
        // v2.2 T5：M 令牌约束
        assert!(p.contains("{{M0}}"));
        assert!(p.contains("恰好出现一次"));
    }

    #[test]
    fn messages_shape() {
        let msgs = build_messages("hello", "en", "zh");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "hello");
    }
}
