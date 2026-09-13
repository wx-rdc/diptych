import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, act } from '@testing-library/react';
import Toast from '../Toast';

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
  cleanup();
});

describe('Toast（瞬时错误出口）', () => {
  it('渲染错误文案 + 手动关闭回调', () => {
    const onDismiss = vi.fn();
    render(<Toast message="连接中断" onDismiss={onDismiss} />);
    expect(screen.getByTestId('toast').textContent).toContain('连接中断');
    fireEvent.click(screen.getByTestId('toast-dismiss'));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('6s 后自动消失', () => {
    const onDismiss = vi.fn();
    render(<Toast message="x" onDismiss={onDismiss} />);
    act(() => {
      vi.advanceTimersByTime(6000);
    });
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('message 为 null 不渲染', () => {
    const { container } = render(<Toast message={null} onDismiss={() => {}} />);
    expect(container.firstChild).toBeNull();
  });
});
