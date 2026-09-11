import { describe, expect, it } from 'vitest';
import { contentBboxForPage, cropRectFor, type BBox } from '../geometry';

const PAGE: [number, number] = [612, 792];
const meta = (id: string, bbox: BBox) => ({ id, meta: { page_idx: 0, bbox } });

describe('contentBboxForPage', () => {
  it('union 各块 bbox + 8pt bleed 并 clamp 到页面', () => {
    const blocks = [meta('a', [100, 50, 500, 100]), meta('b', [90, 400, 520, 700])];
    const r = contentBboxForPage(blocks, 0, PAGE);
    // union=[90,50,520,700]，bleed 8 → [82,42,528,708]，均在页内
    expect(r).toEqual([82, 42, 528, 708]);
  });

  it('只统计当前页的块', () => {
    const blocks = [
      meta('a', [90, 50, 520, 700]),
      { id: 'b', meta: { page_idx: 3, bbox: [1, 1, 2, 2] as BBox } },
    ];
    const r = contentBboxForPage(blocks, 0, PAGE);
    expect(r).toEqual([82, 42, 528, 708]); // 不受他页块影响
  });

  it('无块时返回整页（退化）', () => {
    expect(contentBboxForPage([], 0, PAGE)).toEqual([0, 0, 612, 792]);
  });

  it('bbox 面积 < 20% 页面 → 回退整页（纯图页保护）', () => {
    const blocks = [meta('a', [280, 300, 340, 360])]; // 60x60=3600 << 484704*0.2
    expect(contentBboxForPage(blocks, 0, PAGE)).toEqual([0, 0, 612, 792]);
  });

  it('bleed 不越过页面边界', () => {
    const blocks = [meta('a', [2, 3, 610, 789])];
    expect(contentBboxForPage(blocks, 0, PAGE)).toEqual([0, 0, 612, 792]);
  });
});

describe('cropRectFor', () => {
  it('按目标宽换算源/目标矩形', () => {
    const c = cropRectFor([82, 42, 528, 708], 400);
    expect(c.srcRect).toEqual({ x: 82, y: 42, w: 446, h: 666 });
    expect(c.scale).toBeCloseTo(400 / 446);
    expect(c.destSize.w).toBe(400);
    expect(c.destSize.h).toBe(Math.round((400 / 446) * 666));
  });

  it('零宽 bbox 防御（w 至少 1）', () => {
    const c = cropRectFor([100, 100, 100, 200], 300);
    expect(c.srcRect.w).toBe(1);
    expect(c.destSize.w).toBe(300);
  });
});
