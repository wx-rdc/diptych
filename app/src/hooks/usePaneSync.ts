import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';

/**
 * v2.7 双栏交互（feedback_0920 滚动收口）：
 * a) Hover 联动高亮——mouseover 委托提取 data-block-id（16ms rAF 合帧），双侧同步 .g-block-hover；
 * b) Click 居中——点击段落时，双侧同 id 块 scrollIntoView({block:'center'}) 平滑居中。
 * v2.4 的 hover/滚动驱动自动对齐（滚动锁、锚点追踪、比例降级）已整体移除：
 * 两栏滚动完全独立，仅点击段落触发对齐。
 */
export type PaneSide = 'left' | 'right';

const HOVER_CLASS = 'g-block-hover';

/** 事件目标 → 所属块 id（无块返回 null） */
export function blockIdFromElement(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null;
  return target.closest<HTMLElement>('[data-block-id]')?.dataset.blockId ?? null;
}

export function usePaneSync(
  leftRef: RefObject<HTMLDivElement>,
  rightRef: RefObject<HTMLDivElement>,
): void {
  const hoveredIdRef = useRef<string | null>(null);

  useEffect(() => {
    const paintHover = (id: string | null) => {
      for (const pane of [leftRef.current, rightRef.current]) {
        if (!pane) continue;
        pane.querySelectorAll('.' + HOVER_CLASS).forEach((el) => el.classList.remove(HOVER_CLASS));
        if (id) {
          pane
            .querySelectorAll(`[data-block-id="${CSS.escape(id)}"]`)
            .forEach((el) => el.classList.add(HOVER_CLASS));
        }
      }
    };

    const cleanups: Array<() => void> = [];
    for (const pane of [leftRef.current, rightRef.current]) {
      if (!pane) continue;

      const onLeave = () => {
        hoveredIdRef.current = null;
        paintHover(null);
      };
      let hoverRaf = 0;
      let pendingId: string | null | undefined; // undefined = 无待处理事件
      const onOver = (e: MouseEvent) => {
        // 16ms 合帧：帧内多次 mouseover 只提交最后一次的 id（防抖但不丢末次）
        pendingId = blockIdFromElement(e.target);
        if (hoverRaf) return;
        hoverRaf = requestAnimationFrame(() => {
          hoverRaf = 0;
          if (pendingId !== undefined && pendingId !== hoveredIdRef.current) {
            hoveredIdRef.current = pendingId;
            paintHover(pendingId);
          }
          pendingId = undefined;
        });
      };
      const onClick = (e: MouseEvent) => {
        const id = blockIdFromElement(e.target);
        if (!id) return;
        for (const pane2 of [leftRef.current, rightRef.current]) {
          pane2
            ?.querySelector(`[data-block-id="${CSS.escape(id)}"]`)
            // jsdom 等无 scrollIntoView 的环境跳过（居中降级为仅高亮）
            ?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
        }
      };

      pane.addEventListener('mouseleave', onLeave);
      pane.addEventListener('mouseover', onOver);
      pane.addEventListener('click', onClick);
      cleanups.push(() => {
        pane.removeEventListener('mouseleave', onLeave);
        pane.removeEventListener('mouseover', onOver);
        pane.removeEventListener('click', onClick);
        if (hoverRaf) cancelAnimationFrame(hoverRaf);
      });
    }
    return () => {
      cleanups.forEach((fn) => fn());
      paintHover(null);
    };
  }, [leftRef, rightRef]);
}
