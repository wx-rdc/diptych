//! MinerU content_list.json + layout.json → Document IR（docs/mineru-output-spec.md §三/四）。
//!
//! 数据源选择：content_list.json（阅读顺序 + text_level + LaTeX/HTML/img_path + 归一化 bbox）
//! + layout.json（仅取每页 page_size，真实 PDF 点尺寸）。bbox 还原：宽高各自除以 1000。

use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

use crate::model::{Block, Document, InlineContent, ParagraphStyle, PdfMeta, RenderMeta, Resource};

/// 内容型类型（映射进 IR；v2.2 T1 类型注册表，与 raw.rs 的逻辑类型对齐）
pub const CONTENT_TYPES: &[&str] = &[
    "text", "title", "image", "table", "equation", "chart",
    "figure_caption", "table_caption", "algorithm", "list", "equation_interline",
];
/// 装饰型类型（策略性排除 → Skipped{Decoration}）。
/// v2.3：补 page_header/page_footer——v2 管线对书眉/页脚的命名与 v1 的
/// header/footer 不同，缺失曾致 IEEE 夹具 47 个 Unmapped 违规 + char_coverage 稀释
pub const DECOR_TYPES: &[&str] = &[
    "header", "footer", "page_number", "page_footnote", "page_aside_text", "aside_text", "index",
    "page_header", "page_footer",
];

#[derive(Debug, Error)]
pub enum AdaptError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 解析错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("资源缺失: {0}")]
    MissingResource(String),
}

/// content_list.json 条目（serde 宽松：未知字段忽略）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ContentItem {
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub text_level: Option<u8>,
    #[serde(default)]
    pub img_path: String,
    #[serde(default)]
    pub table_body: String,
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    #[serde(default)]
    pub page_idx: usize,
}

/// 定位 content_list.json：真实 ZIP 内带 UUID 前缀（{uuid}_content_list.json），
/// 夹具/测试目录可能是精确名。返回文件路径。
pub fn find_content_list(dir: &Path) -> Result<std::path::PathBuf, AdaptError> {
    let exact = dir.join("content_list.json");
    if exact.exists() {
        return Ok(exact);
    }
    let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("_content_list.json"))
        })
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| AdaptError::MissingResource("content_list.json 不存在".to_string()))
}

/// 剥离公式文本的 $$ 围栏（MinerU 输出 "$$\n...\n$$"）
fn strip_formula_fence(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix("$$").unwrap_or(t);
    let t = t.strip_suffix("$$").unwrap_or(t);
    t.trim().to_string()
}

/// ── MinerU 伪上/下标噪声清洗（feedback_0913：14_EffTrans_Mamba 全篇伪 `<sub>`）──
///
/// MinerU VLM 对部分 TeX 字体文档（Linux Libertine 等）把正文普通字母识别成
/// 上/下标（`Se<sub>qu</sub>ence` 式），前端白名单如实渲染 → 大量字符变小下沉。
/// 合法标记（作者脚注 `<sup>1</sup>`、化学式 `<sub>2</sub>`）内容多为单字符，
/// 伪标签内容是普通文本片段，据此分级判定后去包裹（剥标签保内容）：
/// - 条目级：长内容（≥2 字符）标签 ≥3 个，或被包裹字符占比 ≥8% → 该条目去包裹；
///   （单字符标签的包裹占比上限 ~7.7%，纯脚注标记天然不触发占比信号）
/// - 文档级：噪声条目占文本条目 ≥10% → 整文档去包裹（零散残留与合法标记一并放弃，
///   病态文档中保文本远比保上下标样式重要）。
const SCRIPT_NOISE_LONG_TAGS: usize = 3;
const SCRIPT_NOISE_CHAR_RATIO: f64 = 0.08;
const SCRIPT_DOC_NOISE_PERCENT: usize = 10;

/// `<sub>/<sup>` 标签统计 → (长内容标签数, 被包裹字符数)；未闭合开标签跳过
fn script_tag_stats(text: &str) -> (usize, usize) {
    let mut long_tags = 0;
    let mut tagged_chars = 0;
    let mut rest = text;
    loop {
        let pos = match (rest.find("<sub>"), rest.find("<sup>")) {
            (None, None) => break,
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
        };
        let (open, close) = if rest[pos..].starts_with("<sub>") { ("<sub>", "</sub>") } else { ("<sup>", "</sup>") };
        let after = &rest[pos + open.len()..];
        let Some(end) = after.find(close) else {
            rest = after;
            continue;
        };
        let n = after[..end].trim().chars().count();
        if n >= 2 {
            long_tags += 1;
        }
        tagged_chars += n;
        rest = &after[end + close.len()..];
    }
    (long_tags, tagged_chars)
}

/// 条目级伪上/下标噪声判定
fn is_script_noise(text: &str) -> bool {
    let (long_tags, tagged_chars) = script_tag_stats(text);
    long_tags >= SCRIPT_NOISE_LONG_TAGS
        || tagged_chars as f64 / text.chars().count().max(1) as f64 >= SCRIPT_NOISE_CHAR_RATIO
}

/// 剥除全部 `<sub>/<sup>` 开闭标签（保留内容；未闭合开标签一并剥除）
fn strip_script_tags(text: &str) -> String {
    text.replace("<sub>", "").replace("</sub>", "").replace("<sup>", "").replace("</sup>", "")
}

/// 文档级升级判定：噪声条目占文本条目 ≥10%
fn is_doc_script_noise<S: AsRef<str>>(texts: &[S]) -> bool {
    let noisy = texts.iter().filter(|t| is_script_noise(t.as_ref())).count();
    !texts.is_empty() && noisy * SCRIPT_DOC_NOISE_PERCENT >= texts.len()
}

/// 条目清洗：文档级升级或条目自身噪声时去包裹
fn sanitize_script_noise(text: &str, doc_noisy: bool) -> String {
    if doc_noisy || is_script_noise(text) { strip_script_tags(text) } else { text.to_string() }
}

/// 按扩展名推断 MIME
fn mime_of(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpeg") || lower.ends_with(".jpg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "application/octet-stream"
    }
}

