import type { Block, Resource } from '../types';
import InlineContentRenderer from './InlineContentRenderer';

type AlgorithmBlock = Extract<Block, { type: 'algorithm' }>;

interface Props {
  docId: string;
  block: AlgorithmBlock;
  resources: Record<string, Resource>;
  active?: boolean;
  onSelect?: (id: string) => void;
}

/**
 * v2.2 T4：算法伪代码卡片——镜像原版式。
 * 标签行（Input:/Output:/for/end for）加粗、语句悬挂缩进、▷ 注释灰斜体、
 * 行内公式 KaTeX（InlineContentRenderer）；整卡可点击高亮。
 */
export default function AlgorithmCard({ docId, block, resources, active, onSelect }: Props) {
  return (
    <div
      data-block-id={block.id}
      data-active={active ? 'true' : undefined}
      data-testid={`algorithm-card-${block.id}`}
      onClick={onSelect ? () => onSelect(block.id) : undefined}
      className={`cursor-pointer rounded border px-4 py-3 transition-colors ${
        active ? 'border-blue-400 bg-blue-500/5' : 'border-slate-300 bg-slate-50'
      }`}
    >
      {block.header ? (
        <p className="mb-2 text-[0.92em] font-bold text-slate-700">{block.header}</p>
      ) : null}
      <div
        className="space-y-0.5 font-mono"
        style={{ fontSize: 'calc(var(--reading-fs) * 0.88)' }}
      >
        {block.lines.map((line, i) => (
          <div
            key={i}
            data-testid={`algo-line-${block.id}-${i}`}
            data-kind={line.kind}
            className={
              line.kind === 'label'
                ? 'font-bold text-slate-800'
                : line.kind === 'comment'
                  ? 'pl-6 italic text-slate-400'
                  : 'pl-6 text-slate-700'
            }
          >
            {line.segments.map((seg, j) => (
              <InlineContentRenderer key={j} docId={docId} item={seg} resources={resources} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
