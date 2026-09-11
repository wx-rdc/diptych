import { describe, expect, it } from 'vitest';
import { binaryFindAnchor, type Anchor } from '../anchor';

const anchors: Anchor[] = [
  { blockId: 'b_0001', y: 0 },
  { blockId: 'b_0002', y: 300 },
  { blockId: 'b_0003', y: 800 },
  { blockId: 'b_0004', y: 1200 },
];

describe('binaryFindAnchor（v2.2 T6）', () => {
  it('viewportTop 在两锚点间 → 返回前者', () => {
    expect(binaryFindAnchor(anchors, 400)?.blockId).toBe('b_0002');
    expect(binaryFindAnchor(anchors, 1000)?.blockId).toBe('b_0003');
  });

  it('viewportTop 恰等于锚点 → 返回该锚点', () => {
    expect(binaryFindAnchor(anchors, 800)?.blockId).toBe('b_0003');
  });

  it('页首 → 首锚点', () => {
    expect(binaryFindAnchor(anchors, 0)?.blockId).toBe('b_0001');
  });

  it('页尾之后 → 末锚点', () => {
    expect(binaryFindAnchor(anchors, 99999)?.blockId).toBe('b_0004');
  });

  it('空表 → null；负值 → 首锚点', () => {
    expect(binaryFindAnchor([], 100)).toBeNull();
    expect(binaryFindAnchor(anchors, -5)?.blockId).toBe('b_0001');
  });
});