/// 读取 zip 解压目录中的图片为 base64 资源；缺失返回 None（调用方降级）
fn read_image_resource(dir: &Path, img_path: &str) -> Option<Resource> {
    if img_path.is_empty() {
        return None;
    }
    // img_path 形如 "images/xxx.jpg"（也可能带 ./ 前缀）
    let rel = img_path.trim_start_matches("./");
    let full = dir.join(rel);
    let bytes = std::fs::read(&full).ok()?;
    use base64::Engine as _;
    Some(Resource::image(mime_of(rel), base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// 解析 layout.json 的每页尺寸（page_idx → [宽, 高]，PDF 点）。
/// 供 v1/v2 适配与 raw 审计加载共用。
pub fn load_page_sizes(dir: &Path) -> HashMap<usize, [f64; 2]> {
    let lay: serde_json::Value = match std::fs::File::open(dir.join("layout.json")) {
        Ok(f) => serde_json::from_reader(f).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };
    lay.get("pdf_info")
        .and_then(|v| v.as_array())
        .map(|pages| {
            pages
                .iter()
                .filter_map(|p| {
                    let idx = p.get("page_idx").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let size = p.get("page_size").and_then(|v| v.as_array())?;
                    let w = size.first().and_then(|v| v.as_f64())?;
                    let h = size.get(1).and_then(|v| v.as_f64())?;
                    Some((idx, [w, h]))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 解析 MinerU 输出目录 → IR。
/// `page_offset`：该片段首页在原文件中的页号（单片传 0；拆分合并时用于全局页号）。
/// `fallback_title`：无标题时的文档标题（通常为文件名主干）。
///
/// v2.2 T3：存在 `*_content_list_v2.json` 时走 v2 数据源（题注内嵌/结构化行内公式/
/// algorithm/chart/title 独立类型）；否则回退 v1 content_list.json。
pub fn adapt_dir(dir: &Path, page_offset: usize, fallback_title: &str) -> Result<Document, AdaptError> {
    if let Some(v2_path) = crate::mineru::raw::find_v2_path(dir) {
        return adapt_v2(dir, &v2_path, page_offset, fallback_title);
    }
    adapt_v1(dir, page_offset, fallback_title)
}

/// v1 数据源（content_list.json 扁平格式）——既有行为，题注等内嵌信息缺失是格式盲区
fn adapt_v1(dir: &Path, page_offset: usize, fallback_title: &str) -> Result<Document, AdaptError> {
    let items: Vec<ContentItem> =
        serde_json::from_reader(std::fs::File::open(find_content_list(dir)?)?)?;
    let layout = load_page_sizes(dir);

    // 伪上/下标噪声文档级判定（条目级清洗在逐条映射时做）
    let doc_noisy = is_doc_script_noise(
        &items
            .iter()
            .filter(|i| matches!(i.item_type.as_str(), "text" | "ref_text"))
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
    );

    let page_count = items.iter().map(|i| i.page_idx + 1).max().unwrap_or(1);
    let page_size_at = |local_idx: usize| -> [f64; 2] {
        layout.get(&local_idx).copied().unwrap_or([612.0, 792.0])
    };

    let mut blocks: Vec<Block> = Vec::new();
    let mut resources: HashMap<String, Resource> = HashMap::new();
    let mut render_meta: HashMap<String, RenderMeta> = HashMap::new();
    let mut first_heading: Option<String> = None;

    for (i, item) in items.into_iter().enumerate() {
        let id = format!("b_{i:04}");
        let global_page = page_offset + item.page_idx;
        let size = page_size_at(item.page_idx);
        if let Some(bb) = item.bbox {
            render_meta.insert(
                id.clone(),
                RenderMeta {
                    page_idx: global_page,
                    bbox: [
                        bb[0] / 1000.0 * size[0],
                        bb[1] / 1000.0 * size[1],
                        bb[2] / 1000.0 * size[0],
                        bb[3] / 1000.0 * size[1],
                    ],
                },
            );
        }

        match item.item_type.as_str() {
            "text" => {
                let text = sanitize_script_noise(&item.text, doc_noisy);
                let style = ParagraphStyle {
                    heading_level: item.text_level,
                    ..Default::default()
                };
                if item.text_level.is_some() && first_heading.is_none() {
                    first_heading = Some(text.trim().to_string());
                }
                blocks.push(Block::Paragraph {
                    id,
                    style,
                    content: vec![InlineContent::Text {
                        text,
                        bold: false,
                        italic: false,
                        underline: false,
                    }],
                });
            }
            "equation" => {
                let latex = strip_formula_fence(&item.text);
                let rid = format!("eq_{i:04}");
                resources.insert(rid.clone(), Resource::formula(String::new(), Some(latex)));
                blocks.push(Block::Formula { id, resource_id: rid, omml: String::new() });
            }
            "table" => {
                let rid = format!("tbl_{i:04}");
                // 表格截图（img_path）额外入池供前端展示原图
                if let Some(img) = read_image_resource(dir, &item.img_path) {
                    resources.insert(format!("{rid}_shot"), img);
                }
                let preview = crate::model::Resource::table(&item.table_body);
                let preview_text: String = item
                    .table_body
                    .replace("<td>", " ")
                    .replace("</td>", "")
                    .replace("<tr>", "")
                    .replace("</tr>", " ")
                    .replace("<table>", "")
                    .replace("</table>", "")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                resources.insert(rid.clone(), preview);
                blocks.push(Block::Table {
                    id,
                    resource_id: rid,
                    preview_text: Some(preview_text.chars().take(120).collect()),
                });
            }
            "image" => {
                let rid = format!("img_{i:04}");
                match read_image_resource(dir, &item.img_path) {
                    Some(res) => {
                        resources.insert(rid.clone(), res);
                        let (w, h) = match item.bbox {
                            Some(bb) => (
                                Some((bb[2] - bb[0]).max(0.0) as u32),
                                Some((bb[3] - bb[1]).max(0.0) as u32),
                            ),
                            None => (None, None),
                        };
                        blocks.push(Block::Image { id, resource_id: rid, width: w, height: h, alt_text: None });
                    }
                    None => {
                        // 图片文件缺失：降级为空块保位（与 v1 图片降级策略一致）
                        blocks.push(Block::Empty { id });
                    }
                }
            }
            "ref_text" => {
                // v2.3 B3：v1 参考文献条目（"[1] Author..." 形态）→ 列表段落。
                // 线上 v2.1 曾静默丢弃 → IEEE 文档 35 条文献消失（feedback_0910 P1-B3）
                blocks.push(Block::Paragraph {
                    id,
                    style: ParagraphStyle { is_list_item: true, ..Default::default() },
                    content: vec![InlineContent::Text {
                        text: sanitize_script_noise(&item.text, doc_noisy),
                        bold: false,
                        italic: false,
                        underline: false,
                    }],
                });
            }
            other => {
                // v2.2 T1 无静默丢弃：未列出类型一律显式 Skipped 落账（存在于 IR 与审计统计）
                let reason = if DECOR_TYPES.contains(&other) {
                    crate::model::SkipReason::Decoration
                } else {
                    crate::model::SkipReason::Unmapped { type_name: other.to_string() }
                };
                blocks.push(Block::Skipped {
                    id,
                    source_type: other.to_string(),
                    reason,
                });
            }
        }
    }

    let page_sizes: Vec<[f64; 2]> = (0..page_count).map(page_size_at).collect();
    Ok(Document {
        id: uuid::Uuid::new_v4().to_string(),
        title: first_heading.filter(|t| !t.is_empty()).unwrap_or_else(|| fallback_title.to_string()),
        blocks,
        resources,
        render_meta,
        pdf_meta: Some(PdfMeta { page_count, page_sizes }),
    })
}


// ────────────────────────── v2.2 T3：v2 数据源适配 ──────────────────────────

fn str_of(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// v2 段数组 → InlineContent 序列。
/// `type=="text"` → Text；`equation_inline` → 独立公式资源 `eqp_{n}` + InlineFormula；
/// `image_inline` → 图片资源 + InlineImage。返回 (纯文本, segments)。
fn segs_to_inline(
    segs: Option<&serde_json::Value>,
    dir: &Path,
    resources: &mut HashMap<String, Resource>,
    counter: &mut usize,
    doc_noisy: bool,
) -> (String, Vec<InlineContent>) {
    let mut text = String::new();
    let mut out = Vec::new();
    for seg in segs.and_then(|v| v.as_array()).into_iter().flatten() {
        match str_of(&seg["type"]).as_str() {
            "text" => {
                let t = sanitize_script_noise(&str_of(&seg["content"]), doc_noisy);
                text.push_str(&t);
                out.push(InlineContent::Text { text: t, bold: false, italic: false, underline: false });
            }
            "equation_inline" => {
                let rid = format!("eqp_{counter:04}");
                *counter += 1;
                resources.insert(
                    rid.clone(),
                    Resource::formula(String::new(), Some(str_of(&seg["content"]))),
                );
                out.push(InlineContent::InlineFormula { resource_id: rid });
            }
            "image_inline" => {
                if let Some(res) = read_image_resource(dir, &str_of(&seg["img_path"])) {
                    let rid = format!("imi_{counter:04}");
                    *counter += 1;
                    resources.insert(rid.clone(), res);
                    out.push(InlineContent::InlineImage { resource_id: rid });
                }
            }
            _ => {}
        }
    }
    (text, out)
}

/// v2 内容字段里 text 段文本收集（伪上/下标文档级判定用）。
/// 覆盖全部已知 `*_content` / `*_caption` 文本段字段。
fn collect_v2_text_segs<'a>(content: Option<&'a serde_json::Value>, out: &mut Vec<&'a str>) {
    const SEG_KEYS: [&str; 7] = [
        "title_content",
        "paragraph_content",
        "item_content",
        "image_caption",
        "chart_caption",
        "table_caption",
        "algorithm_content",
    ];
    let Some(content) = content else { return };
    for key in SEG_KEYS {
        for seg in content.get(key).and_then(|v| v.as_array()).into_iter().flatten() {
            if seg.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = seg.get("content").and_then(|v| v.as_str()) {
                    out.push(t);
                }
            }
        }
    }
}

/// v2 bbox（1000 归一化）→ PDF 点
fn json_bbox(b: Option<&serde_json::Value>, size: [f64; 2]) -> Option<[f64; 4]> {
    let a = b?.as_array()?;
    let n = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
    Some([
        n(0) / 1000.0 * size[0],
        n(1) / 1000.0 * size[1],
        n(2) / 1000.0 * size[0],
        n(3) / 1000.0 * size[1],
    ])
}

/// v2 数据源适配（content_list_v2.json 分页数组）。映射表：
/// title→heading 段 / paragraph→段（equation_inline 独立资源）/ image·chart→Image+
/// 内嵌题注段 / table→Table+题注段 / equation_interline→Formula / list→逐条段落 /
/// algorithm→T3 先 Skipped（T4 实装）/ 装饰类型→Skipped{Decoration}
fn adapt_v2(
    dir: &Path,
    v2_path: &Path,
    page_offset: usize,
    fallback_title: &str,
) -> Result<Document, AdaptError> {
    let pages: serde_json::Value = serde_json::from_reader(std::fs::File::open(v2_path)?)?;
    let layout = load_page_sizes(dir);
    // 伪上/下标噪声文档级判定（条目级清洗在段转换时做）
    let doc_noisy = {
        let mut texts: Vec<&str> = Vec::new();
        for page_blocks in pages.as_array().into_iter().flatten() {
            for b in page_blocks.as_array().into_iter().flatten() {
                collect_v2_text_segs(b.get("content"), &mut texts);
            }
        }
        is_doc_script_noise(&texts)
    };
    let mut blocks: Vec<Block> = Vec::new();
    let mut resources: HashMap<String, Resource> = HashMap::new();
    let mut render_meta: HashMap<String, RenderMeta> = HashMap::new();
    let mut first_heading: Option<String> = None;
    let mut counter = 0usize;
    let mut page_count = 0usize;

    let push_meta = |blocks_len: &mut usize,
                     render_meta: &mut HashMap<String, RenderMeta>,
                     id: String,
                     page: usize,
                     bbox: Option<[f64; 4]>| {
        let _ = blocks_len;
        if let Some(bb) = bbox {
            render_meta.insert(id, RenderMeta { page_idx: page, bbox: bb });
        }
    };

    for (page_idx, page_blocks) in pages.as_array().into_iter().flatten().enumerate() {
        let size = layout.get(&page_idx).copied().unwrap_or([612.0, 792.0]);
        page_count = page_count.max(page_idx + 1);
        for b in page_blocks.as_array().into_iter().flatten() {
            let ty = str_of(&b["type"]);
            let bbox = json_bbox(b.get("bbox"), size);
            let content = b.get("content").cloned().unwrap_or(serde_json::Value::Null);
            match ty.as_str() {
                "title" => {
                    let id = format!("b_{:04}", blocks.len());
                    let level = content.get("level").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                    let (text, segs) =
                        segs_to_inline(content.get("title_content"), dir, &mut resources, &mut counter, doc_noisy);
                    if first_heading.is_none() && !text.trim().is_empty() {
                        first_heading = Some(text.trim().to_string());
                    }
                    push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                    blocks.push(Block::Paragraph {
                        id,
                        style: ParagraphStyle { heading_level: Some(level), ..Default::default() },
                        content: segs,
                    });
                }
                "paragraph" => {
                    let id = format!("b_{:04}", blocks.len());
                    let (_, segs) = segs_to_inline(
                        content.get("paragraph_content"),
                        dir,
                        &mut resources,
                        &mut counter, doc_noisy,
                    );
                    push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                    blocks.push(Block::Paragraph {
                        id,
                        style: ParagraphStyle::default(),
                        content: segs,
                    });
                }
                "list" => {
                    for item in content
                        .get("list_items")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                    {
                        let id = format!("b_{:04}", blocks.len());
                        let (_, segs) = segs_to_inline(
                            item.get("item_content"),
                            dir,
                            &mut resources,
                            &mut counter, doc_noisy,
                        );
                        push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                        blocks.push(Block::Paragraph {
                            id,
                            // v2.3 B3：列表条目（含参考文献）与 DOCX/内置 PDF 路径的
                            // is_list_item 语义对齐
                            style: ParagraphStyle { is_list_item: true, ..Default::default() },
                            content: segs,
                        });
                    }
                }
                "image" | "chart" => {
                    let path = content
                        .pointer("/image_source/path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let id = format!("b_{:04}", blocks.len());
                    match read_image_resource(dir, path) {
                        Some(res) => {
                            let rid = format!("img_{counter:04}");
                            counter += 1;
                            resources.insert(rid.clone(), res);
                            let (w, h) = bbox
                                .map(|bb| {
                                    (
                                        Some((bb[2] - bb[0]).max(0.0) as u32),
                                        Some((bb[3] - bb[1]).max(0.0) as u32),
                                    )
                                })
                                .unwrap_or((None, None));
                            push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                            blocks.push(Block::Image {
                                id,
                                resource_id: rid,
                                width: w,
                                height: h,
                                alt_text: None,
                            });
                        }
                        None => blocks.push(Block::Skipped {
                            id,
                            source_type: format!("{ty}(missing)"),
                            reason: crate::model::SkipReason::Unmapped {
                                type_name: "image file missing".into(),
                            },
                        }),
                    }
                    // 内嵌题注（image_caption / chart_caption）→ 独立题注段，紧随其后
                    let (_, mut cap_segs) = segs_to_inline(
                        content.get("image_caption"),
                        dir,
                        &mut resources,
                        &mut counter, doc_noisy,
                    );
                    let (_, chart_segs) = segs_to_inline(
                        content.get("chart_caption"),
                        dir,
                        &mut resources,
                        &mut counter, doc_noisy,
                    );
                    cap_segs.extend(chart_segs);
                    if cap_segs
                        .iter()
                        .any(|c| matches!(c, InlineContent::Text { text, .. } if !text.trim().is_empty()))
                    {
                        let id = format!("b_{:04}", blocks.len());
                        push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                        blocks.push(Block::Paragraph {
                            id,
                            style: ParagraphStyle { is_caption: true, ..Default::default() },
                            content: cap_segs,
                        });
                    }
                }
                "table" => {
                    let html = content.get("html").and_then(|v| v.as_str()).unwrap_or_default();
                    let id = format!("b_{:04}", blocks.len());
                    let rid = format!("tbl_{counter:04}");
                    counter += 1;
                    if let Some(path) =
                        content.pointer("/image_source/path").and_then(|v| v.as_str())
                    {
                        if let Some(shot) = read_image_resource(dir, path) {
                            resources.insert(format!("{rid}_shot"), shot);
                        }
                    }
                    let preview_text: String = html
                        .replace("<td>", " ")
                        .replace("</td>", "")
                        .replace("<tr>", "")
                        .replace("</tr>", " ")
                        .replace("<table>", "")
                        .replace("</table>", "")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    resources.insert(rid.clone(), Resource::table(html));
                    push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                    blocks.push(Block::Table {
                        id,
                        resource_id: rid,
                        preview_text: Some(preview_text.chars().take(120).collect()),
                    });
                    let (_, cap_segs) =
                        segs_to_inline(content.get("table_caption"), dir, &mut resources, &mut counter, doc_noisy);
                    if cap_segs
                        .iter()
                        .any(|c| matches!(c, InlineContent::Text { text, .. } if !text.trim().is_empty()))
                    {
                        let id = format!("b_{:04}", blocks.len());
                        push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                        blocks.push(Block::Paragraph {
                            id,
                            style: ParagraphStyle { is_caption: true, ..Default::default() },
                            content: cap_segs,
                        });
                    }
                }
                "equation_interline" => {
                    let id = format!("b_{:04}", blocks.len());
                    let latex = str_of(&content["math_content"]);
                    let rid = format!("eq_{counter:04}");
                    counter += 1;
                    resources.insert(rid.clone(), Resource::formula(String::new(), Some(latex)));
                    push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                    blocks.push(Block::Formula { id, resource_id: rid, omml: String::new() });
                }
                "algorithm" => {
                    // v2.2 T4：algorithm_content 的 text 段内含 \n\n 分行、▷ 行内注释、
                    // equation_inline 行内公式。按行重组 AlgoLine（Label/Stmt/Comment）
                    let cap = segs_to_inline(
                        content.get("algorithm_caption"),
                        dir,
                        &mut resources,
                        &mut counter, doc_noisy,
                    )
                    .0;
                    let mut lines: Vec<crate::model::AlgoLine> = Vec::new();
                    let mut cur: Vec<InlineContent> = Vec::new();
                    fn flush(
                        cur: &mut Vec<InlineContent>,
                        lines: &mut Vec<crate::model::AlgoLine>,
                    ) {
                        if cur.is_empty() {
                            return;
                        }
                        let joined = cur
                            .iter()
                            .filter_map(|c| {
                                if let InlineContent::Text { text, .. } = c {
                                    Some(text.clone())
                                } else {
                                    None
                                }
                            })
                            .collect::<String>();
                        let kind = algo_line_kind(&joined);
                        lines.push(crate::model::AlgoLine {
                            kind,
                            segments: std::mem::take(cur),
                        });
                    }
                    for seg in content
                        .get("algorithm_content")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                    {
                        match seg.get("type").and_then(|v| v.as_str()) {
                            Some("text") => {
                                let t = sanitize_script_noise(&str_of(&seg["content"]), doc_noisy);
                                for (k, piece) in t.split("\n\n").enumerate() {
                                    if k > 0 {
                                        flush(&mut cur, &mut lines);
                                    }
                                    if !piece.trim().is_empty() {
                                        cur.push(InlineContent::Text {
                                            text: piece.to_string(),
                                            bold: false,
                                            italic: false,
                                            underline: false,
                                        });
                                    }
                                }
                            }
                            Some("equation_inline") => {
                                let rid = format!("eqa_{counter:04}");
                                counter += 1;
                                resources.insert(
                                    rid.clone(),
                                    Resource::formula(String::new(), Some(str_of(&seg["content"]))),
                                );
                                cur.push(InlineContent::InlineFormula { resource_id: rid });
                            }
                            _ => {}
                        }
                    }
                    flush(&mut cur, &mut lines);
                    let id = format!("b_{:04}", blocks.len());
                    push_meta(&mut 0, &mut render_meta, id.clone(), page_offset + page_idx, bbox);
                    blocks.push(Block::Algorithm { id, header: cap, lines });
                }
                other => {
                    let reason = if DECOR_TYPES.contains(&other) {
                        crate::model::SkipReason::Decoration
                    } else {
                        crate::model::SkipReason::Unmapped { type_name: other.to_string() }
                    };
                    blocks.push(Block::Skipped {
                        id: format!("b_{:04}", blocks.len()),
                        source_type: other.to_string(),
                        reason,
                    });
                }
            }
        }
    }

    let page_sizes: Vec<[f64; 2]> = (0..page_count)
        .map(|i| layout.get(&i).copied().unwrap_or([612.0, 792.0]))
        .collect();
    Ok(Document {
        id: uuid::Uuid::new_v4().to_string(),
        title: first_heading.filter(|t| !t.is_empty()).unwrap_or_else(|| fallback_title.to_string()),
        blocks,
        resources,
        render_meta,
        pdf_meta: Some(PdfMeta { page_count, page_sizes }),
    })
}

/// 算法行归类（v2.2 T4）：▷ 注释 / 结构标签 / 语句。
/// 含 ▷ 的整行归 Comment（数学与文本段全保留，渲染灰斜体）；
/// 行首匹配结构关键字归 Label（不翻译，根治 E9 散文化）。
fn algo_line_kind(line_text: &str) -> crate::model::AlgoLineKind {
    let t = line_text.trim_start();
    if t.contains('▷') {
        return crate::model::AlgoLineKind::Comment;
    }
    let label_heads = [
        "Input:", "Output:", "Require:", "Ensure:", "for ", "end for", "end ", "while ", "if ",
        "else", "return", "repeat ", "until ",
    ];
    if label_heads.iter().any(|p| t.starts_with(p)) || t == "end" {
        return crate::model::AlgoLineKind::Label;
    }
    crate::model::AlgoLineKind::Stmt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, ResourceKind};

    fn fixture_dir(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/mineru")
            .join(name)
    }

    fn text_of_block(b: &Block) -> Option<String> {
        match b {
            Block::Paragraph { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        InlineContent::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    #[test]
    fn v1_ref_text_maps_to_list_paragraph_and_v2_decor_skipped() {
        // v2.3 B3：v1 参考文献条目曾落 Skipped{Unmapped}（线上 v2.1 更是静默 Empty，
        // IEEE 文档 35 条文献消失）；v2 管线的 page_header/page_footer 是装饰同义词
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("content_list.json"),
            r#"[{"type":"text","text":"Intro","page_idx":0},
                {"type":"ref_text","text":"[1] Z. Akata et al., \"Label-embedding,\" in Proc. CVPR, 2016.","page_idx":0}]"#,
        )
        .unwrap();
        let ir = adapt_dir(dir.path(), 0, "x").unwrap();
        let refs: Vec<&Block> = ir
            .blocks
            .iter()
            .filter(|b| match b {
                Block::Paragraph { style, .. } => style.is_list_item,
                _ => false,
            })
            .collect();
        assert_eq!(refs.len(), 1, "ref_text 应映射为 is_list_item 段落");
        assert!(
            text_of_block(refs[0]).unwrap().starts_with("[1]"),
            "文献条目文本保全"
        );
        // v2 装饰同义词（page_header/page_footer）→ Decoration 跳过（不进 Unmapped 违规）
        for decor in ["page_header", "page_footer"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("content_list.json"),
                format!(r#"[{{"type":"{decor}","text":"VOLUME 14, 2026","page_idx":0}}]"#),
            )
            .unwrap();
            let ir = adapt_dir(dir.path(), 0, "x").unwrap();
            assert!(
                matches!(
                    ir.blocks.as_slice(),
                    [Block::Skipped { reason: crate::model::SkipReason::Decoration, .. }]
                ),
                "{decor} 应为 Decoration 跳过: {:?}",
                ir.blocks
            );
        }
    }

    #[test]
    fn adapts_simple_to_ir_with_render_meta() {
        let doc = adapt_dir(&fixture_dir("simple"), 0, "simple").unwrap();
        assert_eq!(doc.blocks.len(), 5);
        assert!(doc.blocks.iter().all(|b| !b.is_placeholder()));
        let id0 = doc.blocks[0].id().to_string();
        let meta = &doc.render_meta[&id0];
        assert_eq!(meta.page_idx, 0);
        // bbox 从 1000 归一化还原到 PDF 点（页 612x792）：
        // simple 首段 content_list bbox=[91,74,893,133] → [55.7, 58.6, 546.1, 105.3]
        assert!((meta.bbox[0] - 91.0 / 1000.0 * 612.0).abs() < 0.01, "x0={}", meta.bbox[0]);
        assert!((meta.bbox[3] - 133.0 / 1000.0 * 792.0).abs() < 0.01, "y1={}", meta.bbox[3]);
        assert_eq!(doc.pdf_meta.as_ref().unwrap().page_count, 1);
        assert_eq!(doc.pdf_meta.as_ref().unwrap().page_sizes[0], [612.0, 792.0]);
        assert_eq!(doc.title, "simple"); // 无标题 → 文件名回退
    }

    #[test]
    fn adapts_headings_with_levels() {
        let doc = adapt_dir(&fixture_dir("headings"), 0, "headings").unwrap();
        match &doc.blocks[0] {
            Block::Paragraph { style, content, .. } => {
                assert_eq!(style.heading_level, Some(1));
                assert!(text_of_block(&doc.blocks[0]).unwrap().contains("Chapter One"));
                assert!(content.len() == 1);
            }
            _ => panic!("首块应为标题段落"),
        }
        match &doc.blocks[1] {
            Block::Paragraph { style, .. } => assert_eq!(style.heading_level, None),
            _ => panic!("第二块应为正文"),
        }
        // 标题成为文档标题
        assert!(doc.title.contains("Chapter One"), "title={}", doc.title);
    }

    #[test]
    fn adapts_equation_blocks_with_latex() {
        let doc = adapt_dir(&fixture_dir("with_formula"), 0, "f").unwrap();
        let eq_blocks = doc
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Formula { .. }))
            .count();
        assert_eq!(eq_blocks, 2);
        let latexes: Vec<String> = doc
            .resources
            .values()
            .filter(|r| r.kind == ResourceKind::Formula)
            .filter_map(|r| r.latex.clone())
            .collect();
        assert_eq!(latexes.len(), 2);
        assert!(
            latexes.iter().any(|l| l.contains("E = m c") || l.contains("mc")),
            "latex={latexes:?}"
        );
        // $$ 围栏已剥离
        assert!(latexes.iter().all(|l| !l.contains("$$")));
    }

    #[test]
    fn adapts_table_block_with_html_and_screenshot() {
        let doc = adapt_dir(&fixture_dir("with_table"), 0, "t").unwrap();
        assert_eq!(
            doc.blocks.iter().filter(|b| matches!(b, Block::Table { .. })).count(),
            1
        );
        let table_res = doc
            .resources
            .values()
            .find(|r| r.kind == ResourceKind::Table)
            .expect("表格资源");
        assert!(table_res.raw_xml.contains("<table>"));
        assert!(table_res.raw_xml.contains("Method"));
        // 表格截图已入池
        let has_shot = doc
            .resources
            .values()
            .any(|r| r.kind == ResourceKind::Image && !r.data_b64.is_empty());
        assert!(has_shot, "表格截图应入资源池");
    }

    #[test]
    fn adapts_image_blocks_with_bytes() {
        let doc = adapt_dir(&fixture_dir("with_image"), 0, "i").unwrap();
        assert_eq!(doc.blocks.iter().filter(|b| matches!(b, Block::Image { .. })).count(), 2);
        let imgs: Vec<&crate::model::Resource> = doc
            .resources
            .values()
            .filter(|r| r.kind == ResourceKind::Image)
            .collect();
        assert!(imgs.iter().all(|r| !r.data_b64.is_empty()));
        assert!(imgs.iter().all(|r| r.mime == "image/jpeg"));
        // 尺寸从 bbox 还原（非零）
        for b in &doc.blocks {
            if let Block::Image { width, height, .. } = b {
                assert!(width.unwrap_or(0) > 0 && height.unwrap_or(0) > 0);
            }
        }
    }

    #[test]
    fn block_ids_sequential_and_reading_order_preserved() {
        let doc = adapt_dir(&fixture_dir("two_column"), 0, "two_column").unwrap();
        for (i, b) in doc.blocks.iter().enumerate() {
            assert_eq!(b.id(), format!("b_{i:04}"));
        }
        let texts: Vec<String> = doc.blocks.iter().filter_map(text_of_block).collect();
        let li = texts.iter().position(|t| t.starts_with("Abstract")).expect("Abstract 段");
        let ri = texts
            .iter()
            .position(|t| t.contains("RIGHTCOL-MARKER"))
            .expect("右栏标记段");
        assert!(li < ri, "左栏内容应在右栏之前（阅读顺序）");
    }

    #[test]
    fn page_offset_applied_for_chunk() {
        let doc = adapt_dir(&fixture_dir("mixed_layout"), 10, "m").unwrap();
        assert!(doc.render_meta.values().all(|m| m.page_idx >= 10));
    }

    #[test]
    fn missing_image_degrades_to_empty_block() {
        // 构造一个 img_path 指向不存在文件的目录
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("content_list.json"),
            r#"[{"type":"image","img_path":"images/missing.jpg","bbox":[1,2,3,4],"page_idx":0}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("layout.json"),
            r#"{"pdf_info":[{"page_size":[612,792],"page_idx":0}]}"#,
        )
        .unwrap();
        let doc = adapt_dir(dir.path(), 0, "x").unwrap();
        assert!(matches!(doc.blocks[0], Block::Empty { .. }));
    }

    #[test]
    fn formula_fence_stripped() {
        assert_eq!(strip_formula_fence("$$\nE=mc^2\n$$"), "E=mc^2");
        assert_eq!(strip_formula_fence("E=mc^2"), "E=mc^2");
    }

    #[test]
    fn unmapped_types_become_skipped_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("content_list.json"),
            r#"[{"type":"text","text":"hello","page_idx":0},
                {"type":"chart","img_path":"","bbox":[1,2,3,4],"page_idx":0},
                {"type":"page_number","text":"3","page_idx":0},
                {"type":"handwriting","text":"?","page_idx":0}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("layout.json"),
            r#"{"pdf_info":[{"page_size":[612,792],"page_idx":0}]}"#,
        )
        .unwrap();
        let doc = adapt_dir(dir.path(), 0, "x").unwrap();
        assert!(matches!(&doc.blocks[0], Block::Paragraph { .. }));
        match &doc.blocks[1] {
            Block::Skipped { source_type, reason, .. } => {
                assert_eq!(source_type, "chart");
                assert!(matches!(reason, crate::model::SkipReason::Unmapped { .. }));
            }
            other => panic!("chart 应落 Skipped，实际 {other:?}"),
        }
        assert!(matches!(&doc.blocks[2], Block::Skipped {
            reason: crate::model::SkipReason::Decoration, .. }));
        assert!(matches!(&doc.blocks[3], Block::Skipped { source_type, .. } if source_type == "handwriting"));
    }

    #[test]
    fn adapts_b2_v2_captions_charts_and_structured_inline_math() {
        let doc = adapt_dir(&fixture_dir("b2_prototypical"), 0, "b2").unwrap();
        // 题注段（figure+table caption：4+6），is_caption=true
        let captions = doc.blocks.iter().filter(|b| matches!(b,
            Block::Paragraph { style, .. } if style.is_caption)).count();
        assert_eq!(captions, 10, "4 figure_caption + 6 table_caption，实际 {captions}");
        // 任一题注含行内公式资源引用（Figure 1 题注的 equation_inline 段）
        let cap_with_formula = doc.blocks.iter().any(|b| matches!(b,
            Block::Paragraph { style, content, .. } if style.is_caption && content.iter().any(|c|
                matches!(c, InlineContent::InlineFormula { .. }))));
        assert!(cap_with_formula, "Figure 1 题注应含行内公式");
        // chart 也映射为 Image（2 image + 4 chart）
        let images = doc.blocks.iter().filter(|b| matches!(b, Block::Image { .. })).count();
        assert_eq!(images, 6, "实际 {images}");
        // v2 结构化行内公式：paragraph 内容含 InlineFormula（数百级）
        let inline_formula_refs = doc.blocks.iter().filter_map(|b| b.paragraph_content())
            .flat_map(|c| c.iter())
            .filter(|c| matches!(c, InlineContent::InlineFormula { .. })).count();
        assert!(inline_formula_refs >= 20, "实际 {inline_formula_refs}");
        // 行内公式资源带 LaTeX
        let inline_latex = doc.resources.values()
            .filter(|r| r.kind == crate::model::ResourceKind::Formula)
            .filter(|r| r.latex.as_deref().is_some_and(|l| !l.is_empty()))
            .count();
        assert!(inline_latex >= 20, "实际 {inline_latex}");
        // algorithm 已实装为 Block::Algorithm（T4）
        assert!(doc.blocks.iter().any(|b| matches!(b, Block::Algorithm { .. })));
        // 阅读顺序：首个图片块的下一块是它的题注
        let bi = doc.blocks.iter().position(|b| matches!(b, Block::Image { .. })).unwrap();
        assert!(matches!(&doc.blocks[bi + 1], Block::Paragraph { style, .. } if style.is_caption));
        // 题注块有 render_meta（bbox 保位）
        let cap_ids: Vec<&str> = doc.blocks.iter().filter_map(|b| match b {
            Block::Paragraph { id, style, .. } if style.is_caption => Some(id.as_str()),
            _ => None,
        }).collect();
        assert!(cap_ids.iter().all(|id| doc.render_meta.contains_key(*id)));
        // 标题从 title 条目还原（21 个 heading 块）
        let headings = doc.blocks.iter().filter(|b| matches!(b,
            Block::Paragraph { style, .. } if style.heading_level.is_some())).count();
        assert_eq!(headings, 21, "实际 {headings}");
        assert!(doc.title.contains("Prototypical"), "title={}", doc.title);
        // list（参考文献 31 条）映射为普通段落
        let paras = doc.blocks.iter().filter(|b| matches!(b,
            Block::Paragraph { style, .. } if !style.is_caption && style.heading_level.is_none())).count();
        assert_eq!(paras, 55 + 31, "55 paragraph + 31 list items，实际 {paras}");
    }

    #[test]
    fn adapts_b2_algorithm_block() {
        let doc = adapt_dir(&fixture_dir("b2_prototypical"), 0, "b2").unwrap();
        let algos: Vec<&Block> =
            doc.blocks.iter().filter(|b| matches!(b, Block::Algorithm { .. })).collect();
        assert_eq!(algos.len(), 1);
        let Block::Algorithm { header, lines, .. } = algos[0] else { panic!() };
        // b2 algorithm_caption 为空；Algorithm 1 题注行是其前面的独立 paragraph（保留）
        assert!(header.is_empty());
        // b2 Algorithm 1 伪代码：Output 行 + 赋值/循环 + end for×2 + 注释行（\n\n 分行）
        assert!(lines.len() >= 10, "lines={}", lines.len());
        assert!(matches!(lines[0].kind, crate::model::AlgoLineKind::Label));
        assert!(lines[0].segments.iter().any(|s| matches!(s,
            InlineContent::Text { text, .. } if text.contains("Output"))));
        assert!(lines.iter().any(|l| matches!(l.kind, crate::model::AlgoLineKind::Label)
            && l.segments.iter().any(|s| matches!(s,
                InlineContent::Text { text, .. } if text.contains("end for")))));
        assert!(lines.iter().any(|l| matches!(l.kind, crate::model::AlgoLineKind::Comment)));
        // 行内公式进入 segments（RANDOMSAMPLE/V← 等数学）
        assert!(lines.iter().flat_map(|l| l.segments.iter())
            .any(|s| matches!(s, InlineContent::InlineFormula { .. })));
        // render_meta 有该块（bbox 保位）
        assert!(doc.render_meta.contains_key(algos[0].id()));
    }

    #[test]
    fn v1_fixtures_unchanged_by_v2_path() {
        // 7 个 v1 夹具无 content_list_v2.json → 走 v1 路径（既有 9 测试覆盖，此处抽查）
        let doc = adapt_dir(&fixture_dir("two_column"), 0, "t").unwrap();
        assert_eq!(doc.blocks.len(), 21);
        assert!(!crate::mineru::raw::has_v2(&fixture_dir("two_column")));
    }

    #[test]
    fn v2_minimal_synth_fixture() {
        // 手工构造最小 v2 目录（题注/公式/表格骨架），验证映射不受 b2 数据耦合
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("content_list_v2.json"), r#"[
          [
            {"type":"title","content":{"title_content":[{"type":"text","content":"Title Here"}],"level":1},"bbox":[100,50,500,70]},
            {"type":"paragraph","content":{"paragraph_content":[
              {"type":"text","content":"mix "},
              {"type":"equation_inline","content":"x^2"},
              {"type":"text","content":" tail"}]},"bbox":[100,80,500,120]},
            {"type":"image","content":{"image_source":{"path":""},
              "image_caption":[{"type":"text","content":"Figure 1: demo"}]},"bbox":[100,130,500,330]},
            {"type":"table","content":{"html":"<table><tr><td>A</td></tr></table>",
              "table_caption":[{"type":"text","content":"Table 1: t"}]},"bbox":[100,340,500,440]},
            {"type":"page_number","content":{"page_number_content":[{"type":"text","content":"9"}]},"bbox":[300,780,320,790]}
          ]
        ]"#).unwrap();
        std::fs::write(dir.path().join("layout.json"),
            r#"{"pdf_info":[{"page_size":[612,792],"page_idx":0}]}"#).unwrap();
        let doc = adapt_dir(dir.path(), 0, "synth").unwrap();
        assert_eq!(doc.blocks.len(), 7, "{:?}", doc.blocks.iter().map(|b| b.id()).collect::<Vec<_>>());
        assert!(matches!(&doc.blocks[0], Block::Paragraph { style, .. } if style.heading_level == Some(1)));
        // 行内公式独立资源
        match &doc.blocks[1] {
            Block::Paragraph { content, .. } => {
                assert_eq!(content.len(), 3);
                assert!(matches!(&content[1], InlineContent::InlineFormula { resource_id } if
                    doc.resources[resource_id].latex.as_deref() == Some("x^2")));
            }
            other => panic!("应为段落：{other:?}"),
        }
        // 空图片路径 → Skipped（资源缺失降级账目）+ 题注仍保留
        assert!(matches!(&doc.blocks[2], Block::Skipped { .. }));
        assert!(matches!(&doc.blocks[3], Block::Paragraph { style, .. } if style.is_caption
            && text_of_block(&doc.blocks[3]).unwrap().contains("Figure 1")));
        assert!(matches!(&doc.blocks[4], Block::Table { .. }));
        assert!(matches!(&doc.blocks[5], Block::Paragraph { style, .. } if style.is_caption
            && text_of_block(&doc.blocks[5]).unwrap().contains("Table 1")));
        assert!(matches!(&doc.blocks[6], Block::Skipped { reason: crate::model::SkipReason::Decoration, .. }));
        assert_eq!(doc.title, "Title Here");
    }

    // ── 伪上/下标噪声清洗（feedback_0913：14_EffTrans_Mamba 全篇伪 <sub>）──

    #[test]
    fn script_noise_thresholds() {
        // 合法脚注/化学式标记：单字符内容不触发
        let footnotes = "HONGYU ZHU<sup>1</sup>, GUI LIU<sup>2</sup>, and RONG LI<sup>1</sup>";
        assert!(!is_script_noise(footnotes));
        assert!(!is_script_noise("Water H<sub>2</sub>O and CO<sub>2</sub> emissions"));
        // 伪下标：≥3 个长内容标签触发（M<sub>ac</sub>hin<sub>e</sub> 式）
        assert!(is_script_noise("M<sub>ac</sub>hin<sub>e</sub> L<sub>ea</sub>rnin<sub>g</sub> D<sub>epa</sub>rtm<sub>e</sub>nt"));
        // 包裹字符占比 ≥8% 触发（不足 3 个长标签但短文本重度包裹）
        assert!(is_script_noise("x<sub>abcd</sub> y<sub>efgh</sub>"));
        // 偶发单个长标签（数学变量下标）不算噪声
        assert!(!is_script_noise("where x<sub>max</sub> denotes the largest value in the input sequence"));
        // 去包裹保留内容；统计跳过未闭合标签
        assert_eq!(strip_script_tags("Se<sub>qu</sub>ence Mod<sup>el</sup>ing"), "Sequence Modeling");
        assert_eq!(script_tag_stats("a<sub>bc"), (0, 0));
        // 文档级：噪声条目占比 ≥10% 升级（1/10 恰好触发，0/10 不触发）
        let noisy = "M<sub>ac</sub>hin<sub>e</sub> L<sub>ea</sub>rnin<sub>g</sub> D<sub>epa</sub>rtm<sub>e</sub>nt";
        let mut texts = vec!["plain text item"; 10];
        assert!(!is_doc_script_noise(&texts));
        texts[0] = noisy;
        assert!(is_doc_script_noise(&texts));
    }

    #[test]
    fn v1_pseudo_script_stripped_footnotes_kept() {
        // 孤立噪声条目（1/11 < 10% 不升级文档级）：仅该条目去包裹，脚注标记保留
        let dir = tempfile::tempdir().unwrap();
        let mut items = vec![
            r#"{"type":"text","text":"F <sup>EW-SHOT</sup> <sup>image</sup> <sup>classification</sup> <sup>novel</sup> <sup>cat-</sup> egories from 1-5 examples","page_idx":0}"#.to_string(),
            r#"{"type":"text","text":"HONGYU ZHU<sup>1</sup>, GUI LIU<sup>2</sup>, and RONG LI<sup>1</sup>","page_idx":0}"#.to_string(),
        ];
        for i in 0..9 {
            items.push(format!(r#"{{"type":"text","text":"filler paragraph {i}","page_idx":0}}"#));
        }
        std::fs::write(dir.path().join("content_list.json"), format!("[{}]", items.join(","))).unwrap();
        let ir = adapt_dir(dir.path(), 0, "x").unwrap();
        let texts: Vec<String> = ir.blocks.iter().filter_map(text_of_block).collect();
        assert_eq!(texts[0], "F EW-SHOT image classification novel cat- egories from 1-5 examples");
        assert_eq!(texts[1], "HONGYU ZHU<sup>1</sup>, GUI LIU<sup>2</sup>, and RONG LI<sup>1</sup>");
    }

    #[test]
    fn v1_pseudo_script_doc_escalation_strips_all() {
        // 文档级升级（2/4 ≥ 10%）：残留 1-2 个标签的边缘条目也一并去包裹
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("content_list.json"),
            r#"[
              {"type":"text","text":"M<sub>ac</sub>hin<sub>e</sub> L<sub>ea</sub>rnin<sub>g</sub> Departm<sub>e</sub>nt","text_level":1,"page_idx":0},
              {"type":"text","text":"Se<sub>qu</sub>ence Modelin<sub>g</sub> <sub>w</sub>ith Selecti<sub>v</sub>e State Spaces","page_idx":0},
              {"type":"text","text":"Arena (Ta<sub>y</sub>, De<sub>g</sub>hani, et al. 2021)","page_idx":0},
              {"type":"ref_text","text":"[1] G<sub>u</sub> et al. 2023.","page_idx":0}
            ]"#,
        )
        .unwrap();
        let ir = adapt_dir(dir.path(), 0, "x").unwrap();
        let texts: Vec<String> = ir.blocks.iter().filter_map(text_of_block).collect();
        assert!(texts.iter().all(|t| !t.contains("<sub>") && !t.contains("<sup>")), "{texts:?}");
        assert_eq!(texts[0], "Machine Learning Department");
        assert_eq!(ir.title, "Machine Learning Department");
        assert_eq!(texts[3], "[1] Gu et al. 2023.");
    }

    #[test]
    fn v2_pseudo_script_stripped_in_segs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("content_list_v2.json"),
            r#"[
              [
                {"type":"title","content":{"title_content":[
                  {"type":"text","content":"M<sub>ac</sub>hin<sub>e</sub> L<sub>ea</sub>rnin<sub>g</sub> at CMU"}],"level":1},"bbox":[100,50,500,70]},
                {"type":"paragraph","content":{"paragraph_content":[
                  {"type":"text","content":"M<sub>ac</sub>hin<sub>e</sub> works "},
                  {"type":"equation_inline","content":"x_t"},
                  {"type":"text","content":"on se<sub>qu</sub>ences of to<sub>k</sub>ens."}]},"bbox":[100,80,500,120]}
              ]
            ]"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("layout.json"),
            r#"{"pdf_info":[{"page_size":[612,792],"page_idx":0}]}"#).unwrap();
        let ir = adapt_dir(dir.path(), 0, "x").unwrap();
        assert_eq!(ir.title, "Machine Learning at CMU");
        let texts: Vec<String> = ir.blocks.iter().filter_map(text_of_block).collect();
        assert!(texts.iter().all(|t| !t.contains("<sub>")), "{texts:?}");
        assert_eq!(texts[1], "Machine works on sequences of tokens.");
        // 行内公式资源不受清洗影响
        assert!(ir.resources.values().any(|r| r.latex.as_deref() == Some("x_t")), "equation_inline 不应被改写");
    }
}
