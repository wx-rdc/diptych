import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render } from '@testing-library/react';
import RichText from '../RichText';

afterEach(cleanup);

describe('RichText 行内公式渲染（T4）', () => {
  it('公式渲染为 .katex 节点且不残留 $ 定界符', () => {
    const { container } = render(<RichText text="误差 $10^{-3}$ 以内" />);
    expect(container.querySelector('.katex')).toBeTruthy();
    expect(container.textContent).not.toContain('$');
  });

  it('\\times 等 LaTeX 指令不残留原文', () => {
    const { container } = render(<RichText text="宽 $w \\times h$" />);
    expect(container.textContent).not.toContain('\\times');
    expect(container.querySelector('.katex')).toBeTruthy();
  });

  it('纯文本原样', () => {
    const { container } = render(<RichText text="hello" />);
    expect(container.textContent).toBe('hello');
    expect(container.querySelector('.katex')).toBeNull();
  });

  it('KaTeX 失败不抛异常（throwOnError:false 占位降级）', () => {
    const { container } = render(<RichText text="怪 $\\frac{$" />);
    expect(container.textContent).toBeTruthy();
  });
});

describe('RichText 内联 HTML 白名单（v2.3 B1）', () => {
  it('白名单内联 HTML 渲染为真实元素（<sup> 上标）', () => {
    const { container } = render(<RichText text={'HONGYU ZHU<sup>1</sup>, GUI LIU<sup>2</sup>'} />);
    const sups = container.querySelectorAll('sup');
    expect(sups.length).toBe(2);
    expect(sups[0].textContent).toBe('1');
    expect(container.textContent).not.toContain('<sup>');
  });

  it('白名单外标签被剥离（script/img/onclick）', () => {
    const { container } = render(
      <RichText text={'a<script>alert(1)</script><img src=x onerror=1>b'} />,
    );
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).not.toContain('alert(1)');
  });

  it('游离的 < 不影响纯文本渲染', () => {
    const { container } = render(<RichText text="a < b 与 c > d" />);
    expect(container.textContent).toBe('a < b 与 c > d');
  });

  it('行内公式与内联 HTML 混排互不干扰', () => {
    const { container } = render(<RichText text={'loss $J(\\phi)$<sup>1</sup> ok'} />);
    expect(container.querySelector('.katex')).toBeTruthy();
    expect(container.querySelector('sup')).toBeTruthy();
  });
});
