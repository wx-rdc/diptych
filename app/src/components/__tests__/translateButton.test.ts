import { describe, expect, it } from 'vitest';
import { translateButtonLabel, type TranslateButtonState } from '../Toolbar';

describe('translateButtonLabel 状态机（T7.1）', () => {
  it('idle → 翻译', () => {
    expect(translateButtonLabel({ kind: 'idle' })).toBe('翻译');
  });

  it('translating → 带进度 n/total', () => {
    expect(translateButtonLabel({ kind: 'translating', current: 3, total: 12 })).toBe(
      '翻译中 3/12…',
    );
  });

  it('done 且零错误 → 已完成 ✓ 重新翻译；有错误 → 提示未翻译段数', () => {
    expect(translateButtonLabel({ kind: 'done', errors: 0 })).toBe('已完成 ✓ 重新翻译');
    expect(translateButtonLabel({ kind: 'done', errors: 2 })).toBe('完成（2 段未翻译）· 重译');
  });

  it('error → 重试（error→retry 流转可再次点击）', () => {
    expect(translateButtonLabel({ kind: 'error' })).toBe('翻译出错 · 重试');
  });

  it('状态全集覆盖（穷举 kind）', () => {
    const states: TranslateButtonState[] = [
      { kind: 'idle' },
      { kind: 'translating', current: 0, total: 0 },
      { kind: 'done', errors: 0 },
      { kind: 'error' },
    ];
    for (const s of states) expect(translateButtonLabel(s).length).toBeGreaterThan(0);
  });
});
