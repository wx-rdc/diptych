//! MinerU 云端 VLM 解析（v2）：client / splitter / adapter / merger。
//!
//! 协议细节见 docs/mineru-output-spec.md（2026-09-09 实测）。
//! 编排入口 [`parse_pdf_mineru`]：拆分 → 批量签名上传 → 轮询 → 下载 → 适配 → 合并。

pub mod adapter;
pub mod client;
pub mod merger;
pub mod raw;
pub mod splitter;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use adapter::{adapt_dir, AdaptError, ContentItem};
pub use client::{
    BatchFileResult, BatchState, FileProgress, MineruClient, MineruConfig, MineruError,
};
pub use merger::merge_documents;
pub use splitter::{plan_boundaries, split_pdf, PdfChunk, SplitConfig, SplitError, SplitResult};

/// SSE 进度阶段（server 转发给前端）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ParseStage {
    /// 拆分为 N 个片段
    Splitting { chunks: usize },
    /// 上传片段 current/total
    Uploading { current: usize, total: usize },
    /// 云端解析：done_files/total_files + 运行中文件的页级进度
    Parsing { done_files: usize, total_files: usize, pages_done: u32, pages_total: u32 },
    /// 下载与合并结果
    Merging,
}

/// MinerU 端到端解析：拆分 → 批量上传 → 轮询 → 下载解压 → 适配 → 合并。
///
/// 单片文件也走同一路径（保证 pdf_meta/render_meta 结构一致）。
/// 任一片段失败 → 整体 Err（auto 回退由 server 层处理）。
pub async fn parse_pdf_mineru<P>(
    file_path: &Path,
    client: &MineruClient,
    cfg: &MineruConfig,
    split: &SplitConfig,
    work_dir: &Path,
    progress: P,
) -> Result<crate::model::Document, MineruError>
where
    P: Fn(&ParseStage),
{
    std::fs::create_dir_all(work_dir)?;

    // 1. 拆分
    let split_result = split_pdf(file_path, split, &work_dir.join("chunks"))?;
    let chunks = split_result.chunks;
    progress(&ParseStage::Splitting { chunks: chunks.len() });

    // 2. 签名上传（一个 batch 携带全部片段）
    let names: Vec<String> = chunks
        .iter()
        .map(|c| c.file_path.file_name().unwrap_or_default().to_string_lossy().to_string())
        .collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let (batch_id, urls) = client.request_upload_urls(&name_refs, cfg).await?;
    for (i, (chunk, url)) in chunks.iter().zip(urls.iter()).enumerate() {
        client.upload_file(url, &chunk.file_path).await?;
        progress(&ParseStage::Uploading { current: i + 1, total: chunks.len() });
    }

    // 3. 轮询（结果按文件名对齐回 chunks）
    let poll_timeout = Duration::from_secs(
        std::env::var("GEMINUS_MINERU_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900),
    );
    let results = client
        .poll_batch(&batch_id, poll_timeout, Duration::from_secs(3), |snapshot| {
            let done_files = snapshot
                .iter()
                .filter(|r| matches!(r.state, BatchState::Done | BatchState::Failed(_)))
                .count();
            let (pages_done, pages_total) = snapshot
                .iter()
                .find_map(|r| match &r.state {
                    BatchState::Running(p) => Some((p.extracted_pages, p.total_pages)),
                    _ => None,
                })
                .unwrap_or((0, 0));
            progress(&ParseStage::Parsing {
                done_files,
                total_files: snapshot.len(),
                pages_done,
                pages_total,
            });
        })
        .await?;

    // 失败检查
    for r in &results {
        if let BatchState::Failed(msg) = &r.state {
            return Err(MineruError::TaskFailed(format!("{}: {}", r.file_name, msg)));
        }
    }

    // 4. 下载解压 + 适配（page_offset = chunk.start_page）
    progress(&ParseStage::Merging);
    let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");
    let mut parts = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let Some(r) = results.iter().find(|r| r.file_name == names[i]) else {
            return Err(MineruError::BadResponse(format!("结果缺少片段 {}", names[i])));
        };
        let Some(zip_url) = &r.full_zip_url else {
            return Err(MineruError::BadResponse(format!("片段 {} 无下载地址", names[i])));
        };
        let dest = work_dir.join(format!("chunk_{i:02}"));
        client.download_zip(zip_url, &dest).await?;
        let doc = adapt_dir(&dest, chunk.start_page, stem).map_err(|e| {
            MineruError::BadResponse(format!("片段 {} 适配失败: {e}", names[i]))
        })?;
        parts.push((format!("c{i:02}"), doc, chunk.start_page));
    }

    // 5. 合并
    Ok(merge_documents(parts, stem))
}

