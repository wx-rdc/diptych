import { useEffect, useRef } from 'react';
import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  blockIdFromElement,
  centerScrollTop,
  lerpTowards,
  proportionalScrollTop,
  useHoverDrivenSync,
} from '../useHoverDrivenSync';

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('blockIdFromElement', () => {
  it('从事件目标冒泡提取 data-block-id；块外返回 null', () => {
    const pane = document.createElement('div');
    const block = document.createElement('div');
    block.dataset.blockId = 'b1';
    const child = document.createElement('span');
    block.appendChild(child);
    pane.appendChild(block);
    pane.appendChild(document.createElement('p'));
    document.body.appendChild(pane);
    expect(blockIdFromElement(child)).toBe('b1');
    expect(blockIdFromElement(pane.querySelector('p'))).toBeNull();
    expect(blockIdFromElement(null)).toBeNull();
  });
});

describe('centerScrollTop', () => {
  it('目标 = scrollTop + 块相对视口偏移 + 半高 − 半视口（rect 差值法）', () => {
    const pane = document.createElement('div');
    const el = document.createElement('div');
    pane.appendChild(el);
    pane.scrollTop = 300;
    vi.spyOn(pane, 'getBoundingClientRect').mockReturnValue({ top: 0 } as DOMRect);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({ top: 120 } as DOMRect);
    Object.defineProperty(el, 'offsetHeight', { value: 80, configurable: true });
    Object.defineProperty(pane, 'clientHeight', { value: 600, configurable: true });
    // 300 + 120 + 40 − 300 = 160
    expect(centerScrollTop(pane, el)).toBe(160);
  });

  it('结果不为负（顶部之上夹到 0）', () => {
    const pane = document.createElement('div');
    const el = document.createElement('div');
    pane.appendChild(el);
    pane.scrollTop = 0;
    vi.spyOn(pane, 'getBoundingClientRect').mockReturnValue({ top: 0 } as DOMRect);
    vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({ top: 10 } as DOMRect);
    Object.defineProperty(el, 'offsetHeight', { value: 20, configurable: true });
    Object.defineProperty(pane, 'clientHeight', { value: 600, configurable: true });
    expect(centerScrollTop(pane, el)).toBe(0);
  });
});

