import { useCallback, useEffect, useState } from 'react';

export const FONT_STEPS = [15, 17, 18, 20, 22] as const;
export const DEFAULT_FONT_SIZE = 18;
const STORAGE_KEY = 'geminus-reading-fs';

function apply(px: number) {
  document.documentElement.style.setProperty('--geminus-user-fs', `${px}px`);
}

/** 五档阅读字号（A-/A+），localStorage 持久化（v2.1 T2.2） */
export function useReadingFontSize() {
  const [fontSize, setFontSize] = useState<number>(() => {
    const saved = Number(localStorage.getItem(STORAGE_KEY));
    return (FONT_STEPS as readonly number[]).includes(saved) ? saved : DEFAULT_FONT_SIZE;
  });

  useEffect(() => {
    apply(fontSize);
    localStorage.setItem(STORAGE_KEY, String(fontSize));
  }, [fontSize]);

  const step = useCallback((dir: 1 | -1) => {
    setFontSize((cur) => {
      const i = (FONT_STEPS as readonly number[]).indexOf(cur);
      const next = Math.min(FONT_STEPS.length - 1, Math.max(0, i + dir));
      return FONT_STEPS[next];
    });
  }, []);

  return {
    fontSize,
    increase: useCallback(() => step(1), [step]),
    decrease: useCallback(() => step(-1), [step]),
  };
}
