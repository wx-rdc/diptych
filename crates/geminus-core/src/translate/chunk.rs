//! 短段合并翻译（v2_plan §3.5）：相邻短段落合并为编号 chunk，一次 LLM 调用，
//! 按 [N] 标记拆分回填。编号解析失败 → 回退逐段翻译（translate/mod.rs 集成处保证）。
//!
//! 缓存策略保持段落级：chunk 翻译成功后逐段写入缓存，二次翻译可全缓存命中。

/// 合并配置
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// 最小合并单元字符数（累积到此值即发送）
    pub min_chars: usize,
    /// 单 chunk 最大字符数（超过则切分）
    pub max_chars: usize,
    /// 每 chunk 最多段落数
    pub max_paras: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self { min_chars: 200, max_chars: 2000, max_paras: 10 }
    }
}

impl ChunkConfig {
    /// 环境变量覆盖（GEMINUS_CHUNK_MIN/MAX_CHARS、GEMINUS_CHUNK_MAX_PARAS），默认 v2 §3.5
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("GEMINUS_CHUNK_MIN_CHARS") {
            if let Ok(n) = v.parse() { cfg.min_chars = n; }
        }
        if let Ok(v) = std::env::var("GEMINUS_CHUNK_MAX_CHARS") {
            if let Ok(n) = v.parse() { cfg.max_chars = n; }
        }
        if let Ok(v) = std::env::var("GEMINUS_CHUNK_MAX_PARAS") {
            if let Ok(n) = v.parse() { cfg.max_paras = n; }
        }
        cfg
    }
}

/// 一个合并翻译单元（段落下标区间，左闭右开）
#[derive(Debug, PartialEq)]
pub struct TextChunk {
    pub range: std::ops::Range<usize>,
}

/// 分组规则（纯函数）：
/// - 单段字符数 > max_chars → 独立成块
/// - 累积字符 ≥ min_chars 或段数 ≥ max_paras → 切分
/// - 末尾不足阈值的残余也成块
pub fn build_chunks(texts: &[&str], cfg: &ChunkConfig) -> Vec<TextChunk> {
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut acc_chars = 0usize;
    for (i, t) in texts.iter().enumerate() {
        let n = t.chars().count();
        if n > cfg.max_chars {
            // 超长段独立：先结算累积块
            if i > start {
                chunks.push(TextChunk { range: start..i });
            }
            chunks.push(TextChunk { range: i..i + 1 });
            start = i + 1;
            acc_chars = 0;
            continue;
        }
        acc_chars += n;
        let paras = i - start + 1;
        if acc_chars >= cfg.min_chars || paras >= cfg.max_paras {
            chunks.push(TextChunk { range: start..i + 1 });
            start = i + 1;
            acc_chars = 0;
        }
    }
    if start < texts.len() {
        chunks.push(TextChunk { range: start..texts.len() });
    }
    chunks
}

/// 合并翻译 prompt（v2_plan §3.5 模板）
pub fn build_merged_prompt(texts: &[&str], source_lang: &str, target_lang: &str) -> String {
    let lang_names: &[(&str, &str)] = &[
        ("en", "英语"), ("zh", "中文"), ("ja", "日语"), ("fr", "法语"), ("de", "德语"),
    ];
    let name = |code: &str| {
        lang_names
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| code.to_string())
    };
    let mut prompt = format!(
        "你是一个专业文档翻译器。请将以下编号段落从{}翻译为{}。\n\n要求：\n1. 保持段落编号 [1] [2] [3] ... 不变\n2. 每个编号对应一个段落的译文\n3. 不要添加任何解释或额外内容\n4. 专业术语保持准确\n5. 正文中的行内公式（$...$ 或 \\(...\\)）必须逐字符原样保留，禁止翻译或修改\n6. 双花括号公式占位令牌（如 {{{{M0}}}}、{{{{M12}}}}）必须原样保留，每个恰好出现一次，禁止翻译或改写\n\n",
        name(source_lang),
        name(target_lang),
    );
    for (i, t) in texts.iter().enumerate() {
        prompt.push_str(&format!("[{}] {}\n", i + 1, t));
    }
    prompt
}

