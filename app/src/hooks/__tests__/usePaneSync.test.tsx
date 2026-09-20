import { useEffect, useRef } from 'react';
import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { blockIdFromElement, usePaneSync } from '../usePaneSync';

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

// ── 交互（jsdom：rAF 可用） ──

function Host({ onReady }: { onReady: (l: HTMLDivElement, r: HTMLDivElement) => void }) {
  const left = useRef<HTMLDivElement>(null);
  const right = useRef<HTMLDivElement>(null);
  usePaneSync(left, right);
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

describe('usePaneSync 交互', () => {
  afterEach(cleanup);

  it('hover 左栏块 → 双侧同 id 块挂 .g-block-hover；mouseleave 清除', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
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

  it('click 块 → 双侧 scrollIntoView({block:"center"})，各一次', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    const calls: Array<{ el: Element; opts: ScrollIntoViewOptions | undefined }> = [];
    Element.prototype.scrollIntoView = function (this: Element, opts?: ScrollIntoViewOptions) {
      calls.push({ el: this, opts });
    };
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    await act(async () => {
      fireEvent.click(right.querySelector('[data-block-id="b1"]')!);
    });
    expect(calls.length).toBe(2);
    expect(calls.filter((c) => left.contains(c.el)).length).toBe(1);
    expect(calls.filter((c) => right.contains(c.el)).length).toBe(1);
    expect(calls.every((c) => c.opts?.block === 'center')).toBe(true);
  });

  it('单栏滚动不驱动另一栏（自动对齐已移除）', async () => {
    let left!: HTMLDivElement;
    let right!: HTMLDivElement;
    render(<Host onReady={(l, r) => { left = l; right = r; }} />);
    Object.defineProperty(right, 'scrollHeight', { value: 2000, configurable: true });
    Object.defineProperty(right, 'clientHeight', { value: 500, configurable: true });
    await act(async () => {
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!); // 先 hover（高亮联动仍在）
      await new Promise((r2) => setTimeout(r2, 30));
      left.scrollTop = 300;
      fireEvent.scroll(left);
      await new Promise((r2) => setTimeout(r2, 200));
    });
    expect(right.scrollTop).toBe(0); // 右栏纹丝不动
    expect(left.scrollTop).toBe(300); // 左栏自身滚动不被回写
  });

  it('卸载时清除 hover 类', async () => {
    let left!: HTMLDivElement;
    const { unmount } = render(<Host onReady={(l) => { left = l; }} />);
    await act(async () => {
      fireEvent.mouseOver(left.querySelector('[data-block-id="b1"]')!);
      await new Promise((r2) => setTimeout(r2, 30));
    });
    unmount();
    expect(document.querySelectorAll('.g-block-hover').length).toBe(0);
  });
});
