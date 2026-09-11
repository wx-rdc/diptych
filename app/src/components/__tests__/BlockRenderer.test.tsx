import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import BlockRenderer from '../BlockRenderer';
import type { Block, Resource } from '../../types';

const resources: Record<string, Resource> = {
  r01_image: { kind: 'image', mime: 'image/png' },
  r02_formula: { kind: 'formula', latex: 'E^{2}' },
};

function renderBlock(block: Block) {
  return render(
    <BlockRenderer docId="d1" block={block} resources={resources} onSelect={() => {}} />,
  );
}

describe('BlockRenderer 段落', () => {
  it('标题按层级渲染语义标签', () => {
    renderBlock({
      type: 'paragraph',
      id: 'b1',
      style: { heading_level: 2, alignment: 'left' },
      content: [{ type: 'text', text: '标题文本' }],
    });
    const h = screen.getByText('标题文本');
    expect(h.closest('h2')).not.toBeNull();
  });

  it('对齐/缩进/列表前缀', () => {
    const { container, getByText } = renderBlock({
      type: 'paragraph',
      id: 'b2',
      style: { alignment: 'center', indent_level: 2, is_list_item: true },
      content: [{ type: 'text', text: '列表项' }],
    });
    expect(getByText('列表项')).toBeInTheDocument();
    expect(getByText('•')).toBeInTheDocument(); // 列表前缀
    const blockEl = container.querySelector('[data-block-id="b2"]') as HTMLElement;
    expect(blockEl.className).toContain('text-center');
    expect(blockEl.style.paddingLeft).toBe('3rem'); // 2 × 1.5rem
  });

  it('粗体/斜体/下划线 run', () => {
    renderBlock({
      type: 'paragraph',
      id: 'b3',
      style: {},
      content: [
        { type: 'text', text: '加粗', bold: true },
        { type: 'text', text: '斜体', italic: true },
        { type: 'text', text: '下划线', underline: true },
        { type: 'text', text: '普通' },
      ],
    });
    // RichText 包层后样式类在外层 span（v2.1 T4）
    expect(screen.getByText('加粗').closest('[class*="font-bold"]')).toBeTruthy();
    expect(screen.getByText('斜体').closest('[class*="italic"]')).toBeTruthy();
    expect(screen.getByText('下划线').closest('[class*="underline"]')).toBeTruthy();
    expect(screen.getByText('普通').closest('[class*="font-bold"]')).toBeNull();
  });

  it('换行渲染 <br>，行内图片指向资源端点', () => {
    const { container } = renderBlock({
      type: 'paragraph',
      id: 'b4',
      style: {},
      content: [
        { type: 'text', text: 'A' },
        { type: 'line_break' },
        { type: 'inline_image', resource_id: 'r01_image' },
      ],
    });
    expect(container.querySelectorAll('br').length).toBe(1);
    const img = container.querySelector('img') as HTMLImageElement;
    expect(img.getAttribute('src')).toBe('/api/resource/d1/r01_image');
  });

  it('空段落渲染占位高度块', () => {
    const { container } = renderBlock({ type: 'empty', id: 'b5' });
    const el = container.querySelector('[data-block-id="b5"]') as HTMLElement;
    expect(el.className).toContain('h-4');
  });

  it('点击回调携带 block id', () => {
    const selected: string[] = [];
    const { container } = render(
      <BlockRenderer
        docId="d1"
        block={{ type: 'paragraph', id: 'b6', style: {}, content: [{ type: 'text', text: 'x' }] }}
        resources={{}}
        onSelect={(id) => selected.push(id)}
      />,
    );
    (container.querySelector('[data-block-id="b6"]') as HTMLElement).click();
    expect(selected).toEqual(['b6']);
  });

  it('v2.2：题注段渲染 0.9em 居中灰字', () => {
    const caption: Block = {
      type: 'paragraph',
      id: 'cap1',
      style: { is_caption: true, alignment: 'center' },
      content: [{ type: 'text', text: 'Figure 1: demo' }],
    };
    const { container } = render(<BlockRenderer docId="d" block={caption} resources={{}} />);
    const el = container.querySelector('[data-block-id="cap1"]') as HTMLElement;
    expect(getComputedStyle(el).fontSize).toBe('0.9em');
    expect(getComputedStyle(el).textAlign).toBe('center');
    expect(getComputedStyle(el).color).toBe('rgb(71, 85, 105)'); // slate-600
  });
});
