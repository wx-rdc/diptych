import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import QuotaExceededCard from '../QuotaExceededCard';

describe('QuotaExceededCard（v2.5 统一部分处理文案）', () => {
  const guestQuota = { limit: 5, used: 5, required: 1, translated_pages: 5, reset_at: 1770000000 };
  const exhaustedQuota = { limit: 5, used: 5, required: 1 };
  const userQuota = { limit: 1000, used: 1000, translated_pages: 3, reset_at: 1770000000, user: true };

  it('游客熔断：已译页数可读 + 24h 恢复 + 登录引导（每天 1000 页）', () => {
    render(
      <QuotaExceededCard
        quota={guestQuota}
        onLogin={() => {}}
        onViewTranslated={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByTestId('quota-exceeded-card')).toBeTruthy();
    expect(screen.getByText(/游客每 24 小时最多翻译 5 页/)).toBeTruthy();
    expect(screen.getByText(/已翻译的前 5 页仍可继续阅读/)).toBeTruthy();
    expect(screen.getByText(/逐页恢复/)).toBeTruthy();
    expect(screen.getByText(/登录后每天可翻译 1000 页/)).toBeTruthy();
    expect(screen.getByTestId('quota-view-translated-btn')).toBeTruthy();
    expect(screen.getByTestId('quota-login-btn')).toBeTruthy();
    expect(screen.getByTestId('quota-dismiss-btn')).toBeTruthy();
  });

  it('额度耗尽（无已译页）：不渲染「查看已译部分」', () => {
    render(<QuotaExceededCard quota={exhaustedQuota} onLogin={() => {}} onDismiss={() => {}} />);
    expect(screen.queryByTestId('quota-view-translated-btn')).toBeNull();
    expect(screen.getByText(/游客每 24 小时最多翻译 5 页/)).toBeTruthy();
  });

  it('登录用户（1000 页用尽）：不渲染登录引导，仅提示恢复', () => {
    render(<QuotaExceededCard quota={userQuota} onLogin={() => {}} onDismiss={() => {}} />);
    expect(screen.queryByTestId('quota-login-btn')).toBeNull();
    expect(screen.getByText(/每 24 小时最多翻译 1000 页/)).toBeTruthy();
    expect(screen.getByText(/逐页恢复/)).toBeTruthy();
  });

  it('回调分别触发', () => {
    const onLogin = vi.fn();
    const onDismiss = vi.fn();
    const onViewTranslated = vi.fn();
    render(
      <QuotaExceededCard
        quota={guestQuota}
        onLogin={onLogin}
        onViewTranslated={onViewTranslated}
        onDismiss={onDismiss}
      />,
    );
    fireEvent.click(screen.getByTestId('quota-login-btn'));
    expect(onLogin).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-view-translated-btn'));
    expect(onViewTranslated).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-dismiss-btn'));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
