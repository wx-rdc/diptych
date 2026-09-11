import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import PdfRegionOverlay, { regionStyle, type RegionDef } from '../PdfRegionOverlay';
import type { RenderMeta } from '../../types';

const pageSize: [number, number] = [612, 792];

const meta = (bbox: [number, number, number, number], page = 0): RenderMeta => ({
  page_idx: page,
  bbox,
});

describe('regionStyle（纯函数）', () => {
  it('bbox 换算为页内百分比定位', () => {
    const s = regionStyle([100, 200, 400, 500], pageSize);
    expect(s.left).toBe(`${(100 / 612) * 100}%`);
    expect(s.top).toBe(`${(200 / 792) * 100}%`);
    expect(s.width).toBe(`${(300 / 612) * 100}%`);
    expect(s.height).toBe(`${(300 / 792) * 100}%`);
  });

  it('退化 bbox（零宽高）不产生负尺寸', () => {
    const s = regionStyle([10, 10, 10, 10], pageSize);
    expect(s.width).toBe('0%');
    expect(s.height).toBe('0%');
  });
});

describe('PdfRegionOverlay', () => {
  const regions: RegionDef[] = [
    { blockId: 'b_0001', meta: meta([50, 60, 500, 100]) },
    { blockId: 'b_0002', meta: meta([50, 200, 300, 260], 0) },
  ];

  it('渲染区域块且点击回调带 blockId', () => {
    const onBlockClick = vi.fn();
    render(
      <PdfRegionOverlay
        regions={regions}
        activeId={null}
        onBlockClick={onBlockClick}
        pageSize={pageSize}
      />,
    );
    const r1 = screen.getByTestId('region-b_0001');
    const r2 = screen.getByTestId('region-b_0002');
    expect(r1).toBeTruthy();
    expect(r2).toBeTruthy();
    fireEvent.click(r2);
    expect(onBlockClick).toHaveBeenCalledWith('b_0002');
  });

  it('activeId 对应区域带高亮样式', () => {
    const { rerender } = render(
      <PdfRegionOverlay
        regions={regions}
        activeId="b_0001"
        onBlockClick={() => {}}
        pageSize={pageSize}
      />,
    );
    const active = screen.getByTestId('region-b_0001');
    expect(active.dataset.active).toBe('true'); // v2.1 T6：高亮改为 data-active + inset 蓝条
    // 切换 active
    rerender(
      <PdfRegionOverlay
        regions={regions}
        activeId="b_0002"
        onBlockClick={() => {}}
        pageSize={pageSize}
      />,
    );
    expect(screen.getByTestId('region-b_0001').dataset.active).toBeUndefined();
    expect(screen.getByTestId('region-b_0002').dataset.active).toBe('true');
  });

  it('空区域列表渲染空容器', () => {
    const { container } = render(
      <PdfRegionOverlay
        regions={[]}
        activeId={null}
        onBlockClick={() => {}}
        pageSize={pageSize}
      />,
    );
    expect(container.querySelector('[data-testid="region-overlay"]')).toBeTruthy();
    expect(container.querySelectorAll('[role="button"]').length).toBe(0);
  });

  it('v2.2：contentBbox 存在时相对内容区定位（裁切渲染对齐）', () => {
    const s = regionStyle([100, 50, 500, 150], [612, 792], [82, 42, 528, 708]);
    expect(s.left).toBe(`${((100 - 82) / (528 - 82)) * 100}%`);
    expect(s.top).toBe(`${((50 - 42) / (708 - 42)) * 100}%`);
    expect(s.width).toBe(`${(400 / (528 - 82)) * 100}%`);
    expect(s.height).toBe(`${(100 / (708 - 42)) * 100}%`);
  });

  it('v2.2：contentBbox=null 时保持整页定位（回退兼容）', () => {
    const a = regionStyle([100, 200, 400, 500], [612, 792]);
    const b = regionStyle([100, 200, 400, 500], [612, 792], null);
    expect(a).toEqual(b);
  });
});
