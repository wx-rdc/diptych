export type InlinePart = { type: 'text' | 'math'; content: string };

// $...$（单行、非空、$ 前无反斜杠）或 \(...\)（单行）
const MATH_RE = /(?<!\\)\$([^$\n]+?)(?<!\\)\$|\\\(([^)\n]*?)\\\)/g;

/** 切分行内公式（v2.1 T4.1）。math 段 content 已剥定界符。 */
export function splitInlineMath(text: string): InlinePart[] {
  const parts: InlinePart[] = [];
  let last = 0;
  for (const m of text.matchAll(MATH_RE)) {
    const idx = m.index ?? 0;
    if (idx > last) parts.push({ type: 'text', content: text.slice(last, idx) });
    parts.push({ type: 'math', content: m[1] ?? m[2] ?? '' });
    last = idx + m[0].length;
  }
  if (last < text.length) parts.push({ type: 'text', content: text.slice(last) });
  return parts;
}

/** 行内公式计数（与 Rust math_guard::count_inline_math 规则一致） */
export function countInlineMath(text: string): number {
  let n = 0;
  for (const _m of text.matchAll(MATH_RE)) n += 1;
  return n;
}
