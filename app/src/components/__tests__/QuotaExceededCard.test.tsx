import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import QuotaExceededCard from '../QuotaExceededCard';

describe('QuotaExceededCard（T6）', () => {
  const quota = { limit: 5, used: 5, required: 1 };

  it('展示限额文案 + [立即登录] / [仅查看已译部分]', () => {
    render(<QuotaExceededCard quota={quota} onLogin={() => {}} onDismiss={() => {}} />);
    expect(screen.getByTestId('quota-exceeded-card')).toBeTruthy();
    expect(screen.getByText(/游客最多可翻译 5 页/)).toBeTruthy();
    expect(screen.getByText(/已翻译的部分仍可继续阅读/)).toBeTruthy();
    expect(screen.getByTestId('quota-login-btn')).toBeTruthy();
    expect(screen.getByTestId('quota-dismiss-btn')).toBeTruthy();
  });

  it('点击登录 / 忽略分别触发回调', () => {
    const onLogin = vi.fn();
    const onDismiss = vi.fn();
    render(<QuotaExceededCard quota={quota} onLogin={onLogin} onDismiss={onDismiss} />);
    fireEvent.click(screen.getByTestId('quota-login-btn'));
    expect(onLogin).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-dismiss-btn'));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
