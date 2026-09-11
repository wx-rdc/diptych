//! MinerU 原始条目加载（v2.2 T1 覆盖度审计的原始侧）。
//!
//! v2（`*_content_list_v2.json`，分页数组 + 结构化内容字段）优先；缺失时回退 v1
//! `content_list.json`（扁平 text/img_path/bbox）。输出统一 [`RawItem`]：
//! 逻辑类型 + 文本 + PDF 点 bbox（两种格式 bbox 均为 1000 归一化，按 layout.json
//! page_size 还原）。v2 的内嵌题注（image_caption/chart_caption/table_caption）
//! 派生为独立逻辑条目，使审计能发现"题注未映射"（A1 根因）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::mineru::adapter::AdaptError;

/// 审计用原始条目（逻辑类型）：
/// text / title / image / chart / table / equation / figure_caption / table_caption /
/// algorithm / page_number / page_footnote / aside_text / …（未知类型原样保留）
#[derive(Debug, Clone, PartialEq)]
pub struct RawItem {
    pub kind: String,
    pub text: String,
    pub page_idx: usize,
    /// PDF 点 bbox [x0, y0, x1, y1]（1000 归一化已按 page_size 还原）
    pub bbox: Option<[f64; 4]>,
}

/// 目录下是否存在 v2 格式
pub fn has_v2(dir: &Path) -> bool {
    find_v2_path(dir).is_some()
}

