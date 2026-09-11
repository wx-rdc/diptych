import { splitInlineMath } from '../utils/inlineMath';
import { sanitizeInlineHtml } from '../utils/inlineHtml';
import FormulaRenderer from './FormulaRenderer';

interface Props {
  text: string;
  className?: string;
}

/** 文本 + 行内公式混排渲染（v2.1 T4.1）：$...$ / \(...\) → KaTeX，失败红框降级不炸整段。
 * v2.3 B1：文本段再做内联 HTML 白名单（sup/sub/b/i 等）——MinerU span 携带的
 * <sup>1</sup> 等标记渲染为真实元素，白名单外剥除（DOMPurify） */
export default function RichText({ text, className }: Props) {
  const parts = splitInlineMath(text);
  return (
    <span className={className}>
      {parts.map((p, i) => {
        if (p.type !== 'text') {
          return <FormulaRenderer key={i} latex={p.content} displayMode={false} />;
        }
        const html = sanitizeInlineHtml(p.content);
        return html ? (
          <span key={i} dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <span key={i}>{p.content}</span>
        );
      })}
    </span>
  );
}
