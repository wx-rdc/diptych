//! v2.4 T5：翻译任务控制流。
//! - `CancelToken`：轻量协作取消（AtomicBool + Notify，无 tokio-util 依赖）；
//! - `TaskRegistry`：doc_id → CancelToken 全局注册表（任务结束必须移除，防泄漏）；
//! - `page_batches`：按页分批（匿名熔断的扣减粒度；登录用户同样分批以获得取消响应）。
//!
//! 计划偏差说明：原计划用 DashMap + tokio_util::CancellationToken——本仓未依赖
//! 二者，等价自实现（语义相同：cancelled() 异步等待 + cancel() 立即生效）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use geminus_core::model::Document;

/// 协作式取消令牌
#[derive(Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

#[derive(Default)]
struct CancelInner {
    cancelled: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// 触发取消（幂等）
    pub fn cancel(&self) {
        self.inner
            .cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 已取消则立即完成，否则挂起等待 cancel()
    pub async fn cancelled(&self) {
        while !self.is_cancelled() {
            // 竞态兜底：notify_waiters 只唤醒已注册的等待者，漏醒则靠超时重查
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), notified).await;
        }
    }
}

/// 全局翻译任务注册表（doc_id → token）
#[derive(Default)]
pub struct TaskRegistry {
    map: Mutex<HashMap<String, CancelToken>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册新任务（同 doc_id 旧任务如有残留 token 先取消——防御性收口）
    pub fn register(&self, doc_id: &str) -> CancelToken {
        let token = CancelToken::new();
        let mut map = self.map.lock().unwrap();
        if let Some(old) = map.insert(doc_id.to_string(), token.clone()) {
            old.cancel();
        }
        token
    }

    pub fn get(&self, doc_id: &str) -> Option<CancelToken> {
        self.map.lock().unwrap().get(doc_id).cloned()
    }

    /// 任务结束移除（防泄漏）；仅当当前注册的仍是本任务的 token 才移除
    pub fn remove(&self, doc_id: &str, token: &CancelToken) {
        let mut map = self.map.lock().unwrap();
        if map
            .get(doc_id)
            .is_some_and(|t| Arc::ptr_eq(&t.inner, &token.inner))
        {
            map.remove(doc_id);
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 一个翻译批次：块下标区间 + 计费页数
pub struct PageBatch {
    pub from: usize,
    pub to: usize, // 不含
    pub pages: i64,
}

/// 按页分批（v2.4 T5 熔断粒度）：
/// - MinerU PDF：render_meta 有块→页号映射，同页块聚一批；
/// - DOCX / builtin：无页号 → 按 `estimated_pages` 把块均分成页等价批。
/// - 单批页数恒为 1（扣减粒度 = 页），批内块翻译仍由 core 并发调度。
pub fn page_batches(doc: &Document) -> Vec<PageBatch> {
    let total = doc.blocks.len();
    if total == 0 {
        return Vec::new();
    }
    // 页号映射（1-based page_idx）
    let page_of = |i: usize| -> Option<usize> {
        doc.render_meta.get(doc.blocks[i].id()).map(|m| m.page_idx)
    };
    if (0..total).all(|i| page_of(i).is_some()) {
        // MinerU PDF：按页聚合（块已按文档序，页号单调不减——防御性处理乱序）
        let mut batches: Vec<PageBatch> = Vec::new();
        let mut start = 0;
        let mut current_page = page_of(0).unwrap();
        for i in 1..total {
            let p = page_of(i).unwrap();
            if p != current_page {
                batches.push(PageBatch { from: start, to: i, pages: 1 });
                start = i;
                current_page = p;
            }
        }
        batches.push(PageBatch { from: start, to: total, pages: 1 });
        batches
    } else {
        // 无页号：estimated_pages 等分（至少 1 批）
        let pages = doc.estimated_pages().max(1);
        let per = total.div_ceil(pages);
        (0..total)
            .step_by(per)
            .map(|from| PageBatch {
                from,
                to: (from + per).min(total),
                pages: 1,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_token_cancel_then_await_completes() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        let waiter = t.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
        });
        tokio::task::yield_now().await;
        t.cancel();
        t.cancel(); // 幂等
        assert!(t.is_cancelled());
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("取消后等待者应被唤醒")
            .unwrap();
    }

    #[test]
    fn registry_register_get_remove() {
        let r = TaskRegistry::new();
        let t1 = r.register("d1");
        assert!(r.get("d1").is_some());
        r.remove("d1", &t1);
        assert!(r.get("d1").is_none(), "移除后查无");
        assert!(r.is_empty());
    }

    #[test]
    fn registry_re_register_cancels_stale_and_remove_is_scoped() {
        let r = TaskRegistry::new();
        let t1 = r.register("d1");
        let t2 = r.register("d1");
        assert!(t1.is_cancelled(), "同 doc 旧任务被防御性取消");
        // 旧 token 的 remove 不应误删新注册
        r.remove("d1", &t1);
        assert!(r.get("d1").is_some(), "旧 token remove 无效");
        r.remove("d1", &t2);
        assert!(r.get("d1").is_none());
    }

    #[test]
    fn page_batches_by_render_meta() {
        use geminus_core::model::{Block, ParagraphStyle, RenderMeta};
        use std::collections::HashMap;
        let mut blocks = Vec::new();
        let mut rm: HashMap<String, RenderMeta> = HashMap::new();
        // 页1: b1,b2；页2: b3；页3: b4,b5
        for (id, page) in [("b1", 1), ("b2", 1), ("b3", 2), ("b4", 3), ("b5", 3)] {
            blocks.push(Block::Paragraph {
                id: id.to_string(),
                content: vec![],
                style: ParagraphStyle::default(),
            });
            rm.insert(
                id.to_string(),
                RenderMeta { page_idx: page, bbox: [0.0, 0.0, 100.0, 20.0] },
            );
        }
        let doc = Document { id: "d".into(), title: "t".into(), blocks, resources: Default::default(), render_meta: rm, pdf_meta: None };
        let doc = &doc;
        let batches = page_batches(doc);
        let spans: Vec<(usize, usize, i64)> = batches.iter().map(|b| (b.from, b.to, b.pages)).collect();
        assert_eq!(spans, vec![(0, 2, 1), (2, 3, 1), (3, 5, 1)], "按页聚合");
    }

    #[test]
    fn page_batches_without_render_meta_splits_by_estimate() {
        use geminus_core::model::{Block, ParagraphStyle};
        let blocks: Vec<Block> = (0..10)
            .map(|i| Block::Paragraph {
                id: format!("b{i}"),
                content: vec![],
                style: ParagraphStyle::default(),
            })
            .collect();
        let doc = Document {
            id: "d".into(),
            title: "t".into(),
            blocks,
            resources: Default::default(),
            render_meta: HashMap::new(),
            pdf_meta: None,
        };
        let batches = page_batches(&doc);
        assert_eq!(batches.len(), doc.estimated_pages().max(1), "批数 = 估算页数");
        assert!(batches.iter().all(|b| b.pages == 1));
        assert_eq!(batches.first().unwrap().from, 0);
        assert_eq!(batches.last().unwrap().to, 10, "区间覆盖全部块");
    }
}
