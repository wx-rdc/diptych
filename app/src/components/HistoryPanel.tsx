import type { HistoryItem } from '../services/history';

interface Props {
  items: HistoryItem[];
  loading: boolean;
  onOpen: (item: HistoryItem) => void;
  onClose: () => void;
}

function statusLabel(s: string): string {
  return s === 'translated' ? '已翻译' : '已解析';
}

function timeLabel(unix: number): string {
  const d = new Date(unix * 1000);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 「我的文档」面板（v2.4 T2）：登录用户历史，点击直接回载阅读器 */
export default function HistoryPanel({ items, loading, onOpen, onClose }: Props) {
  return (
    <div
      className="absolute right-2 top-12 z-40 w-[22rem] rounded-lg border border-slate-200 bg-white shadow-xl"
      data-testid="history-panel"
    >
      <div className="flex items-center justify-between border-b border-slate-100 px-4 py-2">
        <h3 className="text-sm font-bold text-slate-700">🕘 我的文档</h3>
        <button
          onClick={onClose}
          className="rounded px-2 text-slate-400 hover:bg-slate-100 hover:text-slate-600"
          aria-label="关闭历史面板"
          data-testid="history-close"
        >
          ✕
        </button>
      </div>
      <div className="max-h-[60vh] overflow-y-auto">
        {loading ? (
          <p className="px-4 py-6 text-center text-xs text-slate-400" data-testid="history-loading">
            加载中…
          </p>
        ) : items.length === 0 ? (
          <p className="px-4 py-6 text-center text-xs text-slate-400" data-testid="history-empty">
            还没有文档，上传一个试试
          </p>
        ) : (
          items.map((item) => (
            <button
              key={item.doc_id}
              onClick={() => onOpen(item)}
              className="flex w-full items-center gap-2 border-b border-slate-50 px-4 py-2 text-left hover:bg-slate-50"
              data-testid={`history-item-${item.doc_id}`}
            >
              <span className="shrink-0 text-xs font-bold text-slate-400">
                {item.file_type === 'pdf' ? '📄' : '📝'}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs font-medium text-slate-700" title={item.title}>
                  {item.file_name}
                </span>
                <span className="block text-[10px] text-slate-400">
                  {timeLabel(item.uploaded_at)} · {item.pages} 页
                </span>
              </span>
              <span className="shrink-0 rounded-full bg-slate-100 px-2 py-0.5 text-[10px] text-slate-500">
                {statusLabel(item.status)}
              </span>
            </button>
          ))
        )}
      </div>
    </div>
  );
}
