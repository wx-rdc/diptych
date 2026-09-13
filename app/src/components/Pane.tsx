import type { ReactNode, RefObject } from 'react';

interface Props {
  /** 极淡浮标（左上角悬浮，不占布局高度；ui_design_0913 §三 栏头极简化） */
  label?: string;
  containerRef?: RefObject<HTMLDivElement>;
  children: ReactNode;
}

/** 单侧滚动面板（原文/译文共用）；栏头已移除，双栏以 1px 分隔线区隔，模式语义由顶栏分段控件表达 */
export default function Pane({ label, containerRef, children }: Props) {
  return (
    <div className="relative flex h-full min-w-0 flex-1 flex-col bg-white">
      {label ? (
        <span
          data-testid="pane-label"
          className="pointer-events-none absolute left-3 top-2 z-10 select-none text-[11px] font-medium text-slate-300"
        >
          {label}
        </span>
      ) : null}
      <div ref={containerRef} className="geminus-pane flex-1 overflow-y-auto py-3" style={{ paddingInline: 'var(--pane-pad)' }}>
        {children}
      </div>
    </div>
  );
}
