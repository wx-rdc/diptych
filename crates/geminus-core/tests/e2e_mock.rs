//! 端到端集成测试（任务 T7）：解析真实夹具 → mock LLM → 翻译编排 → 结果对齐

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::routing::post;
use axum::Router;
use geminus_core::cache::TranslationCache;
use geminus_core::model::{Block, InlineContent, ProgressStatus};
use geminus_core::translate::{translate_document, LlmClient};
use geminus_core::{parse_docx, ApiConfig};
use tokio::sync::mpsc;

/// 回声 mock：把用户内容包装后返回（保留 {{Pn}} 令牌），
/// 同时统计并发峰值与总请求数。
struct MockLlm {
    calls: Arc<AtomicUsize>,
    peak_concurrency: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    /// 若 true，模拟令牌被破坏（丢弃所有 {{Pn}}）
    destroy_tokens: bool,
    /// numbered：识别合并 prompt（含"保持段落编号"）时返回规范 [N] 编号响应
    numbered: bool,
    /// 若 true，把响应中所有 [N] 编号改成 (N)（模拟模型不按编号格式输出）
    destroy_numbers: bool,
    /// user 内容含此关键字时返回 500（失败注入，触发管线兜底路径）
    fail_on: Option<String>,
    /// 行内公式令牌破坏模式（v2.1 T4.3 → v2.2 T5 演进：公式不经过模型，破坏面是 {{Mn}}）
    math_break: MathBreak,
    /// RetryHeals 模式的同内容请求计数（第二次恢复）
    heal_counts: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, u32>>>,
    /// v2.2 T4：识别算法行 prompt（含 [L1]）时返回规范 [L n] 编号响应
    algo_lines: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum MathBreak {
    #[default]
    Off,
    /// v2.2 T5：吞掉 {{Mn}} 令牌（模拟模型破坏占位符）；同一内容的第二次请求恢复回声
    RetryHeals,
    /// 重试也坏（持续损坏 → 回退原文）
    Always,
}

/// 去 {{M数字}} 令牌
fn strip_m_tokens(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'{' && s[i..].starts_with("{{M") {
            let mut j = i + 3;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if s[j..].starts_with("}}") {
                i = j + 2;
                continue;
            }
        }
        let l = if b[i] < 0x80 { 1 } else if b[i] < 0xE0 { 2 } else if b[i] < 0xF0 { 3 } else { 4 };
        out.push_str(&s[i..i + l]);
        i += l;
    }
    out
}

