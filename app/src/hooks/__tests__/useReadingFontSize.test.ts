import { afterEach, describe, expect, it } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useReadingFontSize, FONT_STEPS, DEFAULT_FONT_SIZE } from '../useReadingFontSize';

describe('useReadingFontSize 五档字号（T2.2）', () => {
  afterEach(() => {
    localStorage.clear();
    document.documentElement.style.removeProperty('--geminus-user-fs');
  });

  it('默认 18px 并写入 CSS 变量', () => {
    const { result } = renderHook(() => useReadingFontSize());
    expect(result.current.fontSize).toBe(DEFAULT_FONT_SIZE);
    expect(document.documentElement.style.getPropertyValue('--geminus-user-fs')).toBe('18px');
  });

  it('A+ 升档到顶不再升 / A- 降档到底不再降', () => {
    const { result } = renderHook(() => useReadingFontSize());
    act(() => result.current.increase());
    expect(result.current.fontSize).toBe(20);
    [...Array(5)].forEach(() => act(() => result.current.increase()));
    expect(result.current.fontSize).toBe(FONT_STEPS[FONT_STEPS.length - 1]);
    [...Array(9)].forEach(() => act(() => result.current.decrease()));
    expect(result.current.fontSize).toBe(FONT_STEPS[0]);
  });

  it('档位持久化 localStorage 并在下次挂载恢复', () => {
    const first = renderHook(() => useReadingFontSize());
    act(() => first.result.current.increase()); // 20
    first.unmount();
    const second = renderHook(() => useReadingFontSize());
    expect(second.result.current.fontSize).toBe(20);
    expect(localStorage.getItem('geminus-reading-fs')).toBe('20');
  });
});
