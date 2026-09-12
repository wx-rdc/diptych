import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import QuotaExceededCard from '../QuotaExceededCard';

describe('QuotaExceededCard（v2.4.1 variant 拆分）', () => {
  const quota = { limit: 5, used: 5, required: 1 };
  const precheckQuota = { limit: 5, used: 3, required: 11 };

  it('limit：展示已译可读承诺 + [查看已译部分] / [知道了]', () => {
    render(
      <QuotaExceededCard quota={quota} variant="limit" onLogin={() => {}} onViewTranslated={() => {}} onDismiss={() => {}} />,
    );
    expect(screen.getByTestId('quota-exceeded-card')).toBeTruthy();
    expect(screen.getByText(/游客最多可翻译 5 页/)).toBeTruthy();
    expect(screen.getByText(/已翻译的部分仍可继续阅读/)).toBeTruthy();
    expect(screen.getByTestId('quota-view-translated-btn')).toBeTruthy();
    expect(screen.getByTestId('quota-dismiss-btn')).toBeTruthy();
    expect(screen.queryByTestId('quota-pick-another-btn')).toBeNull();
  });

  it('limit 无 onViewTranslated（无已译内容兜底）：不渲染该按钮', () => {
    render(<QuotaExceededCard quota={quota} variant="limit" onLogin={() => {}} onDismiss={() => {}} />);
    expect(screen.queryByTestId('quota-view-translated-btn')).toBeNull();
  });

  it('precheck：明确「该文件未开始翻译」+ 剩余/需要页数 + [换个文件试试]，不承诺已译可读', () => {
    render(
      <QuotaExceededCard
        quota={precheckQuota}
        variant="precheck"
        onLogin={() => {}}
        onPickAnother={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByText(/该文件未开始翻译/)).toBeTruthy();
    expect(screen.getByText(/剩余 2 页/)).toBeTruthy();
    expect(screen.getByText(/需要 11 页/)).toBeTruthy();
    expect(screen.getByTestId('quota-pick-another-btn')).toBeTruthy();
    expect(screen.queryByTestId('quota-view-translated-btn')).toBeNull();
    expect(screen.queryByText(/已翻译的部分仍可继续阅读/)).toBeNull();
  });

  it('回调分别触发', () => {
    const onLogin = vi.fn();
    const onDismiss = vi.fn();
    const onViewTranslated = vi.fn();
    render(
      <QuotaExceededCard quota={quota} variant="limit" onLogin={onLogin} onViewTranslated={onViewTranslated} onDismiss={onDismiss} />,
    );
    fireEvent.click(screen.getByTestId('quota-login-btn'));
    expect(onLogin).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-view-translated-btn'));
    expect(onViewTranslated).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('quota-dismiss-btn'));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
