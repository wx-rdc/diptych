import Popover from '../Popover';

interface Props {
  hasDoc: boolean;
  /** 解析阶段完整文案（progressLabel 产物；null = 非解析中） */
  parseLabel: string | null;
  translating: boolean;
  paused: boolean;
  /** 本轮翻译完成（含部分失败） */
  done: boolean;
  stats: { done: number; cached: number; errors: number };
  current: number;
  total: number;
  onStop: () => void;
  onContinue: () => void;
  onRetryFailed: () => void;
  onRetranslate: () => void;
}

type State = 'parsing' | 'translating' | 'paused' | 'warning' | 'done';

/**
 * 状态单一出口（ui_design_0913 §三③/§四）：
 * Chip 承载 解析/进度/完成/失败/暂停 五态；计数细节与动作收进 Popover。
 * （原底部信息栏 4 个技术计数 + TranslateControl 的停止/继续/重译全部收编于此）
 */
export default function StatusChip(p: Props) {
  const state: State | null = p.translating
    ? 'translating'
    : p.paused
      ? 'paused'
      : p.done
        ? p.stats.errors > 0 ? 'warning' : 'done'
        : p.parseLabel
          ? 'parsing'
          : null;
  if (!state) return null; // 空闲不占位（切换文档的瞬态）

  const label =
    state === 'translating' ? `⟳ ${p.current}/${p.total}` :
    state === 'parsing' ? '⟳ 解析中' :
    state === 'paused' ? '⏸ 已暂停' :
    state === 'warning' ? `⚠ ${p.stats.errors} 段` :
    '✓';

  const tone =
    state === 'done' ? 'bg-emerald-50 text-emerald-600' :
    state === 'warning' || state === 'paused' ? 'bg-amber-50 text-amber-700' :
    'bg-blue-50 text-blue-600';

  const summary = [
    `完成 ${p.stats.done}`,
    ...(p.stats.cached > 0 ? [`缓存 ${p.stats.cached}`] : []), // 缓存命中 >0 才显示（降噪）
    ...(p.stats.errors > 0 ? [`失败 ${p.stats.errors}`] : []),
  ].join(' · ');

  return (
    <Popover
      panelTestId="status-popover"
      panelClassName="w-60"
      trigger={({ toggle }) => (
        <button
          onClick={toggle}
          data-testid="status-chip"
          data-state={state}
          title={state === 'parsing' ? (p.parseLabel ?? undefined) : `翻译状态：${summary}`}
          className={`flex h-6 shrink-0 items-center gap-1 rounded-full px-2 text-[11px] font-medium tabular-nums hover:brightness-95 ${tone}`}
        >
          {label}
        </button>
      )}
      panel={(close) => (
        <div>
          <p className="mb-1 text-[11px] font-semibold text-slate-400">翻译状态</p>
          {state === 'parsing' ? (
            <p className="text-xs text-slate-600">{p.parseLabel}</p>
          ) : (
            <>
              <p className="mb-2 text-xs text-slate-700" data-testid="status-summary">{summary}</p>
              <div className="flex flex-wrap gap-1.5 border-t border-slate-100 pt-2">
                {state === 'translating' ? (
                  <button
                    onClick={() => { p.onStop(); close(); }}
                    data-testid="status-stop-btn"
                    className="rounded border border-red-200 bg-red-50 px-2 py-1 font-medium text-red-600 hover:bg-red-100"
                  >
                    ⏹ 停止翻译
                  </button>
                ) : null}
                {state === 'paused' ? (
                  <button
                    onClick={() => { p.onContinue(); close(); }}
                    data-testid="status-continue-btn"
                    className="rounded bg-blue-600 px-2 py-1 font-medium text-white hover:bg-blue-500"
                  >
                    ▶ 继续翻译
                  </button>
                ) : null}
                {state !== 'translating' && p.stats.errors > 0 ? (
                  <button
                    onClick={() => { p.onRetryFailed(); close(); }}
                    data-testid="status-retry-failed-btn"
                    className="rounded border border-amber-300 bg-amber-50 px-2 py-1 font-medium text-amber-700 hover:bg-amber-100"
                  >
                    ↻ 重试失败段
                  </button>
                ) : null}
                {state !== 'translating' && p.hasDoc ? (
                  <button
                    onClick={() => { p.onRetranslate(); close(); }}
                    data-testid="status-retranslate-btn"
                    className="rounded border border-slate-300 px-2 py-1 font-medium text-slate-600 hover:bg-slate-100"
                  >
                    ↻ 重新翻译全文
                  </button>
                ) : null}
              </div>
            </>
          )}
        </div>
      )}
    />
  );
}
