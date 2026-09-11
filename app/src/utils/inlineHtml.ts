import DOMPurify from 'dompurify';

/** 段落文本内联 HTML 白名单（v2.3 B1）：MinerU 文本 span 携带 <sup>/<sub> 等标记
 * （IEEE 作者行/单位行上标），白名单内标签保留渲染、白名单外剥除；
 * 无白名单标签时返回 ''（调用方走纯文本 children 路径，省 innerHTML） */
const ALLOWED = ['sup', 'sub', 'b', 'strong', 'i', 'em', 'br'];

export function sanitizeInlineHtml(text: string): string {
  if (!text.includes('<')) return '';
  const out = DOMPurify.sanitize(text, { ALLOWED_TAGS: ALLOWED, ALLOWED_ATTR: [] });
  // 含白名单标签（渲染富文本），或净化发生了剥离/转义（白名单外标签不留字面残留）
  return out !== text || ALLOWED.some((t) => out.includes(`<${t}`)) ? out : '';
}
