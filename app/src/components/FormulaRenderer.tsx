import { useMemo } from 'react';
import katex from 'katex';

interface Props {
  latex?: string | null;
  displayMode?: boolean;
}

/** KaTeX 公式渲染；latex 缺失/渲染失败时降级为占位框（plan.md PoC-⑥ 降级路径）。
 * output:'html'——不生成 MathML annotation 层，DOM/innerText 不残留 raw LaTeX（v2.1 P0-2）。 */
export default function FormulaRenderer({ latex, displayMode = true }: Props) {
  const html = useMemo(() => {
    if (!latex) return null;
    try {
      return katex.renderToString(latex, { throwOnError: false, displayMode, output: 'html' });
    } catch {
      return null;
    }
  }, [latex, displayMode]);

  if (!html) {
    return (
      <span
        className="inline-flex items-center gap-1 rounded border border-amber-300 bg-amber-50 px-2 py-0.5 text-sm text-amber-700"
        title={latex ?? '公式（无 LaTeX）'}
      >
        ∑ [公式]
      </span>
    );
  }
  return displayMode ? (
    <div className="my-3 overflow-x-auto text-center" dangerouslySetInnerHTML={{ __html: html }} />
  ) : (
    <span className="inline-block" dangerouslySetInnerHTML={{ __html: html }} />
  );
}
