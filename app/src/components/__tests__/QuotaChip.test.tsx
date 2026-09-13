import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import QuotaChip from '../toolbar/QuotaChip';
import * as backend from '../../services/backend';

vi.mock('../../services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/backend')>()),
  fetchQuota: vi.fn(),
}));

const onLoginClick = vi.fn();
const onViewUsage = vi.fn();

afterEach(() => {
  vi.clearAllMocks();
  cleanup();
});

describe('QuotaChip（五碎片收敛为一个 Chip + Popover）', () => {
  it('游客：Chip 显示 ◔ N页；Popover 有剩余/上限、24h 清理提示、登录 CTA、翻译记录入口', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({ anonymous: true, unlimited: false, limit: 5, used: 2, remaining: 3 });
    render(<QuotaChip onLoginClick={onLoginClick} onViewUsage={onViewUsage} />);
    await waitFor(() => expect(screen.getByTestId('quota-indicator')).toBeInTheDocument());
    expect(screen.getByTestId('quota-indicator').textContent).toContain('3');
    fireEvent.click(screen.getByTestId('quota-indicator'));
    const pop = screen.getByTestId('quota-popover');
    expect(pop.textContent).toContain('游客额度');
    expect(pop.textContent).toContain('剩余 3 页');
    expect(pop.textContent).toContain('5 页');
    expect(pop.textContent).toContain('24h');
    fireEvent.click(screen.getByTestId('quota-popover-login-btn'));
    expect(onLoginClick).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-indicator'));
    fireEvent.click(screen.getByTestId('quota-popover-usage-btn'));
    expect(onViewUsage).toHaveBeenCalledOnce();
  });

  it('登录用户（1000 页/24h）：显示剩余；无登录 CTA、无清理提示、无记录入口', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({ anonymous: false, unlimited: false, limit: 1000, used: 120, remaining: 880 });
    render(<QuotaChip onLoginClick={onLoginClick} />);
    await waitFor(() => expect(screen.getByTestId('quota-indicator')).toBeInTheDocument());
    expect(screen.getByTestId('quota-indicator').textContent).toContain('880');
    fireEvent.click(screen.getByTestId('quota-indicator'));
    const pop = screen.getByTestId('quota-popover');
    expect(pop.textContent).toContain('880');
    expect(pop.textContent).toContain('1000');
    expect(screen.queryByTestId('quota-popover-login-btn')).toBeNull();
    expect(screen.queryByTestId('quota-popover-usage-btn')).toBeNull();
    expect(pop.textContent).not.toContain('自动清理');
  });

  it('不限量（本地直连）：不渲染', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({ anonymous: true, unlimited: true, remaining: null });
    const { container } = render(<QuotaChip onLoginClick={onLoginClick} />);
    await waitFor(() => expect(backend.fetchQuota).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });

  it('查询失败：静默隐藏', async () => {
    vi.mocked(backend.fetchQuota).mockRejectedValue(new Error('offline'));
    const { container } = render(<QuotaChip onLoginClick={onLoginClick} />);
    await waitFor(() => expect(backend.fetchQuota).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });
});
