import type { InlineContent, Resource } from '../types';
import { resourceUrl } from '../services/backend';
import FormulaRenderer from './FormulaRenderer';
import RichText from './RichText';

interface Props {
  docId: string;
  item: InlineContent;
  resources: Record<string, Resource>;
}

/** 行内内容：文本（粗斜下划线 + 行内公式）/行内图片/行内公式/换行 */
export default function InlineContentRenderer({ docId, item, resources }: Props) {
  switch (item.type) {
    case 'text': {
      const cls = [item.bold && 'font-bold', item.italic && 'italic', item.underline && 'underline']
        .filter(Boolean)
        .join(' ');
      return <RichText text={item.text} className={cls || undefined} />;
    }
    case 'inline_image':
      return (
        <img
          src={resourceUrl(docId, item.resource_id)}
          alt="行内图片"
          className="mx-0.5 inline-block max-h-8 align-middle"
        />
      );
    case 'inline_formula': {
      const latex = resources[item.resource_id]?.latex ?? null;
      return <FormulaRenderer latex={latex} displayMode={false} />;
    }
    case 'line_break':
      return <br />;
  }
}
