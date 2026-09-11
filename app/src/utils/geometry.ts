/** v2.2 T2：左栏内容裁切几何（纯函数，PDF 点坐标系，与 pdf.js viewport 同系） */

export type BBox = [number, number, number, number];

const BLEED = 8; // pt：内容 bbox 外扩白边
const MIN_AREA_RATIO = 0.2; // 内容 bbox 面积 < 20% 页面 → 回退整页（纯图页/稀疏页保护）

/** 某页内容 bbox = union(该页 IR 块 bbox) + bleed，clamp 页内；退化/过小回退整页 */
export function contentBboxForPage(
  blocks: { id: string; meta: { page_idx: number; bbox: BBox } }[],
  pageIdx: number,
  pageSize: [number, number],
): BBox {
  const onPage = blocks.filter((b) => b.meta.page_idx === pageIdx);
  if (onPage.length === 0) return [0, 0, pageSize[0], pageSize[1]];
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const { meta } of onPage) {
    const [a, b, c, d] = meta.bbox;
    x0 = Math.min(x0, a);
    y0 = Math.min(y0, b);
    x1 = Math.max(x1, c);
    y1 = Math.max(y1, d);
  }
  x0 = Math.max(0, x0 - BLEED);
  y0 = Math.max(0, y0 - BLEED);
  x1 = Math.min(pageSize[0], x1 + BLEED);
  y1 = Math.min(pageSize[1], y1 + BLEED);
  const area = (x1 - x0) * (y1 - y0);
  if (area < MIN_AREA_RATIO * pageSize[0] * pageSize[1]) return [0, 0, pageSize[0], pageSize[1]];
  return [x0, y0, x1, y1];
}

/** 裁切换算：内容 bbox + 目标显示宽 → drawImage 源矩形与目标尺寸 */
export function cropRectFor(bbox: BBox, targetWidth: number) {
  const [x, y, x1, y1] = bbox;
  const w = Math.max(1, x1 - x);
  const h = Math.max(1, y1 - y);
  const scale = targetWidth / w;
  return {
    srcRect: { x, y, w, h },
    scale,
    destSize: { w: Math.round(w * scale), h: Math.round(h * scale) },
  };
}
