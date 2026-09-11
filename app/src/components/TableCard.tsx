import { useMemo, useState } from 'react';
import type { Block, Resource } from '../types';
import { resourceUrl } from '../services/backend';
import { sanitizeTableHtml, tableShape } from '../utils/tableHtml';
import Lightbox from './Lightbox';
import RichTable from './RichTable';

interface Props {
  docId: string;
  block: Extract<Block, { type: 'table' }>;
  resources: Record<string, Resource>;
}

/**
 * 表格卡片（v2.1 T3.3 + T5）：
 * 降级链 raw_xml(HTML 富渲染) → {rid}_shot 原文截图 → preview_text 纯文本；
 * 折叠预览（>8 行渐隐遮罩）+ [展开] Lightbox 双 tab。
 */
export default function TableCard({ docId, block, resources }: Props) {
  const rawHtml = resources[block.resource_id]?.raw_xml ?? '';
  const html = useMemo(() => sanitizeTableHtml(rawHtml), [rawHtml]);
  const shape = useMemo(() => tableShape(rawHtml), [rawHtml]);
  // v2 已将 MinerU 表格截图存为 {rid}_shot 图片资源
  const hasShot = resources[`${block.resource_id}_shot`] !== undefined;
  const shotUrl = hasShot ? resourceUrl(docId, `${block.resource_id}_shot`) : null;
  const [open, setOpen] = useState(false);

  if (!html) {
    return (
      <div
        className="my-3 rounded-lg border border-slate-300 bg-slate-50 p-3"
        data-block-id={block.id}
        data-testid={`table-card-${block.id}`}
      >
        <div className="mb-2 text-sm font-medium text-slate-600">📊 表格（保留原文）</div>
        {shotUrl ? (
          <img
            src={shotUrl}
            alt="表格原文截图"
            className="max-w-full rounded border border-slate-200"
            data-testid={`table-shot-${block.id}`}
            loading="lazy"
          />
        ) : block.preview_text ? (
          <div
            className="whitespace-pre-wrap break-all text-xs text-slate-500"
            data-testid={`table-preview-${block.id}`}
          >
            {block.preview_text}
          </div>
        ) : (
          <div className="text-xs text-slate-400">（无表格数据）</div>
        )}
      </div>
    );
  }

  return (
    <div
      className="my-3 rounded-lg border border-slate-300 bg-white"
      data-block-id={block.id}
      data-testid={`table-card-${block.id}`}
    >
      <div className="flex items-center justify-between gap-2 border-b border-slate-200 bg-slate-50 px-3 py-1.5">
        <span className="text-sm font-medium text-slate-600">📊 表格（保留原文）</span>
        <span className="text-xs text-slate-400" data-testid={`table-shape-${block.id}`}>
          {shape.rows} 行 × {shape.cols} 列
        </span>
        <button
          onClick={() => setOpen(true)}
          className="rounded border border-slate-300 px-2 py-0.5 text-xs text-slate-600 hover:bg-slate-100"
          data-testid={`table-expand-${block.id}`}
        >
          展开
        </button>
      </div>
      <div
        className={`relative p-2 ${shape.rows > 8 ? 'max-h-56 overflow-hidden' : ''}`}
        data-testid={`table-body-${block.id}`}
      >
        <RichTable html={html} />
        {shape.rows > 8 ? (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-16 bg-gradient-to-t from-white to-transparent" />
        ) : null}
      </div>
      {open ? (
        <Lightbox
          title={`表格（${shape.rows} 行 × ${shape.cols} 列）`}
          onClose={() => setOpen(false)}
          tabs={[
            { key: 'rich', label: '富渲染', content: <RichTable html={html} /> },
            ...(shotUrl
              ? [
                  {
                    key: 'shot',
                    label: '原文截图',
                    content: <img src={shotUrl} alt="表格原文截图" className="mx-auto max-w-full" />,
                  },
                ]
              : []),
          ]}
        />
      ) : null}
    </div>
  );
}
