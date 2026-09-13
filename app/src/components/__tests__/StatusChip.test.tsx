import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import StatusChip from '../toolbar/StatusChip';

const base = {
  hasDoc: true,
  parseLabel: null as string | null,
  translating: false,
  paused: false,
  done: false,
  stats: { done: 0, cached: 0, errors: 0 },
  current: 0,
  total: 0,
  onStop: vi.fn(),
  onContinue: vi.fn(),
  onRetryFailed: vi.fn(),
  onRetranslate: vi.fn(),
};

afterEach(cleanup);

function chip(p: Partial<typeof base> = {}) {
  const props = { ...base, ...p };
  render(<StatusChip {...props} />);
  return props;
}

describe('StatusChip（状态单一出口）', () => {
  it('空闲且无文档 → 不渲染', () => {
    const { container } = render(<StatusChip {...base} hasDoc={false} />);
    expect(container.firstChild).toBeNull();
  });

  it('翻译中：⟳ current/total；Popover 有 [⏹ 停止]', () => {
    chip({ translating: true, current: 32, total: 46 });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('translating');
    expect(el.textContent).toContain('32/46');
    fireEvent.click(el);
    expect(screen.getByTestId('status-popover')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('status-stop-btn'));
    expect(base.onStop).toHaveBeenCalledOnce();
  });

  it('解析中：⟳ 解析中 + title 带完整解析文案；Popover 显示解析文案', () => {
    chip({ hasDoc: false, parseLabel: '云端解析 1/2 文件…' });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('parsing');
    expect(el.textContent).toContain('解析中');
    expect(el.getAttribute('title')).toContain('云端解析 1/2');
    fireEvent.click(el);
    expect(screen.getByTestId('status-popover').textContent).toContain('云端解析 1/2');
  });

  it('完成无失败：✓；Popover 有 [↻ 重新翻译全文] 无重试失败', () => {
    chip({ done: true, stats: { done: 46, cached: 0, errors: 0 } });
    expect(screen.getByTestId('status-chip').dataset.state).toBe('done');
    fireEvent.click(screen.getByTestId('status-chip'));
    expect(screen.getByTestId('status-popover').textContent).toContain('完成 46');
    expect(screen.getByTestId('status-popover').textContent).not.toContain('缓存'); // 缓存=0 隐藏
    expect(screen.getByTestId('status-retranslate-btn')).toBeInTheDocument();
    expect(screen.queryByTestId('status-retry-failed-btn')).toBeNull();
  });

  it('完成有失败：⚠ N 段；Popover 显示失败计数 + [重试失败段]', () => {
    chip({ done: true, stats: { done: 45, cached: 1, errors: 1 } });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('warning');
    expect(el.textContent).toContain('1');
    fireEvent.click(el);
    const text = screen.getByTestId('status-popover').textContent ?? '';
    expect(text).toContain('完成 45');
    expect(text).toContain('缓存 1'); // >0 才显示
    expect(text).toContain('失败 1');
    fireEvent.click(screen.getByTestId('status-retry-failed-btn'));
    expect(base.onRetryFailed).toHaveBeenCalledOnce();
  });

  it('暂停态：⏸ 已暂停 + [▶ 继续翻译]', () => {
    chip({ paused: true });
    expect(screen.getByTestId('status-chip').dataset.state).toBe('paused');
    fireEvent.click(screen.getByTestId('status-chip'));
    fireEvent.click(screen.getByTestId('status-continue-btn'));
    expect(base.onContinue).toHaveBeenCalledOnce();
  });

  it('动作点击后面板收起', () => {
    chip({ done: true });
    fireEvent.click(screen.getByTestId('status-chip'));
    fireEvent.click(screen.getByTestId('status-retranslate-btn'));
    expect(screen.queryByTestId('status-popover')).toBeNull();
    expect(base.onRetranslate).toHaveBeenCalledOnce();
  });

  it('打开新文档解析中 → 优先于上一份文档的完成态（否则用户看不到任何反馈）', () => {
    chip({ done: true, stats: { done: 46, cached: 0, errors: 0 }, parseLabel: '上传片段 1/2…' });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('parsing');
    expect(el.textContent).toContain('解析中');
  });

  it('翻译刚启动（进度总数未到）显示「翻译中」而非 0/0', () => {
    chip({ translating: true, current: 0, total: 0 });
    const el = screen.getByTestId('status-chip');
    expect(el.dataset.state).toBe('translating');
    expect(el.textContent).toContain('翻译中');
    expect(el.textContent).not.toContain('0/0');
  });
});
