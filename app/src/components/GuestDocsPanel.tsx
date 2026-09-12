import { useEffect, useState } from 'react';
import {
  listGuestDocs,
  fetchGuestUsage,
  mergeGuestDocs,
  type GuestDocRecord,
} from '../services/guestDocs';

interface Props {
  /** 点击条目：回载该文档（服务端缓存秒回已译部分，零配额消耗） */
  onOpen: (docId: string, fileName: string) => void;
  onClose: () => void;
}

function fmtTime(at: number): string {
  const d = new Date(at);
  const pad = (n: number) => `${n}`.padStart(2, '0');
  return `${d.getMonth() + 1}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/**
 * v2.4.1：游客翻译记录面板（登录用户的「🕘 历史」的游客等价物）。
 * 数据源 = 本会话记录（sessionStorage）∪ 服务端额度明细（/api/quota/usage，同 IP 24h 内）。
 * 回访链路：fetchDocument（token 防盗链放行）→ 翻译（已付费批豁免熔断，缓存秒回零消耗）。
 */
export default function GuestDocsPanel({ onOpen, onClose }: Props) {
  const [items, setItems] = useState<GuestDocRecord[] | null>(null);

  useEffect(() => {
    const local = listGuestDocs();
    fetchGuestUsage()
      .then((remote) => setItems(mergeGuestDocs(local, remote)))
      .catch(() => setItems(local));
  }, []);

  return (
    <div
      className="fixed inset-y-0 right-0 z-50 flex w-[min(90vw,22rem)] flex-col border-l border-slate-200 bg-white shadow-xl"
      data-testid="guest-docs-panel"
    >
      <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
        <h2 className="text-sm font-semibold text-slate-700">🧾 翻译记录（游客）</h2>
        <button
          onClick={onClose}
          className="rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-100"
          data-testid="guest-docs-close"
        >
          ✕ 关闭
        </button>
      </div>

      {items === null ? (
        <p className="px-4 py-6 text-xs text-slate-400">加载中…</p>
      ) : items.length === 0 ? (
        <div className="px-4 py-6 text-xs text-slate-400">
          暂无翻译记录。游客额度按网络环境计费（刷新或更换浏览器后，24 小时内的翻译仍可通过服务端记录找回）。
        </div>
      ) : (
        <>
          <p className="border-b border-slate-100 bg-slate-50 px-4 py-2 text-[11px] text-slate-400">
            点击回访——已翻译部分直接打开，不消耗剩余额度
          </p>
          <ul className="flex-1 overflow-y-auto">
            {items.map((r) => (
              <li key={r.doc_id}>
                <button
                  onClick={() => onOpen(r.doc_id, r.file_name)}
                  className="flex w-full flex-col items-start gap-0.5 border-b border-slate-100 px-4 py-2.5 text-left hover:bg-blue-50"
                  data-testid="guest-doc-item"
                >
                  <span className="max-w-full truncate text-xs font-medium text-slate-700" title={r.file_name}>
                    📄 {r.file_name}
                  </span>
                  <span className="text-[11px] text-slate-400">{fmtTime(r.at)}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
