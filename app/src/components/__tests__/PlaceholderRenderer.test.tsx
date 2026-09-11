import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import PlaceholderRenderer from '../PlaceholderRenderer';
import type { Resource } from '../../types';

const resources: Record<string, Resource> = {
  r01_image: { kind: 'image', mime: 'image/png' },
  r03_table: { kind: 'table', raw_xml: '<w:tbl/>' },
  r02_formula: { kind: 'formula', latex: 'E^{2}' },
  r04_formula_bad: { kind: 'formula', latex: null },
};

describe('PlaceholderRenderer', () => {
  it('图片：src 指向资源端点，尺寸生效', () => {
    const { container } = render(
      <PlaceholderRenderer
        docId="d1"
        block={{ type: 'image', id: 'b1', resource_id: 'r01_image', width: 100, height: 60, alt_text: '图' }}
        resources={resources}
      />,
    );
    const img = container.querySelector('img') as HTMLImageElement;
    expect(img.getAttribute('src')).toBe('/api/resource/d1/r01_image');
    expect(img.getAttribute('width')).toBe('100');
    expect(img.getAttribute('alt')).toBe('图');
  });

  it('表格：占位框 + 预览文本', () => {
    const { container, getByText } = render(
      <PlaceholderRenderer
        docId="d1"
        block={{ type: 'table', id: 'b2', resource_id: 'r03_table', preview_text: 'Name Age City' }}
        resources={resources}
      />,
    );
    expect(getByText('📊 表格（保留原文）')).toBeInTheDocument();
    expect(getByText('Name Age City')).toBeInTheDocument();
    expect(container.querySelector('[data-block-id="b2"]')).toBeInTheDocument();
  });

  it('公式：KaTeX 渲染 latex', () => {
    const { container } = render(
      <PlaceholderRenderer
        docId="d1"
        block={{ type: 'formula', id: 'b3', resource_id: 'r02_formula' }}
        resources={resources}
      />,
    );
    expect(container.querySelector('.katex')).not.toBeNull();
  });

  it('公式：latex 缺失时降级占位框', () => {
    const { container, getByText } = render(
      <PlaceholderRenderer
        docId="d1"
        block={{ type: 'formula', id: 'b4', resource_id: 'r04_formula_bad' }}
        resources={resources}
      />,
    );
    expect(container.querySelector('.katex')).toBeNull();
    expect(getByText('∑ [公式]')).toBeInTheDocument();
  });

  it('v2.2：复制按钮锚定公式块内右上角（top-1/right-1 定位 + opacity 过渡）', () => {
    const { container } = render(
      <PlaceholderRenderer
        docId="d"
        resources={{ r1: { kind: 'formula', latex: 'x^2' } }}
        block={{ type: 'formula', id: 'b1', resource_id: 'r1' }}
      />,
    );
    const block = container.querySelector('[data-block-id="b1"]') as HTMLElement;
    const btn = block.querySelector('[data-testid="copy-latex-b1"]') as HTMLElement;
    // 定位语义：块内右上角（top-1 right-1，非 v2.1 的 -top-2 溢出块外）
    expect(btn.className).toContain('absolute');
    expect(btn.className).toContain('top-1');
    expect(btn.className).toContain('right-1');
    // opacity 过渡替代 hidden/block 切换（C6 闪烁根因）
    expect(btn.className).toContain('opacity-0');
    expect(btn.className).toContain('group-hover:opacity-100');
    expect(btn.className).not.toContain('hidden');
    // 按钮位于块内
    expect(block.contains(btn)).toBe(true);
  });
});
