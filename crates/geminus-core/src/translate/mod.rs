//! 翻译引擎（plan.md §4.2）：prompt 构建 / LLM 客户端 / 译文回填 / 编排

pub mod algorithm;
pub mod chunk;
pub mod llm;
pub mod math_guard;
pub mod math_slot;
pub mod prompt;
pub mod rebuild;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;

use crate::cache::TranslationCache;
use crate::model::{
    Block, Document, ProgressStatus, TranslatedBlock, TranslatedDocument, TranslateProgress,
};

pub use llm::{LlmClient, TranslateError};
pub use prompt::{extract_source_text, PlaceholderRef};

/// 一个待翻译段落任务
struct Task {
    idx: usize,
    block_id: String,
    /// 令牌化源文本（{{Pn}} 资产令牌 + v2.2 T5 的 {{Mn}} 公式令牌）
    source_text: String,
    refs: Vec<PlaceholderRef>,
    /// v2.2 T5：被提取的行内公式原文（回填用；空 = 无公式）
    math_slots: Vec<String>,
    orig: Block,
    /// v2.2 T4：算法块任务（行级 [L n] 翻译路径）
    is_algorithm: bool,
}

impl Task {
    /// 含公式段落使用 math 管线版本化的缓存键（v2.2 T5.3：旧的被污染译文自动失效）
    fn cache_version(&self) -> Option<&'static str> {
        if self.math_slots.is_empty() {
            None
        } else {
            Some(crate::cache::MATH_PIPELINE_VERSION)
        }
    }
}

