/** v2.2 T6：块锚点插值同步——blockId → 滚动容器内像素 y 的锚点表 + 二分查找 */

import type { GDocument } from '../types';
import type { BBox } from './geometry';

export interface Anchor {
  blockId: string;
  y: number;
}

/** 二分查找最后一个 y <= viewportTop 的锚点。
 * viewportTop 在首锚点之上（含负值）→ 首锚点（文档顶部对齐首块）；空表 → null */
export function binaryFindAnchor(anchors: Anchor[], viewportTop: number): Anchor | null {
  if (anchors.length === 0) return null;
  let lo = 0;
  let hi = anchors.length - 1;
  let best: Anchor | null = null;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (anchors[mid].y <= viewportTop) {
      best = anchors[mid];
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best ?? anchors[0];
}

/**
 * 左锚点表：region 像素 y = pageEl.offsetTop + (bbox.y0 − contentBbox.y0) × scale。
 * scale 取页 wrapper 的渲染高 / 内容 bbox 高——wrapper 的 aspect-ratio 即内容高宽比
 * （v2.2 T2），与 canvas 是否懒渲染无关（避免未渲染页锚点压缩、同步漂移）。
 */
export function buildLeftAnchors(
  doc: GDocument,
  contentBboxes: Map<number, BBox>,
  pageEls: Map<number, HTMLElement>,
): Anchor[] {
  const out: Anchor[] = [];
  const scaleCache = new Map<number, number>();
  for (const block of doc.blocks) {
    const m = doc.render_meta?.[block.id];
    if (!m) continue;
    const pageEl = pageEls.get(m.page_idx + 1);
    const cb = contentBboxes.get(m.page_idx);
    if (!pageEl || !cb) continue;
    let scale = scaleCache.get(m.page_idx);
    if (scale === undefined) {
      const contentH = cb[3] - cb[1];
      scale = contentH > 0 && pageEl.clientHeight > 0 ? pageEl.clientHeight / contentH : 1;
      scaleCache.set(m.page_idx, scale);
    }
    const y = pageEl.offsetTop + (m.bbox[1] - cb[1]) * scale;
    if (Number.isFinite(y)) out.push({ blockId: block.id, y });
  }
  out.sort((a, b) => a.y - b.y);
  return out;
}

/** 右锚点表：blockId → 右栏滚动容器内 offsetTop */
export function buildRightAnchors(doc: GDocument, rightContainer: HTMLElement): Anchor[] {
  const out: Anchor[] = [];
  for (const block of doc.blocks) {
    const el = rightContainer.querySelector<HTMLElement>(`[data-block-id="${block.id}"]`);
    if (el) out.push({ blockId: block.id, y: el.offsetTop });
  }
  return out;
}
