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

describe('QuotaIndicator（T6.1）', () => {
  it('游客：显示剩余 N 页 + title 含用量明细', async () => {
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
  });

  it('登录 / 不限量：不渲染', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue({
      anonymous: false,
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
