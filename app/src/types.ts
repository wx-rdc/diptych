// IR 的 TypeScript 镜像 —— 与 crates/geminus-core/src/model.rs 的 serde 契约对齐
// （判别联合：serde tag="type" rename_all="snake_case"）

export interface GDocument {
  id: string;
  title: string;
  blocks: Block[];
  resources: Record<string, Resource>;
  /** v2：PDF 渲染坐标（MinerU 解析填充） */
  render_meta?: Record<string, RenderMeta>;
  /** v2：PDF 页面尺寸（存在时支持原始渲染模式） */
  pdf_meta?: PdfMeta | null;
  /** v2.4：匿名文档访问凭证（上传响应附带；查看/翻译/资源直链防盗链三选一之一） */
  access_token?: string;
}

/** 块级渲染坐标：bbox 原点=左上，PDF 点（与 pdf.js viewport 同坐标系） */
export interface RenderMeta {
  page_idx: number;
  bbox: [number, number, number, number];
}

export interface PdfMeta {
  page_count: number;
  page_sizes: [number, number][];
}

/** MinerU 解析进度（SSE progress 事件） */
export interface ParseProgress {
  stage: 'splitting' | 'uploading' | 'parsing' | 'merging' | 'fallback' | 'cached';
  message?: string;
  chunks?: number;
  current?: number;
  total?: number;
  done_files?: number;
  total_files?: number;
  pages_done?: number;
  pages_total?: number;
}

export type ParserKind = 'auto' | 'mineru' | 'builtin';
export type RenderMode = 'original' | 'text';

export type Block =
  | {
      type: 'paragraph';
      id: string;
      style: ParagraphStyle;
      content: InlineContent[];
    }
  | {
      type: 'image';
      id: string;
      resource_id: string;
      width?: number | null;
      height?: number | null;
      alt_text?: string | null;
    }
  | {
      type: 'table';
      id: string;
      resource_id: string;
      preview_text?: string | null;
    }
  | {
      type: 'formula';
      id: string;
      resource_id: string;
      omml?: string;
    }
  | {
      /** v2.2 T4：算法伪代码块 */
      type: 'algorithm';
      id: string;
      header?: string;
      lines: AlgoLine[];
    }
  | {
      /** v2.2 T1：解析层显式跳过块（不渲染） */
      type: 'skipped';
      id: string;
      source_type?: string;
      reason?: { kind: 'decoration' } | { kind: 'unmapped'; type_name?: string };
    }
  | { type: 'empty'; id: string };

/** v2.2 T4：算法块的一行 */
export interface AlgoLine {
  kind: 'label' | 'stmt' | 'comment';
  segments: InlineContent[];
}

export type InlineContent =
  | {
      type: 'text';
      text: string;
      bold?: boolean;
      italic?: boolean;
      underline?: boolean;
    }
  | { type: 'inline_image'; resource_id: string }
  | { type: 'inline_formula'; resource_id: string }
  | { type: 'line_break' };

export interface ParagraphStyle {
  heading_level?: number | null;
  alignment?: Alignment;
  indent_level?: number;
  is_list_item?: boolean;
  /** v2.2 T3：题注（figure/table caption）样式位 */
  is_caption?: boolean;
}

export type Alignment = 'left' | 'center' | 'right' | 'justify';

export interface Resource {
  kind: ResourceKind;
  mime?: string;
  data_b64?: string;
  omml?: string;
  latex?: string | null;
  raw_xml?: string;
}

export type ResourceKind = 'image' | 'formula' | 'table';

export interface TranslatedBlock {
  block_id: string;
  original: Block;
  /** None = 占位符/空段（复用 original 渲染） */
  translated: Block | null;
  /** true = 翻译失败回退原文（前端打「未翻译」徽标，v2.1 T1） */
  fallback?: boolean;
}

export interface TranslatedDocument {
  doc_id: string;
  pairs: TranslatedBlock[];
}

export type ProgressStatus = 'translating' | 'done' | 'cached' | 'error';

export interface TranslateProgress {
  block_id: string;
  current: number;
  total: number;
  status: ProgressStatus;
  translated_text?: string | null;
  error?: string | null;
}

/** 段落纯文本（进度回填用） */
export function paragraphPlainText(content: InlineContent[]): string {
  return content
    .filter((c): c is Extract<InlineContent, { type: 'text' }> => c.type === 'text')
    .map((c) => c.text)
    .join('');
}
