import type { TranslateProgress } from '../types';

interface Props {
  progress: TranslateProgress | null;
  translating: boolean;
  stats: { done: number; cached: number; errors: number };
  error: string | null;
}

/** 状态栏：翻译进度条、缓存命中、错误（plan.md 组件树） */
export default function StatusBar({ progress, translating, stats, error }: Props) {
  const percent =
    progress && progress.total > 0 ? Math.round((progress.current / progress.total) * 100) : 0;

  return (
    <footer className="flex items-center gap-4 border-t border-slate-200 bg-slate-50 px-4 py-1.5 text-xs text-slate-600">
      <div className="flex min-w-[12rem] flex-1 items-center gap-2">
        <div className="h-2 flex-1 overflow-hidden rounded-full bg-slate-200">
          <div
            className="h-full rounded-full bg-blue-500 transition-all duration-300"
            style={{ width: `${percent}%` }}
            data-testid="progress-bar"
          />
        </div>
        <span data-testid="progress-text">
          {translating || progress ? `${percent}%（${progress?.current ?? 0}/${progress?.total ?? 0}）` : '空闲'}
        </span>
      </div>
      <span>✅ 完成 {stats.done}</span>
      <span>💾 缓存命中 {stats.cached}</span>
      <span className={stats.errors > 0 ? 'text-red-600' : ''}>⚠️ 失败 {stats.errors}</span>
      {error ? (
        <span className="max-w-[24rem] truncate text-red-600" title={error} data-testid="status-error">
          {error}
        </span>
      ) : null}
    </footer>
  );
}
