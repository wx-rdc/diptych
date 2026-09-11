//! PDF 页级拆分（v2_plan §3.1）：大文件拆为 ≤max_pages 的片段，供 MinerU 分段解析。

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SplitError {
    #[error("PDF 读取失败: {0}")]
    Load(#[from] lopdf::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 拆分策略
#[derive(Debug, Clone)]
pub struct SplitConfig {
    /// 每片段最大页数（默认 10；MinerU 上限 200 页，取小值平衡进度粒度与请求数）
    pub max_pages: usize,
    /// 末片最小页数（默认 5；余下不足此数时并入前片，避免产生碎片片段）
    pub min_pages: usize,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self { max_pages: 10, min_pages: 5 }
    }
}

/// 一个拆分片段
#[derive(Debug, Clone)]
pub struct PdfChunk {
    pub index: usize,
    /// 起始页（0-based，相对于原文件，含）
    pub start_page: usize,
    /// 结束页（含）
    pub end_page: usize,
    pub file_path: PathBuf,
    pub page_count: usize,
}

/// 拆分结果
#[derive(Debug)]
pub struct SplitResult {
    pub chunks: Vec<PdfChunk>,
    pub total_pages: usize,
}

/// 分片边界规划（纯函数）：按 max_pages 均分；末片页数 < min_pages 时并入前片。
pub fn plan_boundaries(total_pages: usize, cfg: &SplitConfig) -> Vec<(usize, usize)> {
    if total_pages == 0 {
        return vec![];
    }
    let per = cfg.max_pages.clamp(1, total_pages);
    let n = total_pages.div_ceil(per);
    let mut out: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        let start = i * per;
        let end = (start + per - 1).min(total_pages - 1);
        // 末片太小 → 延伸前片边界（表现为前一片多含几页）
        if i == n - 1 && n > 1 && end - start + 1 < cfg.min_pages.min(per) {
            if let Some(prev) = out.last_mut() {
                prev.1 = end;
                continue;
            }
        }
        out.push((start, end));
    }
    out
}

/// 将 PDF 按配置拆分为多个片段文件（写入 out_dir）。
/// 单页/少页文件产出 1 个片段（走同一路径保证下游一致）。
pub fn split_pdf(
    input: &Path,
    cfg: &SplitConfig,
    out_dir: &Path,
) -> Result<SplitResult, SplitError> {
    std::fs::create_dir_all(out_dir)?;
    let base = lopdf::Document::load(input)?;
    let total = base.get_pages().len();
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");

    let mut chunks = Vec::new();
    for (i, (start, end)) in plan_boundaries(total, cfg).into_iter().enumerate() {
        let mut doc = base.clone();
        let keep: std::collections::HashSet<usize> = (start..=end).collect();
        let drop: Vec<u32> = (0..total).filter(|p| !keep.contains(p)).map(|p| p as u32 + 1).collect();
        if !drop.is_empty() {
            doc.delete_pages(&drop);
        }
        let path = out_dir.join(format!("{stem}_chunk_{i:02}.pdf"));
        doc.compress();
        doc.save(&path)?;
        chunks.push(PdfChunk {
            index: i,
            start_page: start,
            end_page: end,
            file_path: path,
            page_count: end - start + 1,
        });
    }
    Ok(SplitResult { chunks, total_pages: total })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/pdf")
            .join(name)
    }

    #[test]
    fn boundaries_even_split() {
        let b = plan_boundaries(50, &SplitConfig::default());
        assert_eq!(b.len(), 5);
        assert_eq!(b[0], (0, 9));
        assert_eq!(b[4], (40, 49));
    }

    #[test]
    fn boundaries_small_tail_merges_into_prev() {
        // 14 页 max=10 → 10+4，4<min(5) → 合并为 14 页一片
        let b = plan_boundaries(14, &SplitConfig::default());
        assert_eq!(b, vec![(0, 13)]);
        // 16 页 → 10+6，6≥5 → 保持两片
        let b = plan_boundaries(16, &SplitConfig::default());
        assert_eq!(b, vec![(0, 9), (10, 15)]);
    }

    #[test]
    fn boundaries_tiny_and_zero() {
        assert!(plan_boundaries(0, &SplitConfig::default()).is_empty());
        assert_eq!(plan_boundaries(3, &SplitConfig::default()), vec![(0, 2)]);
    }

    #[test]
    fn splits_large_pdf_into_max_page_chunks() {
        // large.pdf 为 100 页夹具
        let result =
            split_pdf(&fixture("large.pdf"), &SplitConfig::default(), tempfile::tempdir().unwrap().path())
                .unwrap();
        assert_eq!(result.total_pages, 100);
        assert_eq!(result.chunks.len(), 10);
        assert_eq!(result.chunks[0].page_count, 10);
        assert_eq!((result.chunks[9].start_page, result.chunks[9].end_page), (90, 99));
    }

    #[test]
    fn small_pdf_yields_single_chunk() {
        let result =
            split_pdf(&fixture("simple.pdf"), &SplitConfig::default(), tempfile::tempdir().unwrap().path())
                .unwrap();
        assert_eq!(result.chunks.len(), 1);
        assert_eq!(result.chunks[0].page_count, result.total_pages);
        assert_eq!(result.chunks[0].start_page, 0);
    }

    #[test]
    fn output_files_are_valid_pdfs_with_page_count() {
        let dir = tempfile::tempdir().unwrap();
        let result = split_pdf(
            &fixture("mixed_layout.pdf"),
            &SplitConfig { max_pages: 2, min_pages: 1 },
            dir.path(),
        )
        .unwrap();
        assert_eq!(result.total_pages, 3);
        for chunk in &result.chunks {
            assert!(chunk.file_path.exists(), "{} 应存在", chunk.file_path.display());
            let doc = lopdf::Document::load(&chunk.file_path).unwrap();
            assert_eq!(doc.get_pages().len(), chunk.page_count);
        }
    }
}
