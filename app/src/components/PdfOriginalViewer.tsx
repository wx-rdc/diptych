import { useEffect, useMemo, useRef, useState } from 'react';
import type { GDocument } from '../types';
import { usePdfRenderer } from '../hooks/usePdfRenderer';
import { contentBboxForPage, type BBox } from '../utils/geometry';
import PdfRegionOverlay, { type RegionDef } from './PdfRegionOverlay';

interface Props {
  doc: GDocument;
  /** 原始 PDF 文件 URL（/api/original/...） */
  url: string;
  activeId: string | null;
  onSelect: (blockId: string) => void;
  /** 滚动容器 ref（active 定位用） */
  containerRef?: React.RefObject<HTMLDivElement>;
  /** v2.2 T2：内容 bbox 表（page_idx → 裁切区）；父组件计算并共享给锚点同步 */
  contentBboxes?: Map<number, BBox>;
  /** v2.2 T6：页元素挂载/卸载回调（父级构建左锚点表用） */
  onPageEl?: (pageNo: number, el: HTMLDivElement | null) => void;
  /** v2.3 C1：pdf.js 致命失败（worker 加载/文档解析失败）上报——App 降级文本对照 */
  onFatalError?: (msg: string) => void;
}

/** 按页分组的区域索引 */
function groupRegionsByPage(doc: GDocument): Map<number, RegionDef[]> {
  const map = new Map<number, RegionDef[]>();
  const meta = doc.render_meta ?? {};
  for (const block of doc.blocks) {
    const m = meta[block.id];
    if (!m) continue;
    const list = map.get(m.page_idx) ?? [];
    list.push({ blockId: block.id, meta: m });
    map.set(m.page_idx, list);
  }
  return map;
}

/** v2.2 T2：全文档内容 bbox 表（每页 union+bleed，退化回整页） */
export function buildContentBboxes(doc: GDocument): Map<number, BBox> {
  const sizes = doc.pdf_meta?.page_sizes ?? [];
  const blocks = Object.entries(doc.render_meta ?? {}).map(([id, meta]) => ({
    id,
    meta: { page_idx: meta.page_idx, bbox: meta.bbox as BBox },
  }));
  const map = new Map<number, BBox>();
  const pageCount = doc.pdf_meta?.page_count ?? 0;
  for (let p = 0; p < pageCount; p++) {
    const size = sizes[p] ?? [612, 792];
    map.set(p, contentBboxForPage(blocks, p, size as [number, number]));
  }
  return map;
}

/**
 * PDF 原始页面渲染（v2）：pdf.js canvas + 区域叠加层。
 * v2.2 T2：内容裁切渲染——纸张白边被裁掉，正文列铺满内容区；
 * 每页固定宽高比占位 wrapper（内容高宽比），IntersectionObserver 懒渲染（可见 ±1 页）；
 * 测试环境（无 IO）直接渲染全部页。
 */
