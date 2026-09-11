// 后端服务层：HTTP+SSE 适配（Tauri 回迁点——换成本模块的 invoke/listen 实现即可）

import { parseSseStream } from './sse';
import type {
  GDocument,
  ParseProgress,
  ParserKind,
  TranslateProgress,
  TranslatedBlock,
  TranslatedDocument,
} from '../types';

const API_BASE = '/api';

export class BackendError extends Error {
  /** HTTP 状态码（429 = 未登录额度用尽，401 = 需登录） */
  readonly status: number;

  constructor(message: string, status = 0) {
    super(message);
    this.name = 'BackendError';
    this.status = status;
  }
}

/**
 * POST /api/parse（multipart 上传）→ Document
 * 公网用户入口：file 字段 + 可选 parser 字段；SSE 语义与 parseDoc 相同。
 */
export async function parseDocUpload(
  file: File,
  parser: ParserKind = 'auto',
  onProgress?: (p: ParseProgress) => void,
): Promise<GDocument> {
  const form = new FormData();
  form.append('file', file);
  form.append('parser', parser);
  const res = await fetch(`${API_BASE}/parse`, { method: 'POST', body: form });
  return consumeParseResponse(res, onProgress);
}

/**
 * POST /api/parse → Document
 * - builtin/DOCX：同步 JSON 响应
 * - mineru/auto（PDF）：SSE 流（progress* → done | error），逐进度回调 onProgress
 */
export async function parseDoc(
  filePath: string,
  parser: ParserKind = 'auto',
  onProgress?: (p: ParseProgress) => void,
): Promise<GDocument> {
  const res = await fetch(`${API_BASE}/parse`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ file_path: filePath, parser }),
  });
  return consumeParseResponse(res, onProgress);
}

/** /api/parse 响应消费：SSE 流或同步 JSON */
async function consumeParseResponse(
  res: Response,
  onProgress?: (p: ParseProgress) => void,
): Promise<GDocument> {
  if (!res.ok) {
    throw new BackendError(
      (await errorOf(res)) ?? `解析失败：HTTP ${res.status}`,
      res.status,
    );
  }
  const contentType = res.headers.get('content-type') ?? '';
  if (contentType.includes('text/event-stream') && res.body) {
    const reader = res.body.getReader();
    for await (const ev of parseSseStream(reader)) {
      if (ev.event === 'progress') {
        onProgress?.(JSON.parse(ev.data) as ParseProgress);
      } else if (ev.event === 'done') {
        return JSON.parse(ev.data) as GDocument;
      } else if (ev.event === 'error') {
        const payload = JSON.parse(ev.data) as { message?: string };
        throw new BackendError(payload.message ?? 'MinerU 解析失败');
      }
    }
    throw new BackendError('连接中断：未收到 done 事件');
  }
  return (await res.json()) as GDocument;
}

/** 原始 PDF 文件直链（pdf.js 渲染用） */
export function originalPdfUrl(docId: string): string {
  return `${API_BASE}/original/${encodeURIComponent(docId)}`;
}

/** v2.4 T2：历史记录回载（GET /api/documents/:id，401 → 引导登录） */
export async function fetchDocument(docId: string): Promise<GDocument> {
  const res = await fetch(`${API_BASE}/documents/${encodeURIComponent(docId)}`);
  if (!res.ok) {
    throw new BackendError(
      (await errorOf(res)) ?? `文档加载失败：HTTP ${res.status}`,
      res.status,
    );
  }
  return (await res.json()) as GDocument;
}

/** 图片等资源直链 */
export function resourceUrl(docId: string, resourceId: string): string {
  return `${API_BASE}/resource/${encodeURIComponent(docId)}/${encodeURIComponent(resourceId)}`;
}

/**
 * POST /api/translate → SSE 流
 * 逐个进度回调 onProgress，收到 complete 事件解析为 TranslatedDocument 返回；
 * error 事件抛 BackendError。
 */
export async function translateDoc(
  docId: string,
  sourceLang: string,
  targetLang: string,
  onProgress: (p: TranslateProgress) => void,
): Promise<TranslatedDocument> {
  const res = await fetch(`${API_BASE}/translate`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ doc_id: docId, source_lang: sourceLang, target_lang: targetLang }),
  });
  if (!res.ok || !res.body) {
    throw new BackendError(
      (await errorOf(res)) ?? `翻译失败：HTTP ${res.status}`,
      res.status,
    );
  }

  const reader = res.body.getReader();
  for await (const ev of parseSseStream(reader)) {
    if (ev.event === 'progress') {
      onProgress(JSON.parse(ev.data) as TranslateProgress);
    } else if (ev.event === 'complete') {
      return JSON.parse(ev.data) as TranslatedDocument;
    } else if (ev.event === 'error') {
      const payload = JSON.parse(ev.data) as { message?: string };
      throw new BackendError(payload.message ?? '翻译失败');
    }
  }
  throw new BackendError('连接中断：未收到 complete 事件');
}

async function errorOf(res: Response): Promise<string | null> {
  try {
    const body = (await res.json()) as { error?: string };
    return body.error ?? null;
  } catch {
    return null;
  }
}

/** 单段重译（v2.1 T7.2）：绕过读缓存，返回重建后的块 */
export async function retranslateParagraph(
  docId: string,
  blockId: string,
  sourceLang: string,
  targetLang: string,
): Promise<TranslatedBlock> {
  const res = await fetch(`${API_BASE}/translate/paragraph`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      doc_id: docId,
      block_id: blockId,
      source_lang: sourceLang,
      target_lang: targetLang,
    }),
  });
  if (!res.ok) {
    throw new BackendError(
      (await errorOf(res)) ?? `重译失败：HTTP ${res.status}`,
      res.status,
    );
  }
  return (await res.json()) as TranslatedBlock;
}
