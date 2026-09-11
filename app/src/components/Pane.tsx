import type { ReactNode, RefObject } from 'react';

interface Props {
  title: string;
  subtitle?: string;
  /** 文件类型徽章（[PDF] / [DOCX]） */
  badge?: string | null;
  containerRef?: RefObject<HTMLDivElement>;
  children: ReactNode;
}

/** 单侧滚动面板（原文/译文共用）；v2.4：滚动同步由 useHoverDrivenSync 在容器上接管 */
export default function Pane({ title, subtitle, badge, containerRef, children }: Props) {
  return (
    <div className="flex h-full min-w-0 flex-1 flex-col bg-white">
      <header className="flex items-baseline justify-between border-b border-slate-200 bg-slate-50 px-4 py-2">
        <h2 className="text-sm font-semibold text-slate-700">
          {title}
          {badge ? (
            <span
              data-testid="file-type-badge"
              className="ml-2 rounded bg-slate-200 px-1.5 py-0.5 align-middle text-[10px] font-bold tracking-wide text-slate-600"
            >
              {badge}
            </span>
          ) : null}
        </h2>
        {subtitle ? <span className="text-xs text-slate-400">{subtitle}</span> : null}
      </header>
      <div ref={containerRef} className="geminus-pane flex-1 overflow-y-auto py-3" style={{ paddingInline: 'var(--pane-pad)' }}>
        {children}
      </div>
    </div>
  );
}