/// 翻译整个文档（docs/design.md §5.2 编排）：
/// 1. Paragraph 块提取源文本（行内资产令牌化）
/// 2. 查缓存 → 命中直接回填（Cached 进度）
/// 3. 未命中任务经 Semaphore 并发调 LLM → 写缓存 → 回填（Done/Error 进度）
/// 4. 占位符/空段 translated=None；失败段译文侧回退原文副本
///
/// 单段失败不中断整体；进度 current 单调递增至 total（可翻译段数）。
pub async fn translate_document(
    doc: &Document,
    client: Arc<LlmClient>,
    cache: Arc<TranslationCache>,
    progress: mpsc::Sender<TranslateProgress>,
    concurrency: usize,
    source_lang: &str,
    target_lang: &str,
) -> TranslatedDocument {
    // 1. 收集可翻译任务（块下标 → 源文本）
    let mut tasks = Vec::new();
    for (idx, block) in doc.blocks.iter().enumerate() {
        if let Block::Paragraph { id, content, .. } = block {
            let (source_text, refs) = prompt::extract_source_text(content);
            // v2.2 T5：行内公式占位符化——公式不经过模型，回填用原文源码逐位替换
            let (source_text, math_slots) = math_slot::extract(&source_text);
            if source_text.trim().is_empty() {
                continue; // 无可见文本：不进任务，组装时回退原文
            }
            tasks.push(Task {
                idx,
                block_id: id.clone(),
                source_text,
                refs,
                math_slots,
                orig: block.clone(),
                is_algorithm: false,
            });
        }
        if let Block::Algorithm { .. } = block {
            // v2.2 T4：算法块按行收集（Label 行不译）；无可译行则跳过
            let lines = algorithm::extract_translatable_lines(block);
            if lines.is_empty() {
                continue;
            }
            let source_text =
                lines.iter().map(|l| l.source.clone()).collect::<Vec<_>>().join("\n");
            tasks.push(Task {
                idx,
                block_id: block.id().to_string(),
                source_text,
                refs: Vec::new(),
                math_slots: Vec::new(),
                orig: block.clone(),
                is_algorithm: true,
            });
        }
    }
    let total = tasks.len() as u32;

    let results: Arc<std::sync::Mutex<HashMap<usize, Block>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let fallback_idxs: Arc<std::sync::Mutex<std::collections::HashSet<usize>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let completed = Arc::new(AtomicU32::new(0));

    let mut pending: Vec<Task> = Vec::new();
    for task in tasks {
        // 2. 缓存（算法块：缓存值 = 行译文 \n 拼接，行数对齐才回填，否则当 miss）
        if task.is_algorithm {
            if let Some(cached) = cache.get(&task.source_text, source_lang, target_lang) {
                let lines = algorithm::extract_translatable_lines(&task.orig);
                let parts: Vec<String> = cached.split('\n').map(str::to_string).collect();
                let mut block = task.orig.clone();
                if parts.len() == lines.len()
                    && algorithm::rebuild_algorithm_lines(&mut block, &lines, &parts)
                {
                    results.lock().unwrap().insert(task.idx, block);
                    let current = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = progress
                        .send(TranslateProgress {
                            block_id: task.block_id.clone(),
                            current,
                            total,
                            status: ProgressStatus::Cached,
                            translated_text: Some(cached),
                            error: None,
                        })
                        .await;
                    continue;
                }
            }
            pending.push(task);
            continue;
        }
        let cached = match task.cache_version() {
            Some(v) => cache.get_versioned(&task.source_text, source_lang, target_lang, v),
            None => cache.get(&task.source_text, source_lang, target_lang),
        };
        if let Some(cached) = cached {
            let (block, _) = rebuild::rebuild_paragraph(&task.orig, &cached, &task.refs);
            results.lock().unwrap().insert(task.idx, block);
            let current = completed.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = progress
                .send(TranslateProgress {
                    block_id: task.block_id,
                    current,
                    total,
                    status: ProgressStatus::Cached,
                    translated_text: Some(cached),
                    error: None,
                })
                .await;
        } else {
            pending.push(task);
        }
    }

    // 3. 并发调 LLM：短段合并为编号 chunk 一次请求（v2 §3.5）；
    //    标题/超长段独立；合并响应编号解析失败或请求失败 → 逐段回退。
    let chunk_cfg = chunk::ChunkConfig::from_env();
    let groups = group_tasks(&pending, &chunk_cfg);

    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut join_set: JoinSet<()> = JoinSet::new();

    let mut pending: Vec<Option<Task>> = pending.into_iter().map(Some).collect();
    for group in groups {
        let tasks: Vec<Task> = group.into_iter().map(|i| pending[i].take().expect("组内下标唯一")).collect();
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("信号量未关闭");
        let client = client.clone();
        let cache = cache.clone();
        let results = results.clone();
        let fallback_idxs = fallback_idxs.clone();
        let completed = completed.clone();
        let progress = progress.clone();
        let source_lang = source_lang.to_string();
        let target_lang = target_lang.to_string();
        let ctx = TaskCtx {
            client: client.clone(),
            cache: cache.clone(),
            results: results.clone(),
            fallback_idxs: fallback_idxs.clone(),
            completed: completed.clone(),
            progress: progress.clone(),
            total,
            source_lang: source_lang.clone(),
            target_lang: target_lang.clone(),
        };
        join_set.spawn(async move {
            let _permit = permit;
            if tasks.len() == 1 && tasks[0].is_algorithm {
                translate_algorithm_task(&tasks[0], &ctx).await;
                return;
            }
            if tasks.len() == 1 {
                translate_one(&tasks[0], &ctx).await;
                return;
            }
            // 合并请求（v2.2 T5：组内 M 令牌全局偏移编号，避免跨段冲突）
            let mut offset = 0usize;
            let texts: Vec<String> = tasks
                .iter()
                .map(|t| {
                    let s = math_slot::renumber_tokens(&t.source_text, offset);
                    offset += t.math_slots.len();
                    s
                })
                .collect();
            let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            let prompt = chunk::build_merged_prompt(&refs, &ctx.source_lang, &ctx.target_lang);
            let merged_ok = match ctx.client.translate_raw(&prompt).await {
                Ok(resp) => chunk::split_numbered_response(&resp, tasks.len()),
                Err(_) => None,
            };
            match merged_ok {
                Some(parts) => {
                    // 拆分成功后按各段原始偏移校验/回填 M 令牌
                    let mut offset = 0usize;
                    for (task, translated) in tasks.iter().zip(parts) {
                        let n_slots = task.math_slots.len();
                        let renumbered_src = math_slot::renumber_tokens(&task.source_text, offset);
                        let restored = if n_slots == 0 {
                            translated
                        } else if math_slot::verify(&renumbered_src, &translated) {
                            math_slot::restore(&translated, &math_slot::padded_slots(&task.math_slots, offset))
                        } else {
                            String::new() // 令牌损坏标记 → 独立重译
                        };
                        offset += n_slots;
                        if n_slots == 0 || !restored.is_empty() {
                            finish_task(task, restored, &ctx).await;
                        } else {
                            eprintln!("[math_slot] chunk 内令牌损坏，独立重译: {}", task.block_id);
                            translate_one(task, &ctx).await;
                        }
                    }
                }
                None => {
                    // 回退：逐段独立翻译（v1 路径）
                    for task in &tasks {
                        translate_one(task, &ctx).await;
                    }
                }
            }
        });
    }
    while join_set.join_next().await.is_some() {}

    // 4. 组装（不变量：pairs.len() == blocks.len()，失败段标 fallback 且译文侧回退原文）
    let results = results.lock().unwrap();
    let fallback_idxs = fallback_idxs.lock().unwrap();
    let pairs = doc
        .blocks
        .iter()
        .enumerate()
        .map(|(idx, block)| TranslatedBlock {
            block_id: block.id().to_string(),
            translated: match block {
                Block::Paragraph { .. } | Block::Algorithm { .. } => {
                    Some(results.get(&idx).cloned().unwrap_or_else(|| block.clone()))
                }
                _ => None, // 占位符/空段：前端复用 original 渲染
            },
            original: block.clone(),
            fallback: fallback_idxs.contains(&idx),
        })
        .collect();

    TranslatedDocument {
        doc_id: doc.id.clone(),
        pairs,
    }
}

