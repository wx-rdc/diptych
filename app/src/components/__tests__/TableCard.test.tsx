import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import TableCard from '../TableCard';
import type { Resource } from '../../types';

vi.mock('../../services/backend', () => ({
  resourceUrl: (docId: string, rid: string) => `/api/resource/${docId}/${rid}`,
}));

const tableHtml = '<table><tr><td>Method</td><td>Acc</td></tr><tr><td>proto</td><td>99.5</td></tr></table>';
const block = { type: 'table', id: 'b_0004', resource_id: 'tbl_0004', preview_text: 'Method Acc' } as const;

afterEach(cleanup);

describe('TableCard 富渲染 + 降级链（T3+T5）', () => {
  it('HTML 渲染为 <table> 且不暴露裸标签', () => {
    const resources: Record<string, Resource> = { tbl_0004: { kind: 'table', raw_xml: tableHtml } };
    render(<TableCard docId="d" block={block} resources={resources} />);
    expect(screen.getByTestId('table-body-b_0004').querySelector('table')).toBeTruthy();
    expect(document.body.textContent).not.toContain('<td');
  });

  it('行列数显示 2×2', () => {
    const resources: Record<string, Resource> = { tbl_0004: { kind: 'table', raw_xml: tableHtml } };
    render(<TableCard docId="d" block={block} resources={resources} />);
    expect(screen.getByTestId('table-shape-b_0004').textContent).toBe('2 行 × 2 列');
  });

  it('点击展开 → Lightbox 双 tab（有截图时）+ tab 切换显示截图', () => {
    const resources: Record<string, Resource> = {
      tbl_0004: { kind: 'table', raw_xml: tableHtml },
      tbl_0004_shot: { kind: 'image', mime: 'image/jpeg' },
    };
    render(<TableCard docId="d" block={block} resources={resources} />);
    fireEvent.click(screen.getByTestId('table-expand-b_0004'));
    expect(screen.getByTestId('lightbox')).toBeTruthy();
    expect(screen.getByTestId('lightbox-tab-rich')).toBeTruthy();
    expect(screen.getByTestId('lightbox-tab-shot')).toBeTruthy();
    fireEvent.click(screen.getByTestId('lightbox-tab-shot'));
    expect(screen.getByAltText('表格原文截图').getAttribute('src')).toContain('tbl_0004_shot');
    fireEvent.click(screen.getByTestId('lightbox-close'));
    expect(screen.queryByTestId('lightbox')).toBeNull();
  });

  it('无截图时 Lightbox 仅富渲染单 tab', () => {
    const resources: Record<string, Resource> = { tbl_0004: { kind: 'table', raw_xml: tableHtml } };
    render(<TableCard docId="d" block={block} resources={resources} />);
    fireEvent.click(screen.getByTestId('table-expand-b_0004'));
    expect(screen.getByTestId('lightbox-tab-rich')).toBeTruthy();
    expect(screen.queryByTestId('lightbox-tab-shot')).toBeNull();
  });

  it('HTML 损坏（无 table）→ 降级 _shot 截图', () => {
    const resources: Record<string, Resource> = {
      tbl_0004: { kind: 'table', raw_xml: 'garbage' },
      tbl_0004_shot: { kind: 'image', mime: 'image/jpeg' },
    };
    render(<TableCard docId="d" block={block} resources={resources} />);
    expect(screen.getByTestId('table-shot-b_0004').getAttribute('src')).toContain('tbl_0004_shot');
  });

  it('HTML 与截图均缺 → preview 文本', () => {
    const resources: Record<string, Resource> = { tbl_0004: { kind: 'table' } };
    render(<TableCard docId="d" block={block} resources={resources} />);
    expect(screen.getByTestId('table-preview-b_0004').textContent).toBe('Method Acc');
  });

  it('script 注入被过滤', () => {
    const evil = '<table><tr><td><script>alert(1)</script>ok</td></tr></table>';
    const resources: Record<string, Resource> = { tbl_0004: { kind: 'table', raw_xml: evil } };
    render(<TableCard docId="d" block={block} resources={resources} />);
    expect(document.querySelector('script')).toBeNull();
    expect(screen.getByTestId('table-body-b_0004').textContent).toContain('ok');
  });

  it('超过 8 行折叠渐隐（max-h 容器）', () => {
    const rows = Array.from({ length: 10 }, (_, i) => `<tr><td>r${i}</td></tr>`).join('');
    const resources: Record<string, Resource> = {
      tbl_0004: { kind: 'table', raw_xml: `<table>${rows}</table>` },
    };
    render(<TableCard docId="d" block={block} resources={resources} />);
    expect(screen.getByTestId('table-body-b_0004').className).toContain('max-h-56');
  });
});