describe('proportionalScrollTop', () => {
  it('按可滚动比例映射；master 不可滚 → 0', () => {
    const m = document.createElement('div');
    const s = document.createElement('div');
    Object.defineProperty(m, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(m, 'clientHeight', { value: 500, configurable: true });
    Object.defineProperty(s, 'scrollHeight', { value: 1000, configurable: true });
    Object.defineProperty(s, 'clientHeight', { value: 400, configurable: true });
    m.scrollTop = 750; // 可滚 1500 的一半
    expect(proportionalScrollTop(m, s)).toBe(300); // 600 的一半
    Object.defineProperty(m, 'scrollHeight', { value: 500, configurable: true });
    expect(proportionalScrollTop(m, s)).toBe(0);
  });
});

describe('lerpTowards', () => {
  it('按系数逼近并在 <1px 时 snap 到目标', () => {
    expect(lerpTowards(0, 100, 0.25)).toBe(25);
    expect(lerpTowards(99.6, 100)).toBe(100);
  });
});

// ── 交互（jsdom：rAF 可用；布局用 rect/尺寸 mock） ──

function Host({ onReady }: { onReady: (l: HTMLDivElement, r: HTMLDivElement) => void }) {
  const left = useRef<HTMLDivElement>(null);
  const right = useRef<HTMLDivElement>(null);
  useHoverDrivenSync(left, right);
  useEffect(() => {
    if (left.current && right.current) onReady(left.current, right.current);
  });
  return (
    <div>
      <div ref={left} data-testid="left">
        <div data-block-id="b1" style={{ height: 50 }}>
          A
        </div>
      </div>
      <div ref={right} data-testid="right">
        <div data-block-id="b1" style={{ height: 50 }}>
          A&rsquo;
        </div>
      </div>
    </div>
  );
}

describe('useHoverDrivenSync 交互', () => {
  afterEach(cleanup);

  it('hover 左栏块 → 双侧同 id 块挂 .g-block-hover；mouseleave 清除', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.mouseEnter(left);
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!);
      await new Promise((r2) => setTimeout(r2, 30)); // rAF 合帧
    });
    expect(left.querySelector('[data-block-id="b1"]')!.classList.contains('g-block-hover')).toBe(true);
    expect(right.querySelector('[data-block-id="b1"]')!.classList.contains('g-block-hover')).toBe(true);
    await act(async () => {
      fireEvent.mouseLeave(left);
    });
    expect(right.querySelector('.g-block-hover')).toBeNull();
    expect(left.querySelector('.g-block-hover')).toBeNull();
  });

  it('click 块 → 双侧 scrollIntoView(center)', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    const calls: Element[] = [];
    Element.prototype.scrollIntoView = function (this: Element) {
      calls.push(this);
    };
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.click(right.querySelector('[data-block-id="b1"]')!);
    });
    expect(calls.length).toBe(2);
    expect(left.contains(calls[0]) || right.contains(calls[0])).toBe(true);
    expect(left.contains(calls[1]) || right.contains(calls[1])).toBe(true);
    // 一侧一个（分别来自左右 pane）
    const inLeft = calls.filter((el) => left.contains(el)).length;
    const inRight = calls.filter((el) => right.contains(el)).length;
    expect(inLeft).toBe(1);
    expect(inRight).toBe(1);
  });

  it('master 滚动（hover 块存在）→ slave 以居中为目标 lerp 前进', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    // 布局 mock：右栏可滚，b1 视口位置随滚动联动（模拟真实布局，目标恒定）
    Object.defineProperty(right, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(right, 'clientHeight', { value: 500, configurable: true });
    const rightBlock = right.querySelector('[data-block-id="b1"]') as HTMLElement;
    vi.spyOn(right, 'getBoundingClientRect').mockReturnValue({ top: 0 } as DOMRect);
    vi.spyOn(rightBlock, 'getBoundingClientRect').mockImplementation(
      () => ({ top: 900 - right.scrollTop } as DOMRect),
    );
    Object.defineProperty(rightBlock, 'offsetHeight', { value: 100, configurable: true });

    await act(async () => {
      fireEvent.mouseEnter(left); // master = left
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!);
      await new Promise((r2) => setTimeout(r2, 30));
    });
    const before = right.scrollTop; // 0
    await act(async () => {
      fireEvent.scroll(left); // master 滚动事件
      await new Promise((r2) => setTimeout(r2, 120)); // 若干帧 lerp
    });
    // 目标 = 0 + 900 + 50 − 250 = 700；lerp 单帧 175 → 前进但未到
    expect(right.scrollTop).toBeGreaterThan(before);
    expect(right.scrollTop).toBeLessThan(700);
    // 左栏（master）不受反向驱动
    expect(left.scrollTop).toBe(0);
  });

  it('无 hover 块时 master 滚动 → slave 比例同步', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    Object.defineProperty(left, 'scrollHeight', { value: 1000, configurable: true });
    Object.defineProperty(left, 'clientHeight', { value: 500, configurable: true });
    Object.defineProperty(right, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(right, 'clientHeight', { value: 500, configurable: true });
    await act(async () => {
      fireEvent.mouseEnter(left); // master，但 mouseover 未命中块
      left.scrollTop = 250; // 可滚 500 的一半
      fireEvent.scroll(left);
      await new Promise((r2) => setTimeout(r2, 600)); // lerp 收敛（~26 帧）后 snap
    });
    expect(right.scrollTop).toBe(750); // 1500 的一半（snap 收敛）
  });

  it('slave 侧滚动事件不反向驱动（滚动锁）', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.mouseEnter(left); // master = left
      fireEvent.mouseEnter(right); // 后进入不覆盖既有 master（仍是 left）
      right.scrollTop = 123;
      fireEvent.scroll(right); // 非 master 滚动 → 忽略
      await new Promise((r2) => setTimeout(r2, 50));
    });
    expect(left.scrollTop).toBe(0);
  });
});