/// 任务分组（v2 §3.5）：标题独立、超 max_chars 独立、连续正文按阈值合并。
/// 返回按原任务下标組成的组序列（组内下标升序）。
fn group_tasks(tasks: &[Task], cfg: &chunk::ChunkConfig) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let mut acc = 0usize;
    for (i, t) in tasks.iter().enumerate() {
        let is_heading = matches!(
            &t.orig,
            Block::Paragraph { style, .. } if style.heading_level.is_some()
        );
        // v2.2 T4：算法块行级 [L n] 翻译，永不与段落合并 chunk
        let is_algo = t.is_algorithm;
        let n = t.source_text.chars().count();
        if is_heading || is_algo || n > cfg.max_chars {
            if !run.is_empty() {
                groups.push(std::mem::take(&mut run));
                acc = 0;
            }
            groups.push(vec![i]);
            continue;
        }
        run.push(i);
        acc += n;
        if acc >= cfg.min_chars || run.len() >= cfg.max_paras {
            groups.push(std::mem::take(&mut run));
            acc = 0;
        }
    }
    if !run.is_empty() {
        groups.push(run);
    }
    groups
}

/// 组执行共享的上下文（收拢参数）
struct TaskCtx {
    client: Arc<LlmClient>,
    cache: Arc<TranslationCache>,
    results: Arc<std::sync::Mutex<HashMap<usize, Block>>>,
    /// 翻译失败回退原文的块下标（组装时标 fallback）
    fallback_idxs: Arc<std::sync::Mutex<std::collections::HashSet<usize>>>,
    completed: Arc<AtomicU32>,
    progress: mpsc::Sender<TranslateProgress>,
    total: u32,
    source_lang: String,
    target_lang: String,
}

/// 算法块任务（v2.2 T4.3）：行级 [L n] 编号请求 → 拆分 → 回填；
/// 拆分或回填失败 → 整块回退原文（fallback=true）。
async fn translate_algorithm_task(task: &Task, ctx: &TaskCtx) {
    let _ = ctx
        .progress
        .send(TranslateProgress {
            block_id: task.block_id.clone(),
            current: ctx.completed.load(Ordering::SeqCst),
            total: ctx.total,
            status: ProgressStatus::Translating,
            translated_text: None,
            error: None,
        })
        .await;
    let lines = algorithm::extract_translatable_lines(&task.orig);
    let srcs: Vec<&str> = lines.iter().map(|l| l.source.as_str()).collect();
    let prompt = algorithm::build_algorithm_prompt(&srcs, &ctx.source_lang, &ctx.target_lang);
    let outcome = match ctx.client.translate_raw(&prompt).await {
        Ok(resp) => algorithm::split_l_numbered(&resp, lines.len()),
        Err(_) => None,
    };
    match outcome {
        Some(parts) => {
            let mut block = task.orig.clone();
            if algorithm::rebuild_algorithm_lines(&mut block, &lines, &parts) {
                let joined = parts.join("\n");
                let _ = ctx.cache.put(
                    &task.source_text,
                    &ctx.source_lang,
                    &ctx.target_lang,
                    &joined,
                    &ctx.client.config().model,
                );
                ctx.results.lock().unwrap().insert(task.idx, block);
                let current = ctx.completed.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = ctx
                    .progress
                    .send(TranslateProgress {
                        block_id: task.block_id.clone(),
                        current,
                        total: ctx.total,
                        status: ProgressStatus::Done,
                        translated_text: Some(joined),
                        error: None,
                    })
                    .await;
                return;
            }
            eprintln!("[algorithm] 行回填失败，回退原文: {}", task.block_id);
        }
        None => eprintln!("[algorithm] 编号拆分失败，回退原文: {}", task.block_id),
    }
    ctx.results.lock().unwrap().insert(task.idx, task.orig.clone());
    ctx.fallback_idxs.lock().unwrap().insert(task.idx);
    let current = ctx.completed.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = ctx
        .progress
        .send(TranslateProgress {
            block_id: task.block_id.clone(),
            current,
            total: ctx.total,
            status: ProgressStatus::Error,
            translated_text: None,
            error: Some("算法块翻译失败，已回退原文".to_string()),
        })
        .await;
}

