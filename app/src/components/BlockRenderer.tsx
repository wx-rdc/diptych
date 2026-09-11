import type { Block, Resource } from '../types';
import AlgorithmCard from './AlgorithmCard';
import InlineContentRenderer from './InlineContentRenderer';
import ParagraphToolbar, { type RetranslatedPayload } from './ParagraphToolbar';
import PlaceholderRenderer from './PlaceholderRenderer';

/** 译文侧段落交互上下文（v2.1 T7.2） */
export interface ParagraphToolbarInteractive {
  docId: string;
  /** block_id → 译文纯文本（查表函数，null=未翻译） */
  translatedText: (blockId: string) => string | null;
  sourceLang: string;
  targetLang: string;
  onRetranslated: (p: RetranslatedPayload) => void;
}

interface Props {
  docId: string;
  block: Block;
  resources: Record<string, Resource>;
  activeId?: string | null;
  onSelect?: (blockId: string) => void;
  /** 译文侧 fallback 段：内容前打「未翻译」徽标（v2.1 T1） */
  badgeFallback?: boolean;
  /** 译文侧段落交互（hover 工具条：复制/重译，v2.1 T7.2）；undefined=原文侧 */
  interactive?: ParagraphToolbarInteractive;
}

const HEADING_TAGS = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'] as const;
// 标题梯度相对正文字号（em）——随 --reading-fs 五档联动（v2.1 T2.2）
const HEADING_CLASSES: Record<number, string> = {
  1: 'text-[1.6em] font-bold mt-6 mb-3',
  2: 'text-[1.35em] font-bold mt-5 mb-2',
  3: 'text-[1.15em] font-semibold mt-4 mb-2',
  4: 'text-[1.05em] font-semibold mt-3 mb-1',
  5: 'text-[1em] font-semibold mt-3 mb-1',
  6: 'text-[1em] font-medium mt-2 mb-1',
};

const ALIGN_CLASSES: Record<string, string> = {
  left: 'text-left',
  center: 'text-center',
  right: 'text-right',
  justify: 'text-justify',
};

/** 块渲染器：标题层级/对齐/缩进/列表前缀/占位符保位 */
export default function BlockRenderer({ docId, block, resources, activeId, onSelect, badgeFallback, interactive }: Props) {
  // v2.1 T6.1：高亮降噪——左侧 4px 蓝条 + 淡底（替代四边 ring）
  const active = activeId === block.id;
  const ring = active ? 'border-l-4 border-blue-500 bg-blue-500/5 rounded-r' : '';
  const common = {
    'data-block-id': block.id,
    'data-active': active ? 'true' : undefined,
    onClick: onSelect ? () => onSelect(block.id) : undefined,
    className: `cursor-pointer transition-colors ${ring}`,
  };

  switch (block.type) {
    case 'empty':
      return <div {...common} className={`${common.className} h-4`} />;
    case 'skipped':
      // v2.2 T1：解析层跳过块——不渲染（存在于 IR 仅为审计账目）
      return null;
    case 'algorithm':
      return (
        <AlgorithmCard
          docId={docId}
          block={block}
          resources={resources}
          active={active}
          onSelect={onSelect}
        />
      );
    case 'image':
    case 'table':
    case 'formula':
      return <PlaceholderRenderer docId={docId} block={block} resources={resources} />;
    case 'paragraph': {
      const { style, content } = block;
      const align = ALIGN_CLASSES[style?.alignment ?? 'left'] ?? 'text-left';
      const indent = style?.indent_level ? { paddingLeft: `${style.indent_level * 1.5}rem` } : undefined;
      const inline = content.map((item, i) => (
        <InlineContentRenderer key={i} docId={docId} item={item} resources={resources} />
      ));
      const badge = badgeFallback ? (
        <span
          className="mr-1 rounded bg-amber-100 px-1.5 py-0.5 align-middle text-[10px] font-bold text-amber-700"
          data-testid={`fallback-badge-${block.id}`}
        >
          未翻译
        </span>
      ) : null;

      // v2.1 T7.2：译文侧段落外层 group wrapper（hover 工具条锚点）；
      // reading-block 同时挂 wrapper——相邻间距选择器在 wrapper 层生效（PdfCompareView 有额外外层 div）
      const toolbar = interactive ? (
        <ParagraphToolbar
          docId={interactive.docId}
          block={block}
          translatedText={interactive.translatedText(block.id)}
          sourceLang={interactive.sourceLang}
          targetLang={interactive.targetLang}
          onRetranslated={interactive.onRetranslated}
        />
      ) : null;

      // 列表项：前缀 + 内容
      if (style?.is_list_item) {
        return (
          <div className="group relative reading-block">
            {toolbar}
            <div
              {...common}
              style={indent}
              className={`${common.className} ${align} my-1 flex gap-2`}
            >
              <span className="select-none font-bold text-slate-400">•</span>
              <div className="min-w-0 flex-1">{badge}{inline}</div>
            </div>
          </div>
        );
      }

      // v2.2 T3：题注——0.9em 居中灰字，紧贴其图/表（编号可指认，D7）
      if (style?.is_caption) {
        return (
          <div className="group relative reading-block">
            {toolbar}
            <p
              {...common}
              className={`${common.className} reading-block`}
              style={{
                fontSize: '0.9em',
                color: '#475569',
                textAlign: 'center',
                marginTop: '0.4em',
              }}
            >
              {badge}
              {inline}
            </p>
          </div>
        );
      }

      // 标题：按层级渲染语义标签
      const level = style?.heading_level;
      if (level && level >= 1 && level <= 6) {
        const Tag = HEADING_TAGS[level - 1];
        return (
          <div className="group relative reading-block">
            {toolbar}
            <Tag
              {...common}
              style={indent}
              className={`${common.className} reading-block ${HEADING_CLASSES[level]} ${align}`}
            >
              {badge}
              {inline}
            </Tag>
          </div>
        );
      }

      return (
        <div className="group relative reading-block">
          {toolbar}
          <p
            {...common}
            style={indent}
            className={`${common.className} reading-block ${align}`}
          >
            {badge}
            {inline}
          </p>
        </div>
      );
    }
  }
}
