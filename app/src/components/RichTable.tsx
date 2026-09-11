import { renderTableMath, tableInner } from '../utils/tableHtml';

interface Props {
  /** 原始 HTML（组件内 sanitize + 剥外壳） */
  html: string;
  testId?: string;
}

/**
 * 净化后的表格富渲染（v2.1 T3.2）：
 * 白名单 sanitize、外壳样式自控、宽表横向滚动适配全宽栏。
 * v2.3 B2：单元格数学双编码渲染（LaTeX $...$ → KaTeX；Unicode ± 原样）。
 */
export default function RichTable({ html, testId }: Props) {
  const inner = tableInner(html);
  if (!inner) return null;
  return (
    <div className="w-full overflow-x-auto" data-testid={testId}>
      <table
        className="w-full border-collapse"
        style={{ fontSize: 'calc(var(--reading-fs) * 0.92)' }}
        dangerouslySetInnerHTML={{ __html: renderTableMath(inner) }}
      />
    </div>
  );
}