/// 单段独立翻译（v1 路径，同时是合并失败/配平失败的回退路径）。
/// 行内公式配平防线（v2.1 T4.3）：译文公式计数不等 → 带强调重试 1 次 → 仍不等回退原文。
async fn translate_one(task: &Task, ctx: &TaskCtx) {
    let current_before = ctx.completed.load(Ordering::SeqCst);
    let _ = ctx
        .progress
        .send(TranslateProgress {
            block_id: task.block_id.clone(),
            current: current_before,
            total: ctx.total,
            status: ProgressStatus::Translating,
            translated_text: None,
            error: None,
        })
        .await;
    let first = ctx
        .client
        .translate_text(&task.source_text, &ctx.source_lang, &ctx.target_lang)
        .await;
    // v2.2 T5 + v2.3 B4：M 令牌集合校验 → 失败重试 1 次 → 仍失败择优强制回填。
    // 令牌纯 ASCII 天然免疫回声污染；强制回填保公式内容（缺失项按序追加），
    // 整段回退仅剩 LLM 调用本身失败一条路
    let outcome: Option<String> = match first {
        Ok(t) if math_slot::verify(&task.source_text, &t) => Some(t),
        Ok(t) => {
            eprintln!("[math_slot] 令牌校验失败，重试: {}", task.block_id);
            let retry = ctx
                .client
                .translate_text(&task.source_text, &ctx.source_lang, &ctx.target_lang)
                .await;
            match retry {
                Ok(t2) if math_slot::verify(&task.source_text, &t2) => Some(t2),
                Ok(t2) => {
                    // 择令牌命中数更高的候选（并列取重试响应）
                    let best = if math_slot::present_tokens(&task.source_text, &t)
                        > math_slot::present_tokens(&task.source_text, &t2)
                    {
                        t
                    } else {
                        t2
                    };
                    let (restored, missing) = math_slot::force_restore(&best, &task.math_slots);
                    eprintln!(
                        "[math_slot][warn] 强制回填 {}: missing_slots={:?}",
                        task.block_id, missing
                    );
                    Some(restored)
                }
                Err(_) => None,
            }
        }
        Err(_) => None,
    };
    match outcome {
        Some(translated) => {
            // M 令牌回填：verify 通过路径用原文公式源码逐位替换；
            // 强制回填路径已在上一步完成（模型无机会改写公式）
            let restored = if translated.contains("{{M") {
                math_slot::restore(&translated, &task.math_slots)
            } else {
                translated
            };
            finish_task(task, restored, ctx).await;
        }
        None => {
            ctx.results.lock().unwrap().insert(task.idx, task.orig.clone());
            ctx.fallback_idxs.lock().unwrap().insert(task.idx);
            let current = ctx.completed.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = ctx
                .progress
                .send(TranslateProgress {
                    block_id: task.block_id.clone(),
                    current,
                    total: ctx.total,
                    status: ProgressStatus::Error,
                    translated_text: None,
                    error: Some("翻译调用失败，已回退原文".to_string()),
                })
                .await;
        }
    }
}

/// 翻译成功后的统一收尾：缓存写入（段落级键；含公式段落版本化）+ 回填 + Done 进度。
/// translated 应已完成 M 令牌回填（原文公式源码）——此处 math_guard 配平仅作二级告警日志。
async fn finish_task(task: &Task, translated: String, ctx: &TaskCtx) {
    // 二级告警：restore 后理论上无 $ 定界符残留差异（回填即原文），不等只记日志
    let original_math = task
        .math_slots
        .iter()
        .map(|m| math_guard::count_inline_math(m))
        .sum::<usize>();
    let translated_math = math_guard::count_inline_math(&translated);
    if original_math != translated_math {
        eprintln!(
            "[math_guard][warn] 二级配平告警 {}: slots={original_math} restored={translated_math}",
            task.block_id
        );
    }
    let _ = match task.cache_version() {
        Some(v) => ctx.cache.put_versioned(
            &task.source_text,
            &ctx.source_lang,
            &ctx.target_lang,
            &translated,
            &ctx.client.config().model,
            v,
        ),
        None => ctx.cache.put(
            &task.source_text,
            &ctx.source_lang,
            &ctx.target_lang,
            &translated,
            &ctx.client.config().model,
        ),
    };
    let (block, _degraded) = rebuild::rebuild_paragraph(&task.orig, &translated, &task.refs);
    ctx.results.lock().unwrap().insert(task.idx, block);
    let current = ctx.completed.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = ctx
        .progress
        .send(TranslateProgress {
            block_id: task.block_id.clone(),
            current,
            total: ctx.total,
            status: ProgressStatus::Done,
            translated_text: Some(translated),
            error: None,
        })
        .await;
}