impl MockLlm {
    async fn spawn(self) -> String {
        let MockLlm { calls, peak_concurrency, active, destroy_tokens, numbered, destroy_numbers, fail_on, math_break, heal_counts, algo_lines } = self;
        let router = Router::new().route(
            "/chat/completions",
            post(move |body: axum::Json<serde_json::Value>| {
                let calls = calls.clone();
                let peak = peak_concurrency.clone();
                let active = active.clone();
                let fail_on = fail_on.clone();
                let heal_counts = heal_counts.clone();
                async move {
                    let cur = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(cur, Ordering::SeqCst);
                    calls.fetch_add(1, Ordering::SeqCst);

                    let user = body.0["messages"]
                        .as_array()
                        .and_then(|ms| ms.last().cloned())
                        .and_then(|m| m["content"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    if std::env::var("GEMINUS_DEBUG_MOCK").is_ok() {
                        eprintln!("[mock] user={:?} math_break={:?}", user.chars().take(60).collect::<String>(), math_break);
                    }
                    if let Some(kw) = &fail_on {
                        if user.contains(kw.as_str()) {
                            active.fetch_sub(1, Ordering::SeqCst);
                            return (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                axum::Json(serde_json::json!({ "error": "mock upstream failure" })),
                            );
                        }
                    }
                    let content = if !algo_lines && user.contains("[L1]") {
                        // 非算法感知模式遇到算法行 prompt：返回无编号胡乱输出（模拟模型不守格式）
                        "抱歉，我无法按格式输出。".to_string()
                    } else if algo_lines && user.contains("[L1]") {
                        // 算法行请求：回声为规范 [L n] 编号格式（提取 prompt 中的行）
                        let mut out = String::from("好的，以下是译文：\n");
                        for line in user.lines() {
                            if let Some(rest) = line.strip_prefix("[L") {
                                if let Some((num, text)) = rest.split_once("] ") {
                                    if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                                        out.push_str(&format!("[L{num}] 【译】{text}\n"));
                                    }
                                }
                            }
                        }
                        out
                    } else if math_break != MathBreak::Off && user.contains("{{M") {
                        // v2.2 T5：令牌破坏（公式已不经过模型，破坏面是 {{Mn}} 占位符）
                        let heal = math_break == MathBreak::RetryHeals && {
                            let mut seen = heal_counts.lock().unwrap();
                            let c = seen.entry(user.clone()).or_insert(0);
                            *c += 1;
                            *c >= 2 // 同一内容的第二次请求（管线重试）恢复回声
                        };
                        if heal {
                            format!("【重试译】{user}")
                        } else {
                            format!("【坏译】{}", strip_m_tokens(&user))
                        }
                    } else if destroy_numbers {
                        let mut out = user.clone();
                        for i in 1..=20 {
                            out = out.replace(&format!("[{i}] "), &format!("({i}) "));
                        }
                        format!("【无编号】{out}")
                    } else if numbered && user.contains("保持段落编号") {
                        // 合并请求：回声为规范编号格式
                        let mut out = String::from("好的，以下是译文：\n");
                        for line in user.lines() {
                            if let Some(rest) = line.strip_prefix('[') {
                                if let Some((num, text)) = rest.split_once("] ") {
                                    if num.chars().all(|c| c.is_ascii_digit()) && !num.is_empty() {
                                        let _ = text;
                                        out.push_str(&format!("[{num}] 【合并译】段落 {num}\n"));
                                    }
                                }
                            }
                        }
                        out
                    } else if destroy_tokens {
                        // 模拟 LLM 破坏令牌：去掉所有 {{Pn}}
                        let mut out = user.clone();
                        for tok in ["{{P0}}", "{{P1}}", "{{P2}}", "{{P3}}"] {
                            out = out.replace(tok, "");
                        }
                        format!("【无令牌译】{out}")
                    } else {
                        format!("【译】{user}")
                    };

                    active.fetch_sub(1, Ordering::SeqCst);
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "choices": [{"message": {"content": content}, "finish_reason": "stop"}],
                            "model": "mock"
                        })),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }
}

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/rich.docx")
}

async fn run_translate(
    base: &str,
    doc: &geminus_core::Document,
    cache: Arc<TranslationCache>,
    concurrency: usize,
) -> (geminus_core::TranslatedDocument, Vec<geminus_core::TranslateProgress>) {
    let client = Arc::new(LlmClient::new(ApiConfig::new(base, "sk-test", "mock-model")));
    let (tx, mut rx) = mpsc::channel(1024);
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let events2 = events.clone();
    let reader = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            events2.lock().unwrap().push(ev);
        }
    });

    // translate_document 返回时其内部所有 Sender 克隆均已 drop，reader 会自然排空退出
    let result = translate_document(doc, client, cache, tx, concurrency, "en", "zh").await;
    let events = match reader.await {
        Ok(_) => events.lock().unwrap().clone(),
        Err(e) => panic!("进度读取任务失败: {e}"),
    };
    (result, events)
}

