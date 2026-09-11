import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';

/**
 * v2.4 Hover 驱动主从同步（feedback_0911 T3/T4）：
 * a) 滚动锁——mouseenter/leave 决定 master；仅 master 的滚动事件驱动同步；
 * b) Hover 联动——mouseover 委托提取 data-block-id（16ms rAF 合帧），双侧同步 .g-block-hover；
 * c) Click 居中——click 双侧 scrollIntoView({block:'center'})；
 * d) 锚点追踪——master 滚动时 slave 以 hovered 块「居中」为目标 rAF Lerp；
 *    hovered 缺失（占位符/页缝）降级比例同步；master=null 停止追踪。
 * 防回授：slave 动画滚动窗（200ms 滚动续期）+ click 静默窗（1200ms）+ master 校验。
 */
export type PaneSide = 'left' | 'right';

const HOVER_CLASS = 'g-block-hover';
const PROGRAMMATIC_MS = 1200;
const LERP_FACTOR = 0.25;

/** 事件目标 → 所属块 id（无块返回 null） */
export function blockIdFromElement(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null;
  return target.closest<HTMLElement>('[data-block-id]')?.dataset.blockId ?? null;
}

/** 让块中心对齐 pane 视口中心所需的 scrollTop（rect 差值法，兼容任意 offsetParent） */
export function centerScrollTop(pane: HTMLElement, el: HTMLElement): number {
  return Math.max(
    0,
    pane.scrollTop +
      (el.getBoundingClientRect().top - pane.getBoundingClientRect().top) +
      el.offsetHeight / 2 -
      pane.clientHeight / 2,
  );
}

/** 比例同步：master 可滚比例 → slave scrollTop（hovered 缺失时的降级路径） */
export function proportionalScrollTop(master: HTMLElement, slave: HTMLElement): number {
  const m = master.scrollHeight - master.clientHeight;
  if (m <= 0) return 0;
  return (master.scrollTop / m) * Math.max(0, slave.scrollHeight - slave.clientHeight);
}

/** 阻尼逼近一步；收敛（<1px）时 snap 到目标终止动画 */
export function lerpTowards(current: number, target: number, factor = LERP_FACTOR): number {
  const next = current + (target - current) * factor;
  return Math.abs(target - next) < 1 ? target : next;
}

export function useHoverDrivenSync(
  leftRef: RefObject<HTMLDivElement>,
  rightRef: RefObject<HTMLDivElement>,
): void {
  const masterRef = useRef<PaneSide | null>(null);
  const hoveredIdRef = useRef<string | null>(null);
  const programmaticRef = useRef<Record<PaneSide, number>>({ left: 0, right: 0 });
  const rafRef = useRef(0);

  useEffect(() => {
    const paneOf = (side: PaneSide) => (side === 'left' ? leftRef.current : rightRef.current);
    const other = (side: PaneSide): PaneSide => (side === 'left' ? 'right' : 'left');

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

    const stopTracking = () => {
      if (rafRef.current) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = 0;
      }
    };

    /** rAF 追踪循环：slave 朝 hovered 块居中目标 Lerp（无 hovered → 比例） */
    const track = () => {
      rafRef.current = 0;
      const master = masterRef.current;
      if (!master) return;
      const mPane = paneOf(master);
      const sSide = other(master);
      const sPane = paneOf(sSide);
      if (!mPane || !sPane) return;
      const id = hoveredIdRef.current;
      const el = id
        ? sPane.querySelector<HTMLElement>(`[data-block-id="${CSS.escape(id)}"]`)
        : null;
      const target = el ? centerScrollTop(sPane, el) : proportionalScrollTop(mPane, sPane);
      const next = lerpTowards(sPane.scrollTop, target);
      if (next !== sPane.scrollTop) {
        // 动画滚动窗滚动续期：窗口内 slave 的 scroll 事件不反向驱动
        programmaticRef.current[sSide] = performance.now() + 200;
        sPane.scrollTop = next;
      }
      if (Math.abs(target - next) >= 1) rafRef.current = requestAnimationFrame(track);
    };

    const cleanups: Array<() => void> = [];
    const sides: Array<[PaneSide, HTMLDivElement | null]> = [
      ['left', leftRef.current],
      ['right', rightRef.current],
    ];
    for (const [side, pane] of sides) {
      if (!pane) continue;

      const onEnter = () => {
        masterRef.current = side;
      };
      const onLeave = () => {
        if (masterRef.current === side) masterRef.current = null;
        hoveredIdRef.current = null;
        paintHover(null);
        stopTracking();
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
        const now = performance.now();
        programmaticRef.current.left = now + PROGRAMMATIC_MS;
        programmaticRef.current.right = now + PROGRAMMATIC_MS;
        for (const pane2 of [leftRef.current, rightRef.current]) {
          pane2
            ?.querySelector(`[data-block-id="${CSS.escape(id)}"]`)
            // jsdom 等无 scrollIntoView 的环境跳过（居中降级为仅高亮）
            ?.scrollIntoView?.({ behavior: 'smooth', block: 'center' });
        }
      };
      const onScroll = () => {
        if (masterRef.current !== side) return; // 滚动锁：仅 master 驱动
        if (performance.now() < programmaticRef.current[side]) return; // 被程序滚动
        if (!rafRef.current) rafRef.current = requestAnimationFrame(track);
      };

      pane.addEventListener('mouseenter', onEnter);
      pane.addEventListener('mouseleave', onLeave);
      pane.addEventListener('mouseover', onOver);
      pane.addEventListener('click', onClick);
      pane.addEventListener('scroll', onScroll, { passive: true });
      cleanups.push(() => {
        pane.removeEventListener('mouseenter', onEnter);
        pane.removeEventListener('mouseleave', onLeave);
        pane.removeEventListener('mouseover', onOver);
        pane.removeEventListener('click', onClick);
        pane.removeEventListener('scroll', onScroll);
        if (hoverRaf) cancelAnimationFrame(hoverRaf);
      });
    }
    return () => {
      cleanups.forEach((fn) => fn());
      stopTracking();
      paintHover(null);
    };
  }, [leftRef, rightRef]);
}
