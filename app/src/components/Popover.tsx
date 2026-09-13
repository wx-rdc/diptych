import { useEffect, useRef, useState, type ReactNode } from 'react';

interface Props {
  /** 触发器：注入 isOpen 与 toggle */
  trigger: (o: { isOpen: boolean; toggle: () => void }) => ReactNode;
  /** 面板内容：注入 close() 供动作按钮收起 */
  panel: (close: () => void) => ReactNode;
  /** 面板容器 testid（断言用） */
  panelTestId: string;
  /** 弹层水平对齐（默认右对齐——顶栏右侧控件） */
  align?: 'left' | 'right';
  /** 面板宽度等 class 覆写（默认 w-64） */
  panelClassName?: string;
}

/** 轻量弹出层（ui_design_0913 §四）：Esc / 点击外部关闭；无依赖、受控于内部 state */
export default function Popover({ trigger, panel, panelTestId, align = 'right', panelClassName = 'w-64' }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div className="relative" ref={rootRef}>
      {trigger({ isOpen: open, toggle: () => setOpen((v) => !v) })}
      {open ? (
        <div
          role="dialog"
          data-testid={panelTestId}
          className={`absolute top-full z-40 mt-1.5 ${panelClassName} rounded-lg border border-slate-200 bg-white p-3 text-xs text-slate-600 shadow-lg ${align === 'right' ? 'right-0' : 'left-0'}`}
        >
          {panel(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  );
}
