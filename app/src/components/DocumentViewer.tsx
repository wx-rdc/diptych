import { useRef } from 'react';
import type { Block, GDocument } from '../types';
import BlockRenderer from './BlockRenderer';
import Pane from './Pane';
import { usePaneSync } from '../hooks/usePaneSync';

interface Props {
  doc: GDocument;
  /** block_id → 译文块（null/缺失 = 未翻译，显示原文） */
  translatedMap: Record<string, Block | null>;
  activeId: string | null;
  onSelect: (blockId: string) => void;
  originalRef?: React.RefObject<HTMLDivElement>;
  translatedRef?: React.RefObject<HTMLDivElement>;
  /** v2.1：翻译失败回退原文的块（右栏「未翻译」徽标） */
  fallbackIds?: Set<string>;
  /** v2.1：译文侧段落交互（hover 工具条） */
  interactive?: import('./BlockRenderer').ParagraphToolbarInteractive;
}

/** 双栏对照视图：左原文 / 右译文，占位符保位。
 * v2.7：交互由 usePaneSync 提供（hover 联动高亮 + click 双侧居中；滚动完全独立）。
 * v2.6：栏头移除——浮标由 Pane 渲染，类型徽章在顶栏文档身份区。 */
export default function DocumentViewer({
  doc,
  translatedMap,
  activeId,
  onSelect,
  originalRef,
  translatedRef,
  fallbackIds,
  interactive,
}: Props) {
  const originalInner = useRef<HTMLDivElement>(null);
  const translatedInner = useRef<HTMLDivElement>(null);
  const leftRef = originalRef ?? originalInner;
  const rightRef = translatedRef ?? translatedInner;

  usePaneSync(leftRef, rightRef);

  const translatedBlocks = doc.blocks.map((b) => translatedMap[b.id] || b);

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden" data-testid="document-viewer">
      <Pane label="原文" containerRef={leftRef}>
        {doc.blocks.map((block) => (
          <BlockRenderer
            key={block.id}
            docId={doc.id}
            block={block}
            resources={doc.resources}
            activeId={activeId}
            onSelect={onSelect}
          />
        ))}
      </Pane>

      <div className="w-px bg-slate-200" />

      <Pane label="译文" containerRef={rightRef}>
        {translatedBlocks.map((block) => (
          <BlockRenderer
            key={block.id}
            docId={doc.id}
            block={block}
            resources={doc.resources}
            activeId={activeId}
            onSelect={onSelect}
            badgeFallback={fallbackIds?.has(block.id)}
            interactive={interactive && block.type === 'paragraph' ? interactive : undefined}
          />
        ))}
      </Pane>
    </div>
  );
}
