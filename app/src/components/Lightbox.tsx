import { useEffect, useState, type ReactNode } from 'react';

export interface LightboxTab {
  key: string;
  label: string;
  content: ReactNode;
}

interface Props {
  tabs: LightboxTab[];
  onClose: () => void;
  title?: string;
}

/** 全屏 Lightbox：ESC/遮罩点击关闭，tab 切换（v2.1 T3.3 富渲染|原文截图双视图） */
export default function Lightbox({ tabs, onClose, title }: Props) {
  const [active, setActive] = useState(tabs[0]?.key ?? '');

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex flex-col bg-black/60 p-6"
      data-testid="lightbox"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="mx-auto flex h-full w-full max-w-5xl flex-col rounded-lg bg-white shadow-xl">
        <div className="flex items-center justify-between border-b border-slate-200 px-4 py-2">
          <span className="text-sm font-semibold text-slate-700">{title ?? '详情'}</span>
          <button
            onClick={onClose}
            className="rounded px-2 py-1 text-sm text-slate-500 hover:bg-slate-100"
            data-testid="lightbox-close"
          >
            ✕ 关闭
          </button>
        </div>
        <div className="flex gap-1 border-b border-slate-200 px-4 pt-2" role="tablist">
          {tabs.map((t) => (
            <button
              key={t.key}
              role="tab"
              aria-selected={active === t.key}
              data-testid={`lightbox-tab-${t.key}`}
              onClick={() => setActive(t.key)}
              className={
                'rounded-t px-3 py-1.5 text-sm ' +
                (active === t.key
                  ? 'bg-slate-100 font-semibold text-slate-800'
                  : 'text-slate-500 hover:text-slate-700')
              }
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="flex-1 overflow-auto p-4">{tabs.find((t) => t.key === active)?.content}</div>
      </div>
    </div>
  );
}
