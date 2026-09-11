import { useState } from 'react';
import type { GDocument, TranslateProgress } from '../types';
import { cancelTranslate } from '../services/backend';

interface Props {
  doc: GDocument;
  translating: boolean;
  /** v2.4 T5：暂停态（用户停止/断开）→ [继续翻译] */
  paused: boolean;
  /** 完成态（重新翻译入口） */
  done: boolean;
  progress: TranslateProgress | null;
  onContinue: () => void;
  onRetranslate: () => void;
}

/**
 * v2.4 T6：翻译控制条（替代被移除的工具栏「开始翻译」按钮）。
 * - 翻译中：进度条 + [⏹ 停止]（POST cancel → SSE cancelled → 暂停态）
 * - 暂停态：[▶ 继续翻译]（缓存秒回 + 增量续译）
 * - 完成态：[↻ 重新翻译]（换语言后重译）
 */
export default function TranslateControl({
  doc,
  translating,
  paused,
  done,
  progress,
  onContinue,
  onRetranslate,
}: Props) {
  const [stopping, setStopping] = useState(false);
  if (!translating && !paused && !done) return null;

  const current = progress?.current ?? 0;
  const total = progress?.total ?? 0;
  const pct = total > 0 ? Math.round((current / total) * 100) : null;

  const handleStop = async () => {
    setStopping(true);
    try {
      await cancelTranslate(doc.id);
      // 状态切换由 SSE cancelled 事件驱动（TranslateCancelledError → paused）
    } finally {
      setStopping(false);
    }
  };

  return (
    <div
      className="flex items-center gap-3 border-t border-slate-200 bg-white px-4 py-1.5 text-xs text-slate-600"
      data-testid="translate-control"
    >
      {translating ? (
        <>
          <div className="h-1.5 w-40 overflow-hidden rounded bg-slate-200" data-testid="translate-progress-bar">
            <div
              className="h-full bg-blue-500 transition-all"
              style={{ width: pct != null ? `${pct}%` : '0%' }}
            />
          </div>
          <span className="tabular-nums" data-testid="translate-progress-text">
            翻译中 {current}/{total}
            {pct != null ? `（${pct}%）` : ''}
          </span>
          <button
            onClick={handleStop}
            disabled={stopping}
            className="rounded border border-red-300 bg-red-50 px-2 py-1 font-medium text-red-600 hover:bg-red-100 disabled:opacity-40"
            data-testid="translate-stop-btn"
          >
            {stopping ? '停止中…' : '⏹ 停止'}
          </button>
        </>
      ) : paused ? (
        <>
          <span className="text-amber-600" data-testid="translate-paused-hint">
            翻译已暂停——已译部分可阅读
          </span>
          <button
            onClick={onContinue}
            className="rounded bg-blue-600 px-3 py-1 font-medium text-white hover:bg-blue-500"
            data-testid="translate-continue-btn"
          >
            ▶ 继续翻译
          </button>
        </>
      ) : (
        <>
          <span data-testid="translate-done-hint">翻译完成</span>
          <button
            onClick={onRetranslate}
            className="rounded border border-slate-300 bg-white px-3 py-1 font-medium text-slate-600 hover:bg-slate-100"
            data-testid="translate-redo-btn"
          >
            ↻ 重新翻译
          </button>
        </>
      )}
    </div>
  );
}