export default function PdfOriginalViewer({
  doc,
  url,
  activeId,
  onSelect,
  containerRef,
  contentBboxes: contentBboxesProp,
  onPageEl,
  onFatalError,
}: Props) {
  const renderer = usePdfRenderer(url);
  const regionsByPage = useMemo(() => groupRegionsByPage(doc), [doc]);
  const ownBboxes = useMemo(() => buildContentBboxes(doc), [doc]);
  const contentBboxes = contentBboxesProp ?? ownBboxes;
  const [renderedPages, setRenderedPages] = useState<Set<number>>(new Set());
  const pageRefs = useRef<Map<number, HTMLDivElement>>(new Map());

  // v2.3 C1：致命错误上报（App 切文本对照模式，左栏不再只剩红字空白）
  useEffect(() => {
    if (renderer.error) onFatalError?.(renderer.error);
  }, [renderer.error, onFatalError]);

  // 懒渲染：IntersectionObserver 标记可见页（±1 预渲染）
  useEffect(() => {
    if (!renderer.ready) return;
    if (typeof IntersectionObserver === 'undefined') {
      // jsdom：全页渲染
      setRenderedPages(new Set(renderer.pages.map((p) => p.pageNo)));
      return;
    }
    const container = containerRef?.current ?? null;
    const observer = new IntersectionObserver(
      (entries) => {
        setRenderedPages((prev) => {
          const next = new Set(prev);
          for (const entry of entries) {
            const pageNo = Number((entry.target as HTMLElement).dataset.pageNo);
            if (!pageNo) continue;
            const idx = renderer.pages.findIndex((p) => p.pageNo === pageNo);
            // 可见 → 该页 ±1 渲染
            for (const d of [-1, 0, 1]) {
              const n = idx + d;
              if (n >= 0 && n < renderer.pages.length) next.add(renderer.pages[n].pageNo);
            }
          }
          return next;
        });
      },
      { root: container, rootMargin: '200px' },
    );
    for (const el of pageRefs.current.values()) observer.observe(el);
    return () => observer.disconnect();
  }, [renderer.ready, renderer.pages, containerRef]);

  // v2.7：active 定位由 usePaneSync 的 click 居中接管（双侧统一），无滚动驱动 effect

  if (renderer.error) {
    return (
      <div className="p-6 text-sm text-red-500" data-testid="pdf-viewer-error">
        PDF 渲染失败：{renderer.error}
      </div>
    );
  }
  if (!renderer.ready) {
    return (
      <div className="p-6 text-sm text-slate-400" data-testid="pdf-viewer-loading">
        正在加载原始 PDF…
      </div>
    );
  }

  return (
    <div
      className="flex flex-col py-3"
      style={{ gap: 'var(--page-gap)' }}
      data-testid="pdf-original-viewer"
    >
      {renderer.pages.map((page) => {
        const rendered = renderedPages.has(page.pageNo);
        const regions = regionsByPage.get(page.pageNo - 1) ?? [];
        // 裁切渲染：显示区高宽比 = 内容 bbox 高宽比（无 bbox 页回退整页）
        const cb = contentBboxes.get(page.pageNo - 1) ?? null;
        const isFullPage = !cb || (cb[0] <= 0 && cb[1] <= 0 && cb[2] >= page.width && cb[3] >= page.height);
        const ratioW = cb ? cb[2] - cb[0] : page.width;
        const ratioH = cb ? cb[3] - cb[1] : page.height;
        return (
          <div key={page.pageNo}>
            {/* v2.1 T6.3：页间缝 sticky 页码标签 */}
            <div
              className="sticky top-0 z-20 bg-white/90 px-1 py-0.5 text-[10px] font-medium text-slate-400 backdrop-blur"
              data-testid={`page-label-${page.pageNo}`}
            >
              Page {page.pageNo}
            </div>
            <div
              data-page-no={page.pageNo}
              data-testid={`pdf-page-${page.pageNo}`}
              ref={(el) => {
                if (el) pageRefs.current.set(page.pageNo, el);
                else pageRefs.current.delete(page.pageNo);
                onPageEl?.(page.pageNo, el);
              }}
              className="relative w-full bg-white ring-1 ring-slate-100"
              style={{ aspectRatio: `${ratioW} / ${ratioH}` }}
            >
              {rendered ? (
                <PageCanvas
                  pageNo={page.pageNo}
                  renderPage={renderer.renderPage}
                  renderPageCrop={renderer.renderPageCrop}
                  crop={isFullPage ? undefined : { x: cb![0], y: cb![1], w: cb![2] - cb![0], h: cb![3] - cb![1] }}
                />
              ) : (
                <div className="absolute inset-0 flex items-center justify-center bg-slate-50 text-xs text-slate-300">
                  第 {page.pageNo} 页
                </div>
              )}
              {rendered ? (
                <PdfRegionOverlay
                  regions={regions}
                  activeId={activeId}
                  onBlockClick={onSelect}
                  pageSize={[page.width, page.height]}
                  contentBbox={isFullPage ? null : cb}
                />
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** 单页 canvas：挂载渲染一次 + ResizeObserver 防抖 150ms 重渲染（v2.1 T2.3）；
 * v2.2 T2：crop 存在时走内容裁切渲染（离屏全页 → drawImage 裁切） */
function PageCanvas({
  pageNo,
  renderPage,
  renderPageCrop,
  crop,
}: {
  pageNo: number;
  renderPage: (pageNo: number, canvas: HTMLCanvasElement, targetWidth: number) => Promise<void>;
  renderPageCrop: (
    pageNo: number,
    canvas: HTMLCanvasElement,
    targetWidth: number,
    crop: { x: number; y: number; w: number; h: number },
  ) => Promise<void>;
  crop?: { x: number; y: number; w: number; h: number };
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const parent = canvas?.parentElement;
    if (!canvas || !parent) return;
    let timer = 0;
    const draw = () => {
      const width = parent.clientWidth || 612;
      const task = crop ? renderPageCrop(pageNo, canvas, width, crop) : renderPage(pageNo, canvas, width);
      task.catch(() => {
        /* 渲染失败保持占位 */
      });
    };
    draw();
    if (typeof ResizeObserver === 'undefined') return; // jsdom 无 RO
    let lastW = parent.clientWidth;
    const ro = new ResizeObserver(() => {
      if (parent.clientWidth === lastW) return;
      lastW = parent.clientWidth;
      clearTimeout(timer);
      timer = window.setTimeout(draw, 150);
    });
    ro.observe(parent);
    return () => {
      ro.disconnect();
      clearTimeout(timer);
    };
  }, [pageNo, renderPage, renderPageCrop, crop]);

  return (
    <canvas
      ref={canvasRef}
      data-testid={`pdf-canvas-${pageNo}`}
      className="absolute left-0 top-0 h-full w-full"
    />
  );
}
