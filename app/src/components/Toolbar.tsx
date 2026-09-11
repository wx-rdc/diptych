import { useRef } from 'react';
import type { ParseProgress, RenderMode } from '../types';

interface Props {
  filePath: string;
  onFilePathChange: (v: string) => void;
  onParse: () => void;
  parsing: boolean;
  /** v2：MinerU 解析进度（SSE），null 无任务 */
  parseProgress: ParseProgress | null;
  /** v2：渲染模式切换（仅 PDF + pdf_meta 时显示） */
  renderMode: RenderMode;
  onRenderModeChange: (m: RenderMode) => void;
  canToggleRenderMode: boolean;
  docTitle: string | null;
  sourceLang: string;
  targetLang: string;
  onSourceLangChange: (v: string) => void;
  onTargetLangChange: (v: string) => void;
  onTranslate: () => void;
  translating: boolean;
  canTranslate: boolean;
  /** v2.1：阅读字号档（A-/A+ 控件） */
  fontSize: number;
  onFontIncrease: () => void;
  onFontDecrease: () => void;
  /** v2.1：翻译按钮状态机（文案/重试语义） */
  buttonState: TranslateButtonState;
  /** 部署安全加固：文件选择上传（公网用户入口） */
  onPickFile: (file: File) => void;
  /** 部署安全加固：当前登录用户（null = 未登录） */
  username: string | null;
  onLoginClick: () => void;
  onLogoutClick: () => void;
  /** v2.4：打开「我的文档」历史面板（仅登录态传入） */
  onHistoryClick?: () => void;
}

/** 翻译按钮状态机（v2.1 T7.1） */
export type TranslateButtonState =
  | { kind: 'idle' }
  | { kind: 'translating'; current: number; total: number }
  | { kind: 'done'; errors: number }
  | { kind: 'error' };

export function translateButtonLabel(s: TranslateButtonState): string {
  switch (s.kind) {
    case 'idle':
      return '翻译';
    case 'translating':
      return `翻译中 ${s.current}/${s.total}…`;
    case 'done':
      return s.errors > 0 ? `完成（${s.errors} 段未翻译）· 重译` : '已完成 ✓ 重新翻译';
    case 'error':
      return '翻译出错 · 重试';
  }
}

/** MinerU 解析进度文案 */
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

