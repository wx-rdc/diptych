import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { Block, GDocument } from '../types';

// 调试钩子类型（?debugAnchors=1 时挂载，E2E 诊断用）
declare global {
  interface Window {
    __anchors?: unknown;
  }
}
import { originalPdfUrl } from '../services/backend';
import BlockRenderer from './BlockRenderer';
import Pane from './Pane';
import PdfOriginalViewer, { buildContentBboxes } from './PdfOriginalViewer';
import { usePaneSync } from '../hooks/usePaneSync';
import { buildLeftAnchors, buildRightAnchors, type Anchor } from '../utils/anchor';

interface Props {
  doc: GDocument;
  translatedMap: Record<string, Block | null>;
  activeId: string | null;
  onSelect: (blockId: string) => void;
  originalRef?: React.RefObject<HTMLDivElement>;
  translatedRef?: React.RefObject<HTMLDivElement>;
  /** v2.1：翻译失败回退原文的块（右栏「未翻译」徽标） */
  fallbackIds?: Set<string>;
  /** v2.1：译文侧段落交互（hover 工具条） */
  interactive?: import('./BlockRenderer').ParagraphToolbarInteractive;
  /** v2.3 C1：左栏 pdf.js 致命失败上报（App 自动降级文本对照 + 横幅重试） */
  onOriginalFatalError?: (msg: string) => void;
}

/**
 * 原始渲染模式（v2）：左 pdf.js 原始页面 + 区域叠加，右译文。
 * v2.7：交互由 usePaneSync 提供（hover 联动高亮 + click 双侧居中；滚动完全独立），
 * v2.4 的 hover/滚动驱动自动对齐已移除；锚点表保留构建（__anchors 调试钩子）。
 */
export default function PdfCompareView({
  doc,
  translatedMap,
  activeId,
  onSelect,
  originalRef,
  translatedRef,
  fallbackIds,
  interactive,
  onOriginalFatalError,
}: Props) {
  const originalRefInner = useRef<HTMLDivElement>(null);
  const translatedRefInner = useRef<HTMLDivElement>(null);
  const leftRef = originalRef ?? originalRefInner;
  const rightRef = translatedRef ?? translatedRefInner;

  const leftAnchorsRef = useRef<Anchor[]>([]);
  const rightAnchorsRef = useRef<Anchor[]>([]);
  const pageElsRef = useRef<Map<number, HTMLDivElement>>(new Map());

  usePaneSync(leftRef, rightRef);

  // 块 → 页号索引（右侧段落定位用）
  const blockPage = (blockId: string): number | null => doc.render_meta?.[blockId]?.page_idx ?? null;

  // 内容 bbox 表（父级计算，与 PdfOriginalViewer/锚点共用——T2 裁切的几何基）
  const contentBboxes = useMemo(() => buildContentBboxes(doc), [doc]);

  const rebuildAnchors = useCallback(() => {
    const left = leftRef.current;
    const right = rightRef.current;
    if (!left || !right) return;
    leftAnchorsRef.current = buildLeftAnchors(doc, contentBboxes, pageElsRef.current);
    rightAnchorsRef.current = buildRightAnchors(doc, right);
    // 调试钩子（常开，E2E/开发诊断用）：最近一次锚点表快照
    if (typeof window !== 'undefined') {
      window.__anchors = {
        left: leftAnchorsRef.current,
        right: rightAnchorsRef.current,
        pageEls: [...pageElsRef.current.keys()],
        contentBboxes: [...contentBboxes.entries()],
      };
    }
  }, [doc, contentBboxes, leftRef, rightRef]);

  // 渲染完成后构建锚点；懒渲染二次校准（800ms 后 canvas 就位）
  useEffect(() => {
    rebuildAnchors();
    const t = window.setTimeout(rebuildAnchors, 800);
    return () => window.clearTimeout(t);
  }, [rebuildAnchors, translatedMap]);

  const translatedBlocks = doc.blocks.map((b) => translatedMap[b.id] || b);

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden" data-testid="pdf-compare-view">
      <Pane label="原文" containerRef={leftRef}>
        <PdfOriginalViewer
          doc={doc}
          url={originalPdfUrl(doc.id)}
          activeId={activeId}
          onSelect={onSelect}
          containerRef={leftRef}
          contentBboxes={contentBboxes}
          onFatalError={onOriginalFatalError}
          onPageEl={(pageNo, el) => {
            if (el) pageElsRef.current.set(pageNo, el);
            else pageElsRef.current.delete(pageNo);
          }}
        />
      </Pane>

      <div className="w-px bg-slate-200" />

      <Pane label="译文" containerRef={rightRef}>
        {translatedBlocks.map((block, i) => {
          const page = blockPage(block.id);
          const prev = i > 0 ? blockPage(translatedBlocks[i - 1].id) : null;
          return (
            <div key={block.id} className="reading-block" data-block-id={block.id} data-page={page ?? undefined}>
              {page !== null && page !== prev ? (
                <div
                  className="flex items-center gap-2 text-[10px] text-slate-300"
                  style={{ marginBlock: 'var(--page-gap)' }}
                  data-testid={`translated-page-divider-${page}`}
                >
                  <span className="h-px flex-1 bg-slate-200" /> 第 {page + 1} 页{' '}
                  <span className="h-px flex-1 bg-slate-200" />
                </div>
              ) : null}
              <BlockRenderer
                docId={doc.id}
                block={block}
                resources={doc.resources}
                activeId={activeId}
                onSelect={onSelect}
                badgeFallback={fallbackIds?.has(block.id)}
                interactive={interactive && block.type === 'paragraph' ? interactive : undefined}
              />
            </div>
          );
        })}
      </Pane>
    </div>
  );
}