/// 定位 content_list_v2.json（真实 ZIP 带 UUID 前缀；夹具可能为精确名）
pub fn find_v2_path(dir: &Path) -> Option<PathBuf> {
    let exact = dir.join("content_list_v2.json");
    if exact.exists() {
        return Some(exact);
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("_content_list_v2.json"))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

fn str_of(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// v2 bbox（1000 归一化）→ PDF 点
fn json_bbox(v: Option<&serde_json::Value>, size: [f64; 2]) -> Option<[f64; 4]> {
    let a = v?.as_array()?;
    let n = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
    Some([
        n(0) / 1000.0 * size[0],
        n(1) / 1000.0 * size[1],
        n(2) / 1000.0 * size[0],
        n(3) / 1000.0 * size[1],
    ])
}

/// 段数组拼接散文文本（仅 type=="text" 的段；equation_inline 是结构化公式，
/// 字符归公式保真而非散文覆盖度——与 IR 侧 ir_text_len 只计 Text 段的口径一致）
fn segs_prose(segs: Option<&serde_json::Value>) -> String {
    segs.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter(|s| s.get("type").and_then(|v| v.as_str()) == Some("text"))
                .map(|s| str_of(&s["content"]))
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// 加载原始条目（v2 优先，v1 回退）
pub fn load_raw_items(dir: &Path) -> Result<Vec<RawItem>, AdaptError> {
    let page_sizes = crate::mineru::adapter::load_page_sizes(dir);
    let size_at = |idx: usize| -> [f64; 2] {
        page_sizes.get(&idx).copied().unwrap_or([612.0, 792.0])
    };

    if let Some(v2_path) = find_v2_path(dir) {
        let pages: serde_json::Value = serde_json::from_reader(std::fs::File::open(v2_path)?)?;
        let mut out = Vec::new();
        for (page_idx, blocks) in pages.as_array().into_iter().flatten().enumerate() {
            let size = size_at(page_idx);
            for b in blocks.as_array().into_iter().flatten() {
                let ty = str_of(&b["type"]);
                let bbox = json_bbox(b.get("bbox"), size);
                let content = b.get("content").cloned().unwrap_or(serde_json::Value::Null);
                match ty.as_str() {
                    "image" | "chart" => {
                        out.push(RawItem { kind: ty.clone(), text: String::new(), page_idx, bbox });
                        // 题注内嵌于 image_caption / chart_caption（子图组常由后者携带）
                        for cap_key in ["image_caption", "chart_caption"] {
                            let text = segs_prose(content.get(cap_key));
                            if !text.trim().is_empty() {
                                out.push(RawItem {
                                    kind: "figure_caption".into(),
                                    text,
                                    page_idx,
                                    bbox,
                                });
                            }
                        }
                    }
                    "table" => {
                        out.push(RawItem { kind: "table".into(), text: String::new(), page_idx, bbox });
                        let text = segs_prose(content.get("table_caption"));
                        if !text.trim().is_empty() {
                            out.push(RawItem { kind: "table_caption".into(), text, page_idx, bbox });
                        }
                    }
                    "algorithm" => {
                        // 与适配器行组装同构：text 段按 \n\n 分行、逐行 trim、空行丢弃、
                        // \n 连接（否则分隔符空字符造成账目假缺口）
                        let body = segs_prose(content.get("algorithm_content"));
                        let body = body
                            .split("\n\n")
                            .map(str::trim)
                            .filter(|t| !t.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let cap = segs_prose(content.get("algorithm_caption"));
                        let text = if cap.trim().is_empty() { body } else { format!("{cap}\n{body}") };
                        out.push(RawItem { kind: "algorithm".into(), text, page_idx, bbox });
                    }
                    "title" => {
                        out.push(RawItem {
                            kind: "title".into(),
                            text: segs_prose(content.get("title_content")),
                            page_idx,
                            bbox,
                        });
                    }
                    "paragraph" => {
                        out.push(RawItem {
                            kind: "text".into(),
                            text: segs_prose(content.get("paragraph_content")),
                            page_idx,
                            bbox,
                        });
                    }
                    "list" => {
                        for item in content.get("list_items").and_then(|v| v.as_array()).into_iter().flatten() {
                            out.push(RawItem {
                                kind: "text".into(),
                                text: segs_prose(item.get("item_content")),
                                page_idx,
                                bbox,
                            });
                        }
                    }
                    "equation_interline" => {
                        out.push(RawItem {
                            kind: "equation".into(),
                            // LaTeX 非散文：字符保真由类型账目+资源结构覆盖，
                            // 不计入 char_coverage（与 IR Formula 块 0 散文字符口径一致）
                            text: String::new(),
                            page_idx,
                            bbox,
                        });
                    }
                    other => {
                        out.push(RawItem {
                            kind: other.to_string(),
                            text: segs_any(&content),
                            page_idx,
                            bbox,
                        });
                    }
                }
            }
        }
        return Ok(out);
    }

    // v1 回退：content_list.json（题注等内嵌信息缺失是 v1 格式固有盲区）
    let items: Vec<crate::mineru::adapter::ContentItem> = serde_json::from_reader(
        std::fs::File::open(crate::mineru::adapter::find_content_list(dir)?)?,
    )?;
    Ok(items
        .iter()
        .map(|i| {
            let size = size_at(i.page_idx);
            let kind = match i.item_type.as_str() {
                // v2.3 B3：ref_text 归并为 text（与 v2 list→text 同口径，
                // 审计账目与适配器的 Paragraph 映射对齐）
                "ref_text" => "text".to_string(),
                other => other.to_string(),
            };
            RawItem {
                kind,
                text: i.text.clone(),
                page_idx: i.page_idx,
                bbox: i.bbox.map(|bb| {
                    [
                        bb[0] / 1000.0 * size[0],
                        bb[1] / 1000.0 * size[1],
                        bb[2] / 1000.0 * size[0],
                        bb[3] / 1000.0 * size[1],
                    ]
                }),
            }
        })
        .collect())
}

/// 通用装饰类型文本（page_footnote_content 等任意 `*_content` 数组）
fn segs_any(content: &serde_json::Value) -> String {
    if let Some(obj) = content.as_object() {
        for v in obj.values() {
            if let Some(text) = v.as_array().map(|a| segs_text_from_array(a)) {
                return text;
            }
        }
    }
    String::new()
}

fn segs_text_from_array(a: &[serde_json::Value]) -> String {
    a.iter().map(|s| str_of(&s["content"])).collect()
}

/// raw 缓存根目录（data/mineru_cache/<hash>/）整体加载：raw_meta.json 记录各片
/// 目录与首页偏移，逐片 load_raw_items 后按偏移平移 page_idx。
pub fn load_raw_items_from_root(root: &Path) -> Result<Vec<RawItem>, AdaptError> {
    #[derive(serde::Deserialize)]
    struct ChunkEntry {
        dir: String,
        #[serde(default)]
        start_page: usize,
    }
    #[derive(serde::Deserialize)]
    struct RawMeta {
        chunks: Vec<ChunkEntry>,
    }
    let meta: RawMeta = serde_json::from_reader(std::fs::File::open(root.join("raw_meta.json"))?)?;
    let mut out = Vec::new();
    for c in &meta.chunks {
        let dir = if Path::new(&c.dir).is_absolute() {
            PathBuf::from(&c.dir)
        } else {
            root.join(&c.dir)
        };
        for mut item in load_raw_items(&dir)? {
            item.page_idx += c.start_page;
            out.push(item);
        }
    }
    Ok(out)
}

/// 兼容 HashMap 页尺寸表（adapter::load_page_sizes 的便捷视图）
#[allow(dead_code)]
fn _type_check(_: &HashMap<usize, [f64; 2]>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/mineru")
            .join(name)
    }

    #[test]
    fn loads_b2_fixture_v2_with_derived_kinds() {
        let items = load_raw_items(&fixture("b2_prototypical")).unwrap();
        let count = |k: &str| items.iter().filter(|i| i.kind == k).count();
        // b2 冻结数据实测（2026-09-10 侦察）：2 image + 4 chart（其中 2 chart 带题注）
        // → figure_caption = 2(image) + 2(chart) = 4；table_caption = 6
        assert_eq!(count("figure_caption"), 4, "{items:?}");
        assert_eq!(count("table_caption"), 6);
        assert_eq!(count("algorithm"), 1);
        assert_eq!(count("chart"), 4);
        assert_eq!(count("image"), 2);
        assert_eq!(count("page_number"), 12);
        assert_eq!(count("title"), 21);
        assert_eq!(count("text"), 55 + 31, "paragraph 55 + list 31 items");
        assert_eq!(count("equation"), 8);
        // 算法条目含伪代码散文（Output 行）；公式 LaTeX 不计入散文口径
        let algo = items.iter().find(|i| i.kind == "algorithm").unwrap();
        assert!(algo.text.contains("Output"));
        assert!(!algo.text.contains("RANDOMSAMPLE"));
        assert!(algo.bbox.is_some());
        // 题注文本非空
        assert!(items.iter().filter(|i| i.kind == "figure_caption").all(|i| i.text.contains("Figure")));
    }

    #[test]
    fn loads_v1_fixture_fallback() {
        let items = load_raw_items(&fixture("simple")).unwrap();
        assert!(items.iter().all(|i| i.kind == "text"));
        assert_eq!(items.len(), 5);
        assert!(items[0].bbox.is_some());
    }

    #[test]
    fn root_loader_offsets_pages() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = r#"{"chunks":[{"dir":"simple","start_page":7}]}"#;
        std::fs::write(tmp.path().join("raw_meta.json"), meta).unwrap();
        // 相对目录不存在于 tmp → 应报错；用绝对路径夹具验证偏移
        let abs = fixture("simple");
        let meta = format!(r#"{{"chunks":[{{"dir":"{}","start_page":7}}]}}"#, abs.display());
        std::fs::write(tmp.path().join("raw_meta.json"), meta).unwrap();
        let items = load_raw_items_from_root(tmp.path()).unwrap();
        assert_eq!(items.len(), 5);
        assert!(items.iter().all(|i| i.page_idx >= 7));
    }

    #[test]
    fn root_loader_missing_meta_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_raw_items_from_root(tmp.path()).is_err());
    }
}
