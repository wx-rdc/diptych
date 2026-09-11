import DOMPurify from 'dompurify';
import katex from 'katex';
import { splitInlineMath } from './inlineMath';

/** 表格 HTML 白名单净化（v2.1 T3.2）：失败/无表返回 ''（降级信号）。
 * v2.3 B2：白名单补内联标签（MinerU 单元格含 <sup> 等标记，与 RichText 同口径） */
export function sanitizeTableHtml(html: string): string {
  if (!html) return '';
  const out = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: [
      'table', 'thead', 'tbody', 'tfoot', 'tr', 'td', 'th', 'caption', 'colgroup', 'col',
      'sup', 'sub', 'b', 'strong', 'i', 'em',
    ],
    ALLOWED_ATTR: ['colspan', 'rowspan', 'align'],
  });
  return out.includes('<table') ? out : '';
}

/** 统计行列数（首行单元格 colspan 求和 = 列数；无表 {0,0}） */
export function tableShape(html: string): { rows: number; cols: number } {
  const clean = sanitizeTableHtml(html);
  if (!clean) return { rows: 0, cols: 0 };
  const doc = new DOMParser().parseFromString(clean, 'text/html');
  const rows = doc.querySelectorAll('tr');
  if (rows.length === 0) return { rows: 0, cols: 0 };
  let cols = 0;
  rows[0].querySelectorAll('td,th').forEach((c) => {
    cols += Number(c.getAttribute('colspan') ?? 1);
  });
  return { rows: rows.length, cols };
}

/** 去掉最外层 <table> 标签仅留内容（外壳样式由 RichTable 控制） */
export function tableInner(html: string): string {
  return sanitizeTableHtml(html).replace(/^<table[^>]*>/, '').replace(/<\/table>$/, '');
}

/** 去标签纯文本（preview 降级用），maxLength 截断 */
export function stripTags(html: string, maxLength = 80): string {
  const text = html.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim();
  return text.length > maxLength ? text.slice(0, maxLength) : text;
}

/** 表格单元格数学双编码渲染（v2.3 B2）：MinerU 单元格数学存在 LaTeX（$...$）与
 * Unicode（±）两种编码；对 td/th 的文本节点做行内公式切分 + KaTeX 渲染，
 * 既有白名单标记（<sup> 等）结构不动。无 $ 时 inner 字节不变。 */
export function renderTableMath(inner: string): string {
  if (!inner.includes('$')) return inner;
  const doc = new DOMParser().parseFromString(`<table>${inner}</table>`, 'text/html');
  for (const cell of doc.querySelectorAll('td,th')) {
    const textNodes = [...cell.childNodes].filter(
      (n): n is Text => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').includes('$'),
    );
    for (const node of textNodes) {
      const frag = doc.createElement('span');
      for (const part of splitInlineMath(node.textContent ?? '')) {
        if (part.type === 'text') {
          frag.appendChild(doc.createTextNode(part.content));
        } else {
          const holder = doc.createElement('span');
          holder.innerHTML = katex.renderToString(part.content, { throwOnError: false });
          frag.appendChild(holder);
        }
      }
      cell.replaceChild(frag, node);
    }
  }
  return doc.body.innerHTML.replace(/^<table>/, '').replace(/<\/table>$/, '');
}