/// 从持久化的 raw 缓存目录离线重放（v2.2 T1.17）：适配器升级后零 API 重解析。
/// 目录布局（backfill_raw_cache.py / server 在线持久化共同维护）：
/// `data/mineru_cache/<hash>/raw_meta.json` + `chunk_NN/`（content_list*.json/layout.json/images）。
/// raw_meta.json：`{"chunks":[{"dir":"chunk_00","start_page":0},…]}`
pub fn reparse_from_dir(root: &Path, title: &str) -> Result<crate::model::Document, adapter::AdaptError> {
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
    let meta: RawMeta = serde_json::from_reader(
        std::fs::File::open(root.join("raw_meta.json")).map_err(adapter::AdaptError::Io)?,
    )?;
    if meta.chunks.is_empty() {
        return Err(adapter::AdaptError::MissingResource("raw_meta.json 无 chunks".into()));
    }
    // 每片 start_page 未记录时按各片 layout 页数累计（backfill 脚本产物无此字段）
    let mut parts = Vec::new();
    let mut offset = 0usize;
    for (i, c) in meta.chunks.iter().enumerate() {
        let dir = if Path::new(&c.dir).is_absolute() {
            PathBuf::from(&c.dir)
        } else {
            root.join(&c.dir)
        };
        let doc = adapter::adapt_dir(&dir, offset.max(c.start_page), title)?;
        offset = c.start_page + doc.pdf_meta.as_ref().map(|m| m.page_count).unwrap_or(0);
        parts.push((format!("c{i:02}"), doc, c.start_page));
    }
    Ok(merger::merge_documents(parts, title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stage_serializes_snake_case() {
        let s = serde_json::to_string(&ParseStage::Splitting { chunks: 3 }).unwrap();
        assert_eq!(s, r#"{"stage":"splitting","chunks":3}"#);
        let s = serde_json::to_string(&ParseStage::Parsing {
            done_files: 1,
            total_files: 2,
            pages_done: 3,
            pages_total: 10,
        })
        .unwrap();
        assert!(s.contains(r#""stage":"parsing""#));
        assert!(s.contains(r#""pages_done":3"#));
    }

    #[test]
    fn reparse_b2_fixture_offline() {
        // 正例：raw_meta 指向夹具（绝对路径兼容测试）→ 与 adapt_dir 单片等价
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/mineru/b2_prototypical");
        let tmp = tempfile::tempdir().unwrap();
        let meta = format!(r#"{{"chunks":[{{"dir":"{}","start_page":0}}]}}"#, fixture.display());
        std::fs::write(tmp.path().join("raw_meta.json"), meta).unwrap();
        let doc = reparse_from_dir(tmp.path(), "b2_Prototypical_Networks").unwrap();
        let direct = adapter::adapt_dir(&fixture, 0, "b2_Prototypical_Networks").unwrap();
        assert_eq!(doc.blocks.len(), direct.blocks.len());
        assert_eq!(doc.pdf_meta.as_ref().unwrap().page_count, 13);
        // 多片偏移：两片同一夹具 → 第二片 page_idx 平移 13
        let meta2 = format!(
            r#"{{"chunks":[{{"dir":"{}","start_page":0}},{{"dir":"{}","start_page":13}}]}}"#,
            fixture.display(),
            fixture.display()
        );
        std::fs::write(tmp.path().join("raw_meta.json"), meta2).unwrap();
        let doc2 = reparse_from_dir(tmp.path(), "b2x2").unwrap();
        assert_eq!(doc2.blocks.len(), direct.blocks.len() * 2);
        assert_eq!(doc2.pdf_meta.as_ref().unwrap().page_count, 26);
        assert!(doc2.render_meta.values().any(|m| m.page_idx >= 13));
    }

    #[test]
    fn reparse_missing_meta_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(reparse_from_dir(tmp.path(), "x").is_err());
    }
}
