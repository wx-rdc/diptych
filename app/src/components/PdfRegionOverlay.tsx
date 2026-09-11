import type { CSSProperties } from 'react';
import type { RenderMeta } from '../types';
import type { BBox } from '../utils/geometry';

export interface RegionDef {
  blockId: string;
  meta: RenderMeta;
}

interface Props {
  /** 当前页的块区域（已按 page_idx 过滤） */
  regions: RegionDef[];
  activeId: string | null;
  onBlockClick: (blockId: string) => void;
  /** PDF 原始页面尺寸（点） */
  pageSize: [number, number];
  /** v2.2 T2：内容裁切 bbox（PDF 点）——存在时区域相对内容区定位（canvas 已裁掉纸张白边） */
  contentBbox?: BBox | null;
}

/**
 * bbox（PDF 点，左上原点）→ 显示区百分比定位。
 * 叠加层容器与 canvas 同为 inset-0/w-full/h-full，百分比换算与渲染分辨率无关。
 * contentBbox 存在时（裁切渲染），坐标先平移到内容区原点再除以内容宽高。
 * 纯函数导出供单测；组件仅做布局与交互。
 */
export function regionStyle(
  bbox: [number, number, number, number],
  pageSize: [number, number],
  contentBbox?: BBox | null,
): CSSProperties {
  const [x1, y1, x2, y2] = bbox;
  const [bx0, by0, bx1, by1] = contentBbox ?? [0, 0, pageSize[0], pageSize[1]];
  const ox = bx0;
  const oy = by0;
  const ow = Math.max(1, bx1 - bx0);
  const oh = Math.max(1, by1 - by0);
  const pct = (v: number, total: number) => `${(Math.max(0, v) / total) * 100}%`;
  return {
    left: pct(x1 - ox, ow),
    top: pct(y1 - oy, oh),
    width: `${(Math.max(0, x2 - x1) / ow) * 100}%`,
    height: `${(Math.max(0, y2 - y1) / oh) * 100}%`,
  };
}

/** PDF 页区域叠加层：透明容器 + 可点击区域块（z-index 确保位于 canvas 之上） */
export default function PdfRegionOverlay({
  regions,
  activeId,
  onBlockClick,
  pageSize,
  contentBbox,
}: Props) {
  return (
    <div
      className="absolute inset-0 z-10"
      data-testid="region-overlay"
      style={{ pointerEvents: 'none' }}
    >
      {regions.map(({ blockId, meta }) => {
        const style = regionStyle(meta.bbox, pageSize, contentBbox);
        const active = activeId === blockId;
        return (
          <div
            key={blockId}
            role="button"
            aria-label={`区域 ${blockId}`}
            data-testid={`region-${blockId}`}
            data-block-id={blockId}
            data-active={active ? 'true' : undefined}
            className={
              'absolute rounded transition-colors ' +
              (active ? 'bg-blue-400/15' : 'hover:bg-blue-200/10')
            }
            style={{
              ...style,
              pointerEvents: 'auto',
              cursor: 'pointer',
              boxShadow: active ? 'inset 3px 0 0 #3b82f6' : undefined,
            }}
            onClick={() => onBlockClick(blockId)}
          />
        );
      })}
    </div>
  );
}
