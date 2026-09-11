import { describe, expect, it, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/react';
import AlgorithmCard from '../AlgorithmCard';
import type { Block, Resource } from '../../types';

const resources: Record<string, Resource> = {
  r_eq: { kind: 'formula', latex: 'V \\leftarrow x' },
};

const algoBlock: Extract<Block, { type: 'algorithm' }> = {
  type: 'algorithm',
  id: 'b_0001',
  header: 'Algorithm 1 Test',
  lines: [
    { kind: 'label', segments: [{ type: 'text', text: 'Output: The loss' }] },
    {
      kind: 'stmt',
      segments: [
        { type: 'text', text: 'Select ' },
        { type: 'inline_formula', resource_id: 'r_eq' },
      ],
    },
    { kind: 'comment', segments: [{ type: 'text', text: '▷ Initialize loss' }] },
  ],
};

describe('AlgorithmCard（v2.2 T4）', () => {
  it('渲染 header 与全部行，Label 加粗 / Comment 斜体', () => {
    const { container } = render(
      <AlgorithmCard docId="d" block={algoBlock} resources={resources} />,
    );
    const card = container.querySelector('[data-testid="algorithm-card-b_0001"]')!;
    expect(card.textContent).toContain('Algorithm 1 Test');
    expect(card.textContent).toContain('Output: The loss');
    const lines = container.querySelectorAll('[data-testid^="algo-line-b_0001-"]');
    expect(lines.length).toBe(3);
    expect(lines[0].className).toContain('font-bold');
    expect(lines[2].className).toContain('italic');
    // 行内公式经 InlineContentRenderer → katex 容器（jsdom 中 KaTeX 渲染 html span）
    expect(card.querySelector('.katex')).toBeTruthy();
  });

  it('点击触发 onSelect；active 态样式切换', () => {
    const onSelect = vi.fn();
    const { container, rerender } = render(
      <AlgorithmCard docId="d" block={algoBlock} resources={resources} onSelect={onSelect} />,
    );
    fireEvent.click(container.querySelector('[data-block-id="b_0001"]')!);
    expect(onSelect).toHaveBeenCalledWith('b_0001');
    rerender(
      <AlgorithmCard
        docId="d"
        block={algoBlock}
        resources={resources}
        onSelect={onSelect}
        active
      />,
    );
    expect(
      container.querySelector('[data-active="true"][data-block-id="b_0001"]'),
    ).toBeTruthy();
  });

  it('空 header 不渲染标题行', () => {
    const { container } = render(
      <AlgorithmCard
        docId="d"
        block={{ ...algoBlock, header: '' }}
        resources={resources}
      />,
    );
    expect(container.querySelector('p')).toBeNull();
  });
});
