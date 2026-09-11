import type { Block, Resource } from '../types';
import { resourceUrl } from '../services/backend';
import FormulaRenderer from './FormulaRenderer';
import TableCard from './TableCard';

interface Props {
  docId: string;
  block: Block;
  resources: Record<string, Resource>;
}

/** 占位符渲染器：图片/表格/公式在译文侧原样显示（plan.md §4.4；表格 v2.1 富渲染） */
export default function PlaceholderRenderer({ docId, block, resources }: Props) {
  switch (block.type) {
    case 'image':
      return (
        <div className="my-3 flex justify-center" data-block-id={block.id}>
          <img
            src={resourceUrl(docId, block.resource_id)}
            width={block.width ?? undefined}
            height={block.height ?? undefined}
            alt={block.alt_text ?? '图片'}
            className="max-w-full rounded border border-slate-200 bg-white"
            loading="lazy"
          />
        </div>
      );
    case 'table':
      return <TableCard docId={docId} block={block} resources={resources} />;
    case 'formula': {
      const latex = resources[block.resource_id]?.latex ?? null;
      return (
        // v2.2 T7：py-1 保证按钮 top-1 在块内；opacity 过渡替代 hidden/block 切换（C6 闪烁根因）
        <div className="group relative py-1" data-block-id={block.id}>
          <FormulaRenderer latex={latex} displayMode />
          <button
            className="absolute right-1 top-1 z-10 rounded border border-slate-200 bg-white px-1.5 py-0.5 text-[10px] text-slate-500 opacity-0 shadow-sm transition-opacity hover:border-blue-300 hover:text-blue-600 group-hover:opacity-100"
            data-testid={`copy-latex-${block.id}`}
            title={latex ?? '无 LaTeX'}
            onClick={() => {
              void navigator.clipboard.writeText(latex ?? '');
            }}
          >
            复制 LaTeX
          </button>
        </div>
      );
    }
    default:
      return null;
  }
}