/// 按 [N] 标记拆分译文。
///
/// 模型可能在正文前输出说明文字（其中也可能含 `[1] [2]` 字样，如复述指令），
/// 因此不能简单取首个标记：先收集全部 `[数字]` 记号，再取**最紧凑覆盖窗口**
/// （覆盖 1..=para_count 全部编号的最短区间），窗口内每个编号取首次出现，
/// 内容延伸至窗口内下一记号或串尾。无法完整覆盖 → None（触发逐段回退）。
pub fn split_numbered_response(response: &str, para_count: usize) -> Option<Vec<String>> {
    split_numbered_response_with_prefix(response, para_count, None)
}

/// v2.2 T4：前缀参数化版本。`prefix=Some(b'L')` 匹配 `[L n]` 形态（算法行编号），
/// None 保持 `[n]` 形态（段落 chunk 编号）。
pub fn split_numbered_response_with_prefix(
    response: &str,
    para_count: usize,
    prefix: Option<u8>,
) -> Option<Vec<String>> {
    if para_count == 0 || response.is_empty() {
        return None;
    }
    // 1. 收集记号：(编号, '[' 起始字节, 记号后内容起始字节)。仅保留 1..=para_count。
    let bytes = response.as_bytes();
    let mut marks: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            if let Some(p) = prefix {
                if j >= bytes.len() || bytes[j] != p {
                    i += 1;
                    continue;
                }
                j += 1;
            }
            let mut num: usize = 0;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num = num * 10 + (bytes[j] - b'0') as usize;
                j += 1;
                digits += 1;
            }
            if digits > 0 && j < bytes.len() && bytes[j] == b']' && num >= 1 && num <= para_count {
                marks.push((num, i, j + 1));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    if marks.is_empty() {
        return None;
    }
    // 2. 滑动窗口：最短区间覆盖全部 1..=para_count
    let mut count = vec![0usize; para_count + 1];
    let mut covered = 0usize;
    let mut best: Option<(usize, usize)> = None; // marks 下标区间 [lo, hi)
    let mut lo = 0usize;
    for hi in 0..marks.len() {
        let n = marks[hi].0;
        count[n] += 1;
        if count[n] == 1 {
            covered += 1;
        }
        while covered == para_count {
            let len = hi - lo + 1;
            if best.is_none_or(|(blo, bhi)| len <= bhi - blo) {
                best = Some((lo, hi + 1));
            }
            let ln = marks[lo].0;
            count[ln] -= 1;
            if count[ln] == 0 {
                covered -= 1;
            }
            lo += 1;
        }
    }
    let (w_lo, w_hi) = best?;
    // 3. 窗口内每编号取首次出现；内容到窗口内下一记号或串尾
    let mut results = vec![String::new(); para_count];
    let mut taken = vec![false; para_count];
    for k in w_lo..w_hi {
        let (num, _bracket_start, content_start) = marks[k];
        if taken[num - 1] {
            continue;
        }
        // 内容边界：窗口内下一记号的 '[' 起始（跨记号跳过时同理），否则串尾
        let end = marks.get(k + 1).map(|(_, b, _)| *b).unwrap_or(response.len());
        let text = response[content_start..end].trim_matches(|c: char| c == '\n' || c == ' ');
        results[num - 1] = text.to_string();
        taken[num - 1] = true;
    }
    if taken.iter().all(|t| *t) {
        Some(results)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_short_paragraphs_merge_into_one_chunk() {
        let paras: Vec<String> = (0..5).map(|i| format!("Short paragraph number {i} about ML.")).collect();
        let refs: Vec<&str> = paras.iter().map(|s| s.as_str()).collect();
        let chunks = build_chunks(&refs, &ChunkConfig::default());
        // 5×31=155 字符 < min(200) → 1 块
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].range, 0..5);
    }

    #[test]
    fn long_paragraph_is_own_chunk() {
        let long = "x".repeat(2500);
        let tiny = "tiny".to_string();
        let refs = vec![long.as_str(), tiny.as_str()];
        let chunks = build_chunks(&refs, &ChunkConfig::default());
        assert_eq!(chunks[0].range, 0..1, "超长段独立");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].range, 1..2);
    }

    #[test]
    fn config_thresholds() {
        // 12 段各 15 字符：10 段仅 150 字符（<min 200）→ max_paras 触发切分 + 残余 2 段
        let paras: Vec<String> = (0..12).map(|i| format!("para {i:02}").repeat(3)).collect();
        assert_eq!(paras[0].chars().count(), 21);
        let refs: Vec<&str> = paras.iter().map(|s| s.as_str()).collect();
        let chunks = build_chunks(&refs, &ChunkConfig::default());
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert_eq!(chunks[0].range, 0..10);
        assert_eq!(chunks[1].range, 10..12);
    }

    #[test]
    fn min_chars_splits_before_max_paras() {
        // 各 50 字符段：第 4 段累积 200 ≥ min_chars → 提前切（每 4 段一块）
        let paras: Vec<String> = (0..12).map(|i| format!("{i:05}").repeat(10)).collect();
        let refs: Vec<&str> = paras.iter().map(|s| s.as_str()).collect();
        let chunks = build_chunks(&refs, &ChunkConfig::default());
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert_eq!(chunks[0].range, 0..4);
        assert_eq!(chunks[2].range, 8..12);
    }

    #[test]
    fn min_chars_threshold_splits() {
        // 4 段各 60 字符：第 4 段累积 240 ≥ 200 → 切为 0..4，残余无
        let paras: Vec<String> = (0..4).map(|_| "y".repeat(60)).collect();
        let refs: Vec<&str> = paras.iter().map(|s| s.as_str()).collect();
        let chunks = build_chunks(&refs, &ChunkConfig::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].range, 0..4);
        // 5 段各 60：300≥200 在第 4 段切 → 0..4 + 4..5
        let paras: Vec<String> = (0..5).map(|_| "y".repeat(60)).collect();
        let refs: Vec<&str> = paras.iter().map(|s| s.as_str()).collect();
        let chunks = build_chunks(&refs, &ChunkConfig::default());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].range, 4..5);
    }

    #[test]
    fn merged_prompt_has_numbered_paragraphs() {
        let p = build_merged_prompt(&["Alpha.", "Beta."], "en", "zh");
        assert!(p.contains("[1] Alpha."), "{p}");
        assert!(p.contains("[2] Beta."), "{p}");
        assert!(p.contains("保持段落编号"), "{p}");
        assert!(p.contains("英语"), "{p}");
        assert!(p.contains("中文"), "{p}");
        // v2.1 T4.2：行内公式约束
        assert!(p.contains("行内公式"), "{p}");
    }

    #[test]
    fn split_numbered_response_basic() {
        let parts = split_numbered_response("[1] 第一段\n[2] 第二段", 2).unwrap();
        assert_eq!(parts, vec!["第一段", "第二段"]);
    }

    #[test]
    fn split_numbered_response_multiline_paragraphs() {
        let resp = "前言废话\n[1] 第一段\n继续行\n[2] 第二段";
        let parts = split_numbered_response(resp, 2).unwrap();
        assert_eq!(parts[0], "第一段\n继续行");
        assert_eq!(parts[1], "第二段");
    }

    #[test]
    fn split_numbered_response_detects_failure() {
        assert!(split_numbered_response("模型未按格式输出", 2).is_none());
        assert!(split_numbered_response("[1] only one", 2).is_none());
        assert!(split_numbered_response("", 0).is_none());
    }

    #[test]
    fn split_numbered_response_out_of_range_ignored() {
        let parts = split_numbered_response("[0] zero\n[1] one\n[2] two", 2).unwrap();
        assert_eq!(parts, vec!["one", "two"]);
    }

    #[test]
    fn split_numbered_response_extra_prefix_and_suffix() {
        let resp = "好的，以下是译文：\n[1] 甲\n[2] 乙\n希望有帮助";
        let parts = split_numbered_response(resp, 2).unwrap();
        assert_eq!(parts[0], "甲");
        assert_eq!(parts[1], "乙\n希望有帮助");
    }

    #[test]
    fn split_numbered_response_instruction_prefix_with_numbers() {
        // 模型先复述指令（含 [1] [2] [3] 字样）再给译文 → 取最紧凑窗口（译文部分）
        let resp = "请保持段落编号 [1] [2] [3] 不变。\n[1] 甲段\n[2] 乙段";
        let parts = split_numbered_response(resp, 2).unwrap();
        assert_eq!(parts, vec!["甲段", "乙段"]);
    }

    #[test]
    fn split_numbered_response_no_false_positive_on_brackets() {
        // 文本中的 [see Figure 3] 不是编号标记
        assert!(split_numbered_response("结果 [see Figure 3] 显示", 1).is_none());
    }
}
