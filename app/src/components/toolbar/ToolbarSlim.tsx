import { useRef } from 'react';
import type { ParseProgress, RenderMode } from '../../types';
import StatusChip from './StatusChip';
import SettingsPopover from './SettingsPopover';
import QuotaChip from './QuotaChip';

/** MinerU 解析进度文案（自旧 Toolbar 迁入，语义不变） */
export function progressLabel(p: ParseProgress): string {
  switch (p.stage) {
    case 'splitting':
      return `拆分为 ${p.chunks} 个片段…`;
    case 'uploading':
      return `上传片段 ${p.current ?? 0}/${p.total ?? '?'}…`;
    case 'parsing':
      return `云端解析 ${p.done_files ?? 0}/${p.total_files ?? '?'} 文件${
        p.pages_total ? `（${p.pages_done}/${p.pages_total} 页）` : ''
      }…`;
    case 'merging':
      return '合并解析结果…';
    case 'fallback':
      return '已回退内置解析';
    case 'cached':
      return '解析缓存命中';
  }
}

interface Props {
  docTitle: string | null;
  fileType: 'docx' | 'pdf' | null;
  onPickFile: (file: File) => void;
  // ③ 状态
  parseLabel: string | null;
  translating: boolean;
  paused: boolean;
  done: boolean;
  stats: { done: number; cached: number; errors: number };
  progressCurrent: number;
  progressTotal: number;
  onStopTranslate: () => void;
  onContinueTranslate: () => void;
  onRetryFailed: () => void;
  onRetranslate: () => void;
  // ④ 视图
  renderMode: RenderMode;
  onRenderModeChange: (m: RenderMode) => void;
  canToggleRenderMode: boolean;
  // ⑤ 设置
  fontSize: number;
  onFontIncrease: () => void;
  onFontDecrease: () => void;
  sourceLang: string;
  targetLang: string;
  onSourceLangChange: (v: string) => void;
  onTargetLangChange: (v: string) => void;
  devPath: string;
  onDevPathChange: (v: string) => void;
  onParseLocal: () => void;
  // ⑥⑦ 配额/账户
  username: string | null;
  onLoginClick: () => void;
  onLogoutClick: () => void;
  onHistoryClick?: () => void;
  onViewUsage?: () => void;
}

/**
 * 单行工具栏（ui_design_0913 §三）：48px 高、三区分治（身份/文档 ｜ 主操作/状态 ｜ 视图/账户），
 * 区际留白分隔；翻译进行时下缘 2px 主色进度条。
 */
export default function ToolbarSlim(p: Props) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const pct =
    p.translating && p.progressTotal > 0
      ? Math.round((p.progressCurrent / p.progressTotal) * 100)
      : null;

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-3 border-b border-slate-200 bg-white px-3">
      {/* ① 身份/文档 */}
      <span className="shrink-0 text-base leading-none" title="geminus">🦊</span>
      {p.docTitle ? (
        <span className="min-w-0 max-w-[16rem] truncate text-sm text-slate-700" title={p.docTitle}>
          {p.docTitle}
          {p.fileType ? (
            <span
              data-testid="file-type-badge"
              className="ml-1.5 rounded bg-slate-100 px-1 py-0.5 align-middle text-[10px] font-bold tracking-wide text-slate-500"
            >
              {p.fileType.toUpperCase()}
            </span>
          ) : null}
        </span>
      ) : (
        <span className="text-sm font-semibold text-slate-800">geminus</span>
      )}

      {/* ② 主操作：唯一实心主色按钮 */}
      <input
        type="file"
        accept=".docx,.pdf"
        ref={fileInputRef}
        className="hidden"
        data-testid="upload-input"
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) p.onPickFile(f);
          e.target.value = ''; // 允许重复选择同一文件
        }}
      />
      <button
        onClick={() => fileInputRef.current?.click()}
        data-testid="upload-btn"
        title="打开 .docx / .pdf 文件，自动解析并翻译"
        className="h-8 shrink-0 rounded-lg bg-blue-600 px-3 text-xs font-medium text-white hover:bg-blue-500"
      >
        ＋ 打开
      </button>

      {/* ③ 状态 */}
      <StatusChip
        hasDoc={!!p.docTitle}
        parseLabel={p.parseLabel}
        translating={p.translating}
        paused={p.paused}
        done={p.done}
        stats={p.stats}
        current={p.progressCurrent}
        total={p.progressTotal}
        onStop={p.onStopTranslate}
        onContinue={p.onContinueTranslate}
        onRetryFailed={p.onRetryFailed}
        onRetranslate={p.onRetranslate}
      />

      <div className="flex-1" />

      {/* ④ 视图：分段控件（原始│对照） */}
      {p.canToggleRenderMode ? (
        <div className="flex h-8 shrink-0 items-center rounded-lg bg-slate-100 p-0.5 text-xs" data-testid="render-mode-toggle">
          <button
            onClick={() => p.onRenderModeChange('original')}
            data-testid="mode-original-btn"
            className={'h-7 rounded-md px-2.5 ' + (p.renderMode === 'original' ? 'bg-white font-medium text-blue-700 shadow-sm' : 'text-slate-500 hover:text-slate-700')}
          >
            原始
          </button>
          <button
            onClick={() => p.onRenderModeChange('text')}
            data-testid="mode-text-btn"
            className={'h-7 rounded-md px-2.5 ' + (p.renderMode === 'text' ? 'bg-white font-medium text-blue-700 shadow-sm' : 'text-slate-500 hover:text-slate-700')}
          >
            对照
          </button>
        </div>
      ) : null}

      {/* ⑤ 设置 */}
      <SettingsPopover
        fontSize={p.fontSize}
        onFontIncrease={p.onFontIncrease}
        onFontDecrease={p.onFontDecrease}
        sourceLang={p.sourceLang}
        targetLang={p.targetLang}
        onSourceLangChange={p.onSourceLangChange}
        onTargetLangChange={p.onTargetLangChange}
        devPath={p.devPath}
        onDevPathChange={p.onDevPathChange}
        onParseLocal={p.onParseLocal}
      />

      {/* ⑥ 配额 */}
      <QuotaChip onLoginClick={p.onLoginClick} onViewUsage={p.onViewUsage} />

      {/* ⑦ 账户 */}
      {p.username ? (
        <div className="flex shrink-0 items-center gap-1" data-testid="auth-area">
          <span
            className="max-w-[10rem] truncate rounded-full bg-emerald-50 px-2.5 py-1 text-xs text-emerald-700"
            title={p.username}
            data-testid="auth-username"
          >
            👤 {p.username}
          </span>
          <button
            onClick={p.onHistoryClick}
            className="flex h-8 w-8 items-center justify-center rounded-lg text-sm text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            data-testid="history-btn"
            title="我的文档"
          >
            🕘
          </button>
          <button
            onClick={p.onLogoutClick}
            className="rounded-lg px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            data-testid="logout-btn"
          >
            退出
          </button>
        </div>
      ) : (
        <div className="flex shrink-0 items-center gap-1" data-testid="auth-area">
          <button
            onClick={p.onLoginClick}
            className="rounded-lg border border-blue-300 bg-blue-50 px-2.5 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
            data-testid="login-btn"
          >
            登录
          </button>
        </div>
      )}

      {/* 翻译进行时：顶栏下缘 2px 进度条（完成即隐藏） */}
      {pct != null ? (
        <div
          data-testid="top-progress-bar"
          className="absolute bottom-0 left-0 h-0.5 bg-blue-600 transition-all duration-300"
          style={{ width: `${pct}%` }}
        />
      ) : null}
    </header>
  );
}