const LANGS = [
  { code: 'en', label: '英语' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日语' },
  { code: 'fr', label: '法语' },
  { code: 'de', label: '德语' },
];

/** 工具栏：打开文件、语言选择、翻译按钮（plan.md 组件树） */
export default function Toolbar({
  filePath,
  onFilePathChange,
  onParse,
  parsing,
  parseProgress,
  renderMode,
  onRenderModeChange,
  canToggleRenderMode,
  docTitle,
  sourceLang,
  targetLang,
  onSourceLangChange,
  onTargetLangChange,
  onTranslate,
  translating,
  canTranslate,
  fontSize,
  onFontIncrease,
  onFontDecrease,
  buttonState,
  onPickFile,
  username,
  onLoginClick,
  onLogoutClick,
  onHistoryClick,
}: Props) {
  const fileInputRef = useRef<HTMLInputElement>(null);

  return (
    <header className="flex flex-wrap items-center gap-2 border-b border-slate-200 bg-white px-4 py-2">
      <span className="text-lg">🦊</span>
      <h1 className="mr-2 text-sm font-bold text-slate-800">geminus</h1>

      <input
        type="file"
        accept=".docx,.pdf"
        ref={fileInputRef}
        className="hidden"
        data-testid="upload-input"
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) onPickFile(f);
          e.target.value = ''; // 允许重复选择同一文件
        }}
      />
      <button
        onClick={() => fileInputRef.current?.click()}
        disabled={parsing}
        className="rounded border border-slate-300 bg-white px-3 py-1 text-sm text-slate-700 hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-40"
        title="选择 .docx / .pdf 文件上传"
        data-testid="upload-btn"
      >
        📁 选择文件
      </button>

      <input
        type="text"
        value={filePath}
        onChange={(e) => onFilePathChange(e.target.value)}
        placeholder="上传文件，或输入服务器本地 .docx / .pdf 路径（开发）"
        className="min-w-[16rem] flex-1 rounded border border-slate-300 px-2 py-1 text-sm focus:border-blue-400 focus:outline-none"
        data-testid="file-path-input"
      />
      <button
        onClick={onParse}
        disabled={parsing || !filePath.trim()}
        className="rounded bg-slate-800 px-3 py-1 text-sm font-medium text-white hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-40"
        data-testid="open-btn"
      >
        {parsing ? '解析中…' : '打开'}
      </button>

      <div
        className="flex items-center overflow-hidden rounded border border-slate-300 text-xs"
        data-testid="font-size-control"
      >
        <button onClick={onFontDecrease} className="px-2 py-1 hover:bg-slate-100" aria-label="减小字号">
          A-
        </button>
        <span className="px-1.5 py-1 tabular-nums text-slate-500" data-testid="font-size-value">
          {fontSize}
        </span>
        <button onClick={onFontIncrease} className="px-2 py-1 hover:bg-slate-100" aria-label="增大字号">
          A+
        </button>
      </div>

      <div className="mx-2 flex items-center gap-1 text-sm text-slate-600">
        <select
          value={sourceLang}
          onChange={(e) => onSourceLangChange(e.target.value)}
          className="rounded border border-slate-300 px-1 py-1"
          aria-label="源语言"
        >
          {LANGS.map((l) => (
            <option key={l.code} value={l.code}>
              {l.label}
            </option>
          ))}
        </select>
        <span>→</span>
        <select
          value={targetLang}
          onChange={(e) => onTargetLangChange(e.target.value)}
          className="rounded border border-slate-300 px-1 py-1"
          aria-label="目标语言"
        >
          {LANGS.map((l) => (
            <option key={l.code} value={l.code}>
              {l.label}
            </option>
          ))}
        </select>
      </div>

      <button
        onClick={onTranslate}
        disabled={!canTranslate || translating}
        className={
          'rounded px-3 py-1 text-sm font-medium text-white disabled:cursor-not-allowed disabled:opacity-40 ' +
          (buttonState.kind === 'error'
            ? 'bg-red-600 hover:bg-red-500'
            : buttonState.kind === 'done'
              ? 'bg-emerald-600 hover:bg-emerald-500'
              : 'bg-blue-600 hover:bg-blue-500')
        }
        data-testid="translate-btn"
      >
        {translateButtonLabel(buttonState)}
      </button>

      {canToggleRenderMode ? (
        <div className="flex overflow-hidden rounded border border-slate-300 text-xs" data-testid="render-mode-toggle">
          <button
            onClick={() => onRenderModeChange('original')}
            className={
              'px-2 py-1 ' + (renderMode === 'original' ? 'bg-slate-800 text-white' : 'bg-white text-slate-600 hover:bg-slate-100')
            }
            data-testid="mode-original-btn"
          >
            📄 原始渲染
          </button>
          <button
            onClick={() => onRenderModeChange('text')}
            className={
              'px-2 py-1 ' + (renderMode === 'text' ? 'bg-slate-800 text-white' : 'bg-white text-slate-600 hover:bg-slate-100')
            }
            data-testid="mode-text-btn"
          >
            📝 文本对照
          </button>
        </div>
      ) : null}

      {parseProgress ? (
        <span className="text-xs text-blue-600" data-testid="parse-progress">
          {progressLabel(parseProgress)}
        </span>
      ) : null}

      {docTitle ? (
        <span className="ml-auto max-w-[14rem] truncate text-xs text-slate-400" title={docTitle}>
          {docTitle}
        </span>
      ) : null}

      {username ? (
        <div className="flex items-center gap-1" data-testid="auth-area">
          <span
            className="max-w-[10rem] truncate rounded-full bg-emerald-50 px-2.5 py-1 text-xs text-emerald-700"
            title={username}
            data-testid="auth-username"
          >
            👤 {username}
          </span>
          <button
            onClick={onHistoryClick}
            className="rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            data-testid="history-btn"
            title="我的文档"
          >
            🕘 历史
          </button>
          <button
            onClick={onLogoutClick}
            className="rounded px-2 py-1 text-xs text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            data-testid="logout-btn"
          >
            退出
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-1" data-testid="auth-area">
          <span className="hidden text-xs text-slate-400 lg:inline" data-testid="anon-hint">
            未登录 · 限 5 页
          </span>
          <button
            onClick={onLoginClick}
            className="rounded border border-blue-300 bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-700 hover:bg-blue-100"
            data-testid="login-btn"
          >
            登录
          </button>
        </div>
      )}
    </header>
  );
}
