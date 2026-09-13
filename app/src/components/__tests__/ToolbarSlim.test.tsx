import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import ToolbarSlim, { progressLabel } from '../toolbar/ToolbarSlim';
import type { ParseProgress } from '../../types';

vi.mock('../../services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/backend')>()),
  fetchQuota: vi.fn().mockResolvedValue({ anonymous: true, unlimited: true, remaining: null }),
}));

afterEach(cleanup);

const base = {
  docTitle: null as string | null,
  fileType: null as 'docx' | 'pdf' | null,
  onPickFile: vi.fn(),
  parseLabel: null as string | null,
  translating: false,
  paused: false,
  done: false,
  stats: { done: 0, cached: 0, errors: 0 },
  progressCurrent: 0,
  progressTotal: 0,
  onStopTranslate: vi.fn(),
  onContinueTranslate: vi.fn(),
  onRetryFailed: vi.fn(),
  onRetranslate: vi.fn(),
  renderMode: 'original' as const,
  onRenderModeChange: vi.fn(),
  canToggleRenderMode: false,
  fontSize: 18,
  onFontIncrease: vi.fn(),
  onFontDecrease: vi.fn(),
  sourceLang: 'en',
  targetLang: 'zh',
  onSourceLangChange: vi.fn(),
  onTargetLangChange: vi.fn(),
  devPath: '',
  onDevPathChange: vi.fn(),
  onParseLocal: vi.fn(),
  username: null as string | null,
  onLoginClick: vi.fn(),
  onLogoutClick: vi.fn(),
};

describe('ToolbarSlim（48px 单行七区）', () => {
  it('默认渲染：Logo/wordmark、＋打开主按钮、⚙、登录；无哈希 ID/游客模式碎片/字号/语言对常驻', () => {
    render(<ToolbarSlim {...base} />);
    expect(screen.getByTestId('upload-btn')).toBeInTheDocument();
    expect(screen.getByTestId('upload-input')).toBeInTheDocument();
    expect(screen.getByTestId('settings-btn')).toBeInTheDocument();
    expect(screen.getByTestId('login-btn')).toBeInTheDocument();
    // 收纳验证：常驻区不再直接出现语言对/字号控件
    expect(screen.queryByTestId('font-size-control')).toBeNull();
    expect(screen.queryByLabelText('源语言')).toBeNull();
    expect(screen.queryByTestId('anon-hint')).toBeNull();
  });

  it('文档打开：文档名 + 类型徽章；分段控件按需出现', () => {
    render(<ToolbarSlim {...base} docTitle="01_ViT_Original_ViT.pdf" fileType="pdf" canToggleRenderMode />);
    expect(screen.getByText('01_ViT_Original_ViT.pdf')).toBeInTheDocument();
    expect(screen.getByTestId('file-type-badge').textContent).toBe('PDF');
    expect(screen.getByTestId('render-mode-toggle')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('mode-text-btn'));
    expect(base.onRenderModeChange).toHaveBeenCalledWith('text');
  });

  it('无 pdf_meta：分段控件不出现', () => {
    render(<ToolbarSlim {...base} canToggleRenderMode={false} />);
    expect(screen.queryByTestId('render-mode-toggle')).toBeNull();
  });

  it('翻译中：顶栏下缘 2px 进度条按比例；未翻译时隐藏', () => {
    const { rerender } = render(<ToolbarSlim {...base} translating progressCurrent={23} progressTotal={46} />);
    const bar = screen.getByTestId('top-progress-bar');
    expect(bar.style.width).toBe('50%');
    rerender(<ToolbarSlim {...base} translating={false} progressCurrent={46} progressTotal={46} />);
    expect(screen.queryByTestId('top-progress-bar')).toBeNull();
  });

  it('登录态：用户名 + 历史 + 退出 + 配额 Chip 挂载点', () => {
    render(<ToolbarSlim {...base} username="alice" onHistoryClick={vi.fn()} />);
    expect(screen.getByTestId('auth-username').textContent).toContain('alice');
    expect(screen.getByTestId('history-btn')).toBeInTheDocument();
    expect(screen.getByTestId('logout-btn')).toBeInTheDocument();
  });

  it('progressLabel：各解析阶段文案', () => {
    const p = (stage: ParseProgress['stage']) => ({ stage }) as ParseProgress;
    expect(progressLabel(p('merging'))).toContain('合并');
    expect(progressLabel(p('fallback'))).toContain('回退');
    expect(progressLabel(p('cached'))).toContain('缓存');
  });
});
