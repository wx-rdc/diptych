import { describe, expect, it } from 'vitest';
import { renderTableMath, sanitizeTableHtml, tableShape, tableInner, stripTags } from '../tableHtml';

const ok = '<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>';

describe('sanitizeTableHtml（T3.5）', () => {
  it('保留 table/td/th/colspan/rowspan', () => {
    const html = '<table><tr><th colspan="2">h</th></tr><tr><td rowspan="2">x</td></tr></table>';
    const out = sanitizeTableHtml(html);
    expect(out).toContain('<table');
    expect(out).toContain('colspan="2"');
    expect(out).toContain('rowspan="2"');
  });

  it('过滤 script 与 onclick', () => {
    const out = sanitizeTableHtml(
      '<table><tr><td onclick="evil()">a<script>alert(1)</script></td></tr></table>',
    );
    expect(out).not.toContain('script');
    expect(out).not.toContain('onclick');
    expect(out).toContain('a');
  });

  it('无 table → 空串（降级信号）', () => {
    expect(sanitizeTableHtml('<p>no table</p>')).toBe('');
    expect(sanitizeTableHtml('')).toBe('');
  });
});

describe('tableShape 行列统计（T3.3）', () => {
  it('行列数正确', () => {
    expect(tableShape(ok)).toEqual({ rows: 2, cols: 2 });
  });
  it('colspan 求和计入列数', () => {
    expect(tableShape('<table><tr><th colspan="3">h</th></tr></table>')).toEqual({ rows: 1, cols: 3 });
  });
  it('无表 → {0,0}', () => {
    expect(tableShape('')).toEqual({ rows: 0, cols: 0 });
    expect(tableShape('garbage')).toEqual({ rows: 0, cols: 0 });
  });
});

describe('tableInner 外壳剥离（T3.2）', () => {
  it('去最外层 table 标签保留内容（DOMPurify 可能补 tbody，属合法结构）', () => {
    const out = tableInner(ok);
    expect(out).not.toContain('<table');
    expect(out).toContain('<tr><td>a</td><td>b</td></tr>');
    expect(out).toContain('<tr><td>c</td><td>d</td></tr>');
  });
  it('无表 → 空串', () => {
    expect(tableInner('nope')).toBe('');
  });
});

describe('stripTags（T3.1 preview）', () => {
  it('去标签压缩空白并截断', () => {
    const long = 'x'.repeat(200);
    const out = stripTags(`<table><tr><td>${long}</td></tr></table>`, 80);
    expect(out).toHaveLength(80);
    expect(out).not.toContain('<');
  });
});

describe('renderTableMath（v2.3 B2 表格单元格双编码）', () => {
  it('单元格 LaTeX（$...\\pm...$）渲染为 KaTeX HTML', () => {
    const out = renderTableMath('<tr><td>$55.21 \\pm 0.91$</td></tr>');
    expect(out).toContain('katex');
    expect(out).not.toContain('$55.21');
  });

  it('Unicode 编码（±）原样保留不进 KaTeX', () => {
    const out = renderTableMath('<tr><td>55.21 ± 0.91</td></tr>');
    expect(out).toContain('±');
    expect(out).not.toContain('katex');
  });

  it('单元格内 <sup> 结构保留且公式仍渲染（B1/B2 共存）', () => {
    const out = renderTableMath('<tr><td>x<sup>2</sup> = $a^2$</td></tr>');
    expect(out).toContain('<sup>2</sup>');
    expect(out).toContain('katex');
  });

  it('纯文本单元格零改写（字节不变）', () => {
    expect(renderTableMath('<tr><td>plain</td></tr>')).toBe('<tr><td>plain</td></tr>');
  });

  it('LaTeX 与 Unicode 混合单元格：仅 LaTeX 段进 KaTeX', () => {
    const out = renderTableMath('<tr><td>$\\pm$ vs ±</td></tr>');
    expect(out).toContain('katex');
    expect(out).toContain('±');
  });
});