#[tokio::test]
async fn end_to_end_with_mock_llm() {
    let doc = parse_docx(&fixture_path()).unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let base = MockLlm {
        calls: calls.clone(),
        peak_concurrency: peak.clone(),
        active,
        destroy_tokens: false,
        numbered: false,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;

    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache.clone(), 3).await;

    // 对齐：pairs 与原 blocks 等长、逐位 block_id 相等
    assert_eq!(result.pairs.len(), doc.blocks.len());
    for (pair, block) in result.pairs.iter().zip(doc.blocks.iter()) {
        assert_eq!(pair.block_id, block.id());
    }

    let paragraph_count = doc
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Paragraph { .. }))
        .count();

    // 全部 Paragraph 有译文（mock 永不失败），占位符/空段 translated=None
    for pair in &result.pairs {
        match &pair.original {
            Block::Paragraph { .. } => assert!(pair.translated.is_some()),
            _ => assert!(pair.translated.is_none()),
        }
    }
    let translated_paras = result.pairs.iter().filter(|p| p.translated.is_some()).count();
    assert_eq!(translated_paras, paragraph_count);

    // 译文内容：行内图片段落令牌保位（InlineImage 保留在译文 content 中）
    let inline_img_pair = result
        .pairs
        .iter()
        .find(|p| {
            p.original
                .paragraph_content()
                .map(|c| c.iter().any(|i| matches!(i, InlineContent::InlineImage { .. })))
                .unwrap_or(false)
        })
        .expect("rich.docx 应有含行内图片的段落");
    let tcontent = inline_img_pair
        .translated
        .as_ref()
        .unwrap()
        .paragraph_content()
        .unwrap();
    assert!(
        tcontent.iter().any(|i| matches!(i, InlineContent::InlineImage { .. })),
        "令牌保位：译文中应保留行内图片"
    );
    let joined: String = tcontent
        .iter()
        .filter_map(|i| match i {
            InlineContent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    // v2 合并翻译：该段走合并路径时译文来自 [N] 拆分（回声 mock 下为原文本身），
    // 不再带【译】前缀；只要求非空 + 令牌已被 rebuild 消费
    assert!(!joined.trim().is_empty(), "合并路径译文非空：{joined}");

    // 并发峰值 ≤ 3
    assert!(
        peak.load(Ordering::SeqCst) <= 3,
        "并发峰值 {} > 3",
        peak.load(Ordering::SeqCst)
    );

    // 进度事件：Done+Cached 总数 == 段落数；最后 current == total
    let done_count = events
        .iter()
        .filter(|e| matches!(e.status, ProgressStatus::Done | ProgressStatus::Cached))
        .count();
    assert_eq!(done_count, paragraph_count);
    let max_current = events.iter().map(|e| e.current).max().unwrap();
    assert_eq!(max_current as usize, paragraph_count);

    // 第二次翻译（同缓存）：0 次 LLM 调用，全部 Cached
    let calls_before = calls.load(Ordering::SeqCst);
    assert!(calls_before > 0, "首轮应有 LLM 调用");
    let (_r2, events2) = run_translate(&base, &doc, cache.clone(), 3).await;
    assert_eq!(
        calls.load(Ordering::SeqCst) - calls_before,
        0,
        "缓存命中后不应再调 LLM"
    );
    let cached_count = events2
        .iter()
        .filter(|e| matches!(e.status, ProgressStatus::Cached))
        .count();
    assert_eq!(cached_count, paragraph_count);
}

#[tokio::test]
async fn degraded_tokens_still_preserve_assets() {
    let doc = parse_docx(&fixture_path()).unwrap();
    let base = MockLlm {
        calls: Arc::new(AtomicUsize::new(0)),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: true,
        numbered: false,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;

    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, _) = run_translate(&base, &doc, cache, 3).await;

    // 令牌被 mock 破坏后：行内资产仍应出现在译文段（降级追加）
    let inline_img_pair = result
        .pairs
        .iter()
        .find(|p| {
            p.original
                .paragraph_content()
                .map(|c| c.iter().any(|i| matches!(i, InlineContent::InlineImage { .. })))
                .unwrap_or(false)
        })
        .expect("应有行内图片段落");
    let tcontent = inline_img_pair
        .translated
        .as_ref()
        .unwrap()
        .paragraph_content()
        .unwrap();
    assert!(
        tcontent.iter().any(|i| matches!(i, InlineContent::InlineImage { .. })),
        "降级路径行内资产不应丢失"
    );
}

/// 构造 N 个短正文段 + 1 个标题的文档
fn synth_doc(short_paras: usize) -> geminus_core::Document {
    synth_doc_with_heading(short_paras, "Synthetic Title", "Short body paragraph number {i} about machine learning.")
}

/// 参数化标题与正文模板的合成文档（{i} 占位段号）
fn synth_doc_with_heading(short_paras: usize, heading: &str, body_tpl: &str) -> geminus_core::Document {
    use geminus_core::model::*;
    let mut blocks = vec![Block::Paragraph {
        id: "b_0000".into(),
        style: ParagraphStyle { heading_level: Some(1), ..Default::default() },
        content: vec![InlineContent::Text { text: heading.into(), bold: false, italic: false, underline: false }],
    }];
    for i in 1..=short_paras {
        blocks.push(Block::Paragraph {
            id: format!("b_{i:04}"),
            style: ParagraphStyle::default(),
            content: vec![InlineContent::Text { text: body_tpl.replace("{i}", &i.to_string()), bold: false, italic: false, underline: false }],
        });
    }
    Document { id: "synthetic".into(), title: "Synthetic".into(), blocks, resources: Default::default(), render_meta: Default::default(), pdf_meta: None }
}

fn pair_text(pair: &geminus_core::model::TranslatedBlock) -> String {
    pair.translated
        .as_ref()
        .and_then(|b| b.paragraph_content().cloned())
        .map(|c| {
            c.iter()
                .filter_map(|i| match i {
                    InlineContent::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn math_guard_retry_heals_broken_formula() {
    // 标题独立路径 + 正文合并路径（回声吞公式）双双触发配平重试，重试恢复公式（T4.3）
    let doc = synth_doc_with_heading(2, "Accuracy $10^{-3}$ study", "Error bar $x^{i}$ is small.");
    let base = MockLlm {
        calls: Arc::new(AtomicUsize::new(0)),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: true,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::RetryHeals,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache, 3).await;

    // 全段落：配平后带公式的译文（【重试译】前缀），无 fallback
    for pair in &result.pairs {
        assert!(!pair.fallback, "{} 不应 fallback", pair.block_id);
        assert!(pair_text(pair).contains("【重试译】"), "应经重试恢复: {}", pair_text(pair));
        assert!(pair_text(pair).contains('$'), "公式保位: {}", pair_text(pair));
    }
    assert!(events.iter().all(|e| e.status != ProgressStatus::Error), "重试成功不应有 Error");
}

#[tokio::test]
async fn math_guard_persistent_break_force_restores() {
    // 两次都坏 → v2.3 B4 起不再整段回退：择优译文强制回填（缺失公式按序追加）+
    // 告警日志 + Done 事件；fallback 仅剩 LLM 调用失败一条路
    let doc = synth_doc_with_heading(1, "Range $[0, 1]$ test", "Plain body no math here.");
    let base = MockLlm {
        calls: Arc::new(AtomicUsize::new(0)),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: false,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Always,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache, 3).await;

    // 标题（独立路径，公式持续损坏）：不 fallback，译文含强制回填的原文公式
    let heading = &result.pairs[0];
    assert!(!heading.fallback, "v2.3 起持续损坏应强制回填而非 fallback");
    let text = pair_text(heading);
    assert!(text.contains("$[0, 1]$"), "缺失槽位公式按序追加: {text}");
    assert!(text.contains("【坏译】"), "译文主体保留: {text}");
    // 正文无公式：正常翻译
    assert!(!result.pairs[1].fallback);
    assert!(pair_text(&result.pairs[1]).contains("【译】"));
    // 无 Error 事件（强制回填走 Done）
    assert!(
        events.iter().all(|e| e.status != ProgressStatus::Error),
        "强制回填不应有 Error 事件"
    );
}

#[tokio::test]
async fn merged_chunk_translation_one_request_for_short_paras() {
    // 4 个短段（各 ~50 字符）→ 1 次合并请求（+标题独立 1 次）= 2 次调用覆盖 5 段
    let doc = synth_doc(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let base = MockLlm {
        calls: calls.clone(),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: true,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache.clone(), 3).await;

    // 调用数：标题 1（独立）+ 正文 4 段合并为 1 → 共 2
    assert_eq!(calls.load(Ordering::SeqCst), 2, "短段应合并为一次请求");
    // 全部段落有译文且来自合并拆分（含【合并译】）
    let merged_translated = result
        .pairs
        .iter()
        .filter(|p| p.translated.is_some())
        .filter_map(|p| {
            p.translated.as_ref().and_then(|b| b.paragraph_content().cloned())
        })
        .flatten()
        .filter_map(|i| match i {
            InlineContent::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("|");
    assert!(merged_translated.contains("【合并译】段落 1"), "{merged_translated}");
    assert!(merged_translated.contains("【合并译】段落 4"), "{merged_translated}");
    // 进度：5 段全部 Done（无 Error）
    assert!(events.iter().all(|e| e.status != ProgressStatus::Error));
    let done = events.iter().filter(|e| matches!(e.status, ProgressStatus::Done)).count();
    assert_eq!(done, 5);

    // 二次翻译：段落级缓存全命中 → 0 次调用
    let before = calls.load(Ordering::SeqCst);
    let (_r2, events2) = run_translate(&base, &doc, cache.clone(), 3).await;
    assert_eq!(calls.load(Ordering::SeqCst), before, "缓存后零调用");
    assert_eq!(events2.iter().filter(|e| matches!(e.status, ProgressStatus::Cached)).count(), 5);
}

#[tokio::test]
async fn invariant_failed_paragraph_marked_fallback() {
    // 标题段触发 mock 500 → 管线不变量：不丢块、fallback 标记、译文回退原文（T1.2）
    let doc = synth_doc(3);
    let base = MockLlm {
        calls: Arc::new(AtomicUsize::new(0)),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: false,
        destroy_numbers: false,
        fail_on: Some("Synthetic Title".into()),
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache, 3).await;

    // 不变量 1：pairs 与 blocks 等长、逐位 block_id 相等（禁止任何路径静默丢弃）
    assert_eq!(result.pairs.len(), doc.blocks.len());
    for (pair, block) in result.pairs.iter().zip(doc.blocks.iter()) {
        assert_eq!(pair.block_id, block.id());
    }
    // 不变量 2：失败段 fallback=true 且译文为原文副本
    let failed = &result.pairs[0];
    assert!(failed.fallback, "失败段应标 fallback");
    assert_eq!(failed.translated.as_ref().unwrap(), &doc.blocks[0]);
    // 不变量 3：其余段 fallback=false 且有真译文
    for pair in &result.pairs[1..] {
        assert!(!pair.fallback);
        assert!(pair.translated.is_some());
    }
    // 进度：恰好 1 个 Error 事件
    assert_eq!(
        events.iter().filter(|e| matches!(e.status, ProgressStatus::Error)).count(),
        1
    );
    // 审计联动：失败段进 fallbacks 而非 missing
    let report = geminus_core::audit::alignment_audit(&doc, &result);
    assert!(report.missing.is_empty(), "{report:?}");
    assert_eq!(report.fallbacks, vec!["b_0000".to_string()]);
}

#[tokio::test]
async fn numbered_parse_failure_falls_back_to_per_paragraph() {
    // mock 对合并请求返回无编号文本（numbered=false 回声整个 prompt）→
    // 拆分失败 → 回退逐段翻译；标题独立 → 调用数 = 1 + 4
    let doc = synth_doc(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let base = MockLlm {
        calls: calls.clone(),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: false,
        destroy_numbers: true,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache.clone(), 3).await;

    // 1 次合并尝试（拆分失败）+ 4 次逐段回退 + 1 次标题独立 = 6
    assert_eq!(calls.load(Ordering::SeqCst), 6, "合并失败后逐段回退（含失败的合并尝试与标题）");
    let done = events.iter().filter(|e| matches!(e.status, ProgressStatus::Done)).count();
    assert_eq!(done, 5);
    // 全部有译文（回声带【译】前缀）
    assert_eq!(result.pairs.iter().filter(|p| p.translated.is_some()).count(), 5);
}

// ────────────────────────── v2.2 T4：算法块翻译管线 ──────────────────────────

use geminus_core::model::{AlgoLine, AlgoLineKind};

fn algo_doc() -> geminus_core::Document {
    let mut resources = std::collections::HashMap::new();
    resources.insert(
        "r_eq".to_string(),
        geminus_core::model::Resource::formula(String::new(), Some("V \\\\leftarrow x".to_string())),
    );
    let para = |id: &str, text: &str| Block::Paragraph {
        id: id.into(),
        style: Default::default(),
        content: vec![InlineContent::Text {
            text: text.into(),
            bold: false,
            italic: false,
            underline: false,
        }],
    };
    geminus_core::Document {
        id: "d1".into(),
        title: "algo doc".into(),
        blocks: vec![
            para("b_0", "Intro paragraph about the algorithm."),
            Block::Algorithm {
                id: "b_1".into(),
                header: String::new(),
                lines: vec![
                    AlgoLine {
                        kind: AlgoLineKind::Label,
                        segments: vec![t("Output: The loss")],
                    },
                    AlgoLine {
                        kind: AlgoLineKind::Stmt,
                        segments: vec![
                            t("Select class indices "),
                            InlineContent::InlineFormula { resource_id: "r_eq".into() },
                        ],
                    },
                    AlgoLine {
                        kind: AlgoLineKind::Comment,
                        segments: vec![t("Initialize loss")],
                    },
                ],
            },
        ],
        resources,
        render_meta: Default::default(),
        pdf_meta: None,
    }
}

fn t(s: &str) -> InlineContent {
    InlineContent::Text { text: s.into(), bold: false, italic: false, underline: false }
}

#[tokio::test]
async fn algorithm_block_translated_line_wise() {
    let doc = algo_doc();
    let calls = Arc::new(AtomicUsize::new(0));
    let base = MockLlm {
        calls: calls.clone(),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: true, // 段落合并响应
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: true,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache.clone(), 3).await;

    let algo_pair = result.pairs.iter().find(|p| p.block_id == "b_1").unwrap();
    assert!(!algo_pair.fallback, "算法块不应回退");
    let Block::Algorithm { lines, .. } = algo_pair.translated.as_ref().unwrap() else {
        panic!("译文侧应为算法块")
    };
    // Label 行原样（结构标签不译）
    assert!(matches!(&lines[0].segments[0],
        InlineContent::Text { text, .. } if text == "Output: The loss"));
    // Stmt/Comment 行已译（mock 回声【译】前缀）且公式令牌保位
    let stmt_text = lines[1]
        .segments
        .iter()
        .filter_map(|c| if let InlineContent::Text { text, .. } = c { Some(text.clone()) } else { None })
        .collect::<String>();
    assert!(stmt_text.contains("【译】"), "Stmt 行应被翻译：{stmt_text}");
    assert!(lines[1]
        .segments
        .iter()
        .any(|c| matches!(c, InlineContent::InlineFormula { resource_id } if resource_id == "r_eq")));
    assert!(matches!(&lines[2].segments[0],
        InlineContent::Text { text, .. } if text.contains("【译】")));
    // 进度含算法块的 Done
    assert!(events.iter().any(|e| e.block_id == "b_1" && matches!(e.status, ProgressStatus::Done)));

    // 二次翻译：算法块走缓存（0 新调用）
    let calls_before = calls.load(Ordering::SeqCst);
    let (result2, events2) = run_translate(&base, &doc, cache, 3).await;
    assert_eq!(calls.load(Ordering::SeqCst), calls_before, "二次应全缓存");
    assert!(events2.iter().any(|e| e.block_id == "b_1" && matches!(e.status, ProgressStatus::Cached)));
    let algo2 = result2.pairs.iter().find(|p| p.block_id == "b_1").unwrap();
    let Block::Algorithm { lines: l2, .. } = algo2.translated.as_ref().unwrap() else { panic!() };
    assert_eq!(l2.len(), lines.len(), "缓存回填行数一致");
}

#[tokio::test]
async fn algorithm_block_bad_response_falls_back() {
    // mock 不识别 [L n] 格式（回声全文无编号拆分）→ 整块回退原文 + fallback 标记
    let doc = algo_doc();
    let base = MockLlm {
        calls: Arc::new(AtomicUsize::new(0)),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: false,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache, 3).await;

    let algo_pair = result.pairs.iter().find(|p| p.block_id == "b_1").unwrap();
    assert!(algo_pair.fallback, "编号拆分失败应整块回退");
    assert_eq!(algo_pair.translated.as_ref().unwrap(), &doc.blocks[1], "回退=原文副本");
    assert!(events
        .iter()
        .any(|e| e.block_id == "b_1" && matches!(e.status, ProgressStatus::Error)));
    // 普通段不受影响
    assert!(result.pairs.iter().any(|p| p.block_id == "b_0" && !p.fallback));
}

// ────────────────────────── v2.2 T5：公式占位符化 ──────────────────────────

#[tokio::test]
async fn math_slot_formulas_restored_verbatim_and_cached() {
    // 含 $...$ / \(...\) 的段落：令牌化 → 回声 → 回填后公式逐位 == 原文；
    // 缓存走版本化键（第二次全 Cached、0 新调用）
    let para = |id: &str, text: &str| Block::Paragraph {
        id: id.into(),
        style: Default::default(),
        content: vec![InlineContent::Text {
            text: text.into(),
            bold: false,
            italic: false,
            underline: false,
        }],
    };
    let doc = geminus_core::Document {
        id: "d".into(),
        title: "math".into(),
        blocks: vec![para("b_0", "loss is $J(\\phi)$ and rate \\(r = 0.1\\) ok")],
        resources: Default::default(),
        render_meta: Default::default(),
        pdf_meta: None,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let base = MockLlm {
        calls: calls.clone(),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: false,
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, events) = run_translate(&base, &doc, cache.clone(), 3).await;

    let pair = &result.pairs[0];
    assert!(!pair.fallback);
    let text = pair_text(pair);
    // 公式逐位回填 == 原文源码（模型无机会改写）
    assert!(text.contains("$J(\\phi)$"), " Dollar 公式保位: {text}");
    assert!(text.contains("\\(r = 0.1\\)"), "圆括号公式保位: {text}");
    assert!(!text.contains("{{M"), "无令牌残留: {text}");
    // 发给模型的 user 内容不含 $ 定界符（公式不经过模型）
    // （mock 回声带【译】前缀 → text 以【译】开头即证明走了 echo）
    assert!(text.starts_with("【译】"));
    assert!(events.iter().any(|e| matches!(e.status, ProgressStatus::Done)));

    // 二次翻译：版本化缓存命中（0 新调用 + Cached）
    let before = calls.load(Ordering::SeqCst);
    let (_, events2) = run_translate(&base, &doc, cache, 3).await;
    assert_eq!(calls.load(Ordering::SeqCst), before, "二次应命中缓存");
    assert!(events2.iter().all(|e| matches!(e.status, ProgressStatus::Cached)));

    // 版本隔离：legacy 键读不到 math 管线译文
    // （get_versioned 与 get 的键不同——由 cache 单测覆盖，此处不再重复）
}

#[tokio::test]
async fn math_slot_chunk_merged_tokens_globally_offset() {
    // 两段各含 1 个公式 → 合并 chunk：第二段令牌全局偏移为 {{M1}}；
    // 编号拆分回填后两段公式各自正确还原
    let para = |id: &str, text: &str| Block::Paragraph {
        id: id.into(),
        style: Default::default(),
        content: vec![InlineContent::Text {
            text: text.into(),
            bold: false,
            italic: false,
            underline: false,
        }],
    };
    let doc = geminus_core::Document {
        id: "d".into(),
        title: "math".into(),
        blocks: vec![
            para("b_0", "first $alpha$ end"),
            para("b_1", "second $beta$ end"),
        ],
        resources: Default::default(),
        render_meta: Default::default(),
        pdf_meta: None,
    };
    let base = MockLlm {
        calls: Arc::new(AtomicUsize::new(0)),
        peak_concurrency: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        destroy_tokens: false,
        numbered: true, // 合并响应：回显编号
        destroy_numbers: false,
        fail_on: None,
        math_break: MathBreak::Off,
        heal_counts: Default::default(),
        algo_lines: false,
    }
    .spawn()
    .await;
    let cache = Arc::new(TranslationCache::open_in_memory().unwrap());
    let (result, _) = run_translate(&base, &doc, cache, 3).await;

    let t0 = pair_text(&result.pairs[0]);
    let t1 = pair_text(&result.pairs[1]);
    assert!(t0.contains("$alpha$"), "{t0}");
    assert!(t1.contains("$beta$"), "{t1}");
    assert!(!t0.contains("{{M") && !t1.contains("{{M"));
}
