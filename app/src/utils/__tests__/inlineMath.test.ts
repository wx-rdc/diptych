import { describe, expect, it } from 'vitest';
import { splitInlineMath, countInlineMath } from '../inlineMath';

describe('splitInlineMath（T4.4）', () => {
  it('无公式 → 单 text 段', () => {
    expect(splitInlineMath('plain text only')).toEqual([{ type: 'text', content: 'plain text only' }]);
  });

  it('单公式切三段', () => {
    expect(splitInlineMath('精度为 $10^{-3}$ 米')).toEqual([
      { type: 'text', content: '精度为 ' },
      { type: 'math', content: '10^{-3}' },
      { type: 'text', content: ' 米' },
    ]);
  });

  it('多公式', () => {
    const parts = splitInlineMath('$a$ x $b$');
    expect(parts.filter((p) => p.type === 'math').map((p) => p.content)).toEqual(['a', 'b']);
  });

  it('转义 \\$ 不视为定界符', () => {
    expect(splitInlineMath('价格 \\$5 与 $x$')).toEqual([
      { type: 'text', content: '价格 \\$5 与 ' },
      { type: 'math', content: 'x' },
    ]);
  });

  it('跨行不匹配（$ 内无换行）', () => {
    expect(splitInlineMath('a $b\nc d')).toEqual([{ type: 'text', content: 'a $b\nc d' }]);
  });

  it('\\( \\) 形式', () => {
    expect(splitInlineMath('值 \\(E=mc^2\\) 结束')).toEqual([
      { type: 'text', content: '值 ' },
      { type: 'math', content: 'E=mc^2' },
      { type: 'text', content: ' 结束' },
    ]);
  });

  it('空公式 $$ 不匹配', () => {
    expect(splitInlineMath('a $$ b')).toEqual([{ type: 'text', content: 'a $$ b' }]);
  });
});

describe('countInlineMath（配平口径）', () => {
  it('与 split 一致', () => {
    expect(countInlineMath('a $x$ b \\(y\\) $z$')).toBe(3);
    expect(countInlineMath('no math')).toBe(0);
    expect(countInlineMath('价 \\$5 与 $x$')).toBe(1);
  });
});
