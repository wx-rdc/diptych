import { useEffect } from 'react';

interface Props {
  message: string | null;
  onDismiss: () => void;
  /** 自动消失时长 */
  duration?: number;
}

/** 瞬时错误 Toast（ui_design_0913 §三：底部栏移除后错误信息的出口） */
export default function Toast({ message, onDismiss, duration = 6000 }: Props) {
  useEffect(() => {
    if (message == null) return;
    const t = window.setTimeout(onDismiss, duration);
    return () => window.clearTimeout(t);
  }, [message, duration, onDismiss]);

  if (message == null) return null;
  return (
    <div
      data-testid="toast"
      role="alert"
      className="fixed left-1/2 top-14 z-50 flex max-w-md -translate-x-1/2 items-center gap-2 rounded-lg bg-red-600 px-3 py-2 text-xs font-medium text-white shadow-lg"
    >
      <span className="truncate" title={message}>{message}</span>
      <button onClick={onDismiss} data-testid="toast-dismiss" aria-label="关闭" className="shrink-0 rounded px-1 text-base leading-none hover:bg-red-500">
        ×
      </button>
    </div>
  );
}
