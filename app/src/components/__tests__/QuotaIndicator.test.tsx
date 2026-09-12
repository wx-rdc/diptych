import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import QuotaIndicator from '../QuotaIndicator';
import * as backend from '../../services/backend';

vi.mock('../../services/backend', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/backend')>()),
  fetchQuota: vi.fn(),
}));

afterEach(() => {
  vi.clearAllMocks();
});

describe('QuotaIndicator（v2.5 统一 finite 口径）', () => {
  it('游客：显示剩余 N 页 + title 含用量明细与 24h 恢复', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({
      anonymous: true,
      unlimited: false,
      limit: 5,
      used: 2,
      remaining: 3,
    });
    render(<QuotaIndicator />);
    await waitFor(() => expect(screen.getByTestId('quota-indicator')).toBeInTheDocument());
    expect(screen.getByTestId('quota-indicator').textContent).toContain('剩余 3 页');
    expect(screen.getByTestId('quota-indicator').getAttribute('title')).toContain('已用 2 / 5');
    expect(screen.getByTestId('quota-indicator').getAttribute('title')).toContain('24 小时');
  });

  it('登录用户（1000 页/24h）：同样渲染（v2.5 统一额度）', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({
      anonymous: false,
      unlimited: false,
      limit: 1000,
      used: 120,
      remaining: 880,
    });
    render(<QuotaIndicator />);
    await waitFor(() => expect(screen.getByTestId('quota-indicator')).toBeInTheDocument());
    expect(screen.getByTestId('quota-indicator').textContent).toContain('剩余 880 页');
    expect(screen.getByTestId('quota-indicator').getAttribute('title')).toContain('已用 120 / 1000');
  });

  it('不限量（本地直连）：不渲染', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({
      anonymous: true,
      unlimited: true,
      remaining: null,
    });
    const { container } = render(<QuotaIndicator />);
    await waitFor(() => expect(vi.mocked(backend.fetchQuota)).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });

  it('查询失败：静默隐藏（不打扰游客）', async () => {
    vi.mocked(backend.fetchQuota).mockRejectedValue(new Error('offline'));
    const { container } = render(<QuotaIndicator />);
    await waitFor(() => expect(vi.mocked(backend.fetchQuota)).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });
});
