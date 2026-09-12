import { useRef } from 'react';
import type { ParseProgress, RenderMode } from '../types';
import QuotaIndicator from './QuotaIndicator';

interface Props {
  filePath: string;
  onFilePathChange: (v: string) => void;
  /** v2.4 T3：本地路径开发分支（Enter 触发零点击流水线；上传入口已自动化） */
  onParseLocal: () => void;
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
  /** v2.1：阅读字号档（A-/A+ 控件） */
  fontSize: number;
  onFontIncrease: () => void;
  onFontDecrease: () => void;
  /** 部署安全加固：文件选择上传（公网用户入口；v2.4 零点击自动流水线起点） */
  onPickFile: (file: File) => void;
  /** 部署安全加固：当前登录用户（null = 未登录） */
  username: string | null;
  onLoginClick: () => void;
  onLogoutClick: () => void;
  /** v2.4：打开「我的文档」历史面板（仅登录态传入） */
  onHistoryClick?: () => void;
  /** v2.4.1：游客点额度指示器 → 打开翻译记录面板 */
  onViewUsage?: () => void;
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

/**
 * 工具栏（v2.4 T3 零点击后）：文件选择即自动「解析 → 翻译」，
 * 「打开 / 开始翻译」按钮已移除；本地路径输入保留为开发分支（Enter 触发）。
 */
export default function Toolbar({
  filePath,
  onFilePathChange,
  onParseLocal,
  parseProgress,
  renderMode,
  onRenderModeChange,
  canToggleRenderMode,
  docTitle,
  sourceLang,
  targetLang,
  onSourceLangChange,
  onTargetLangChange,
  fontSize,
  onFontIncrease,
  onFontDecrease,
  onPickFile,
  username,
  onLoginClick,
  onLogoutClick,
  onHistoryClick,
  onViewUsage,
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
        className="rounded bg-slate-800 px-3 py-1 text-sm font-medium text-white hover:bg-slate-700"
        title="选择 .docx / .pdf 文件，自动解析并翻译"
        data-testid="upload-btn"
      >
        📁 选择文件
      </button>

      <input
        type="text"
        value={filePath}
        onChange={(e) => onFilePathChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && filePath.trim()) onParseLocal();
        }}
        placeholder="上传文件，或输入服务器本地 .docx / .pdf 路径后回车（开发）"
        className="hidden min-w-[16rem] flex-1 rounded border border-slate-300 px-2 py-1 text-sm focus:border-blue-400 focus:outline-none md:block"
        data-testid="file-path-input"
      />
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
            游客模式 · 额度内免登录
          </span>
          <QuotaIndicator onViewUsage={onViewUsage} />
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
