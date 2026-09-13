// 后端服务层：HTTP+SSE 适配（Tauri 回迁点——换成本模块的 invoke/listen 实现即可）

import { parseSseStream } from './sse';
import { rememberToken, tokenOf, tokenQuery } from './docToken';
import type {
  GDocument,
  ParseProgress,
  ParserKind,
  TranslateProgress,
  TranslatedBlock,
  TranslatedDocument,
} from '../types';

const API_BASE = '/api';

/** 402 / limit_reached 结构化配额字段（QuotaExceededCard 消费） */
export interface QuotaInfo {
  limit: number;
  used: number;
  /** 触发拦截时还差多少页（熔断批页数） */
  required?: number;
  /** 触发时已完整译出的页数（部分处理成果） */
  translated_pages?: number;
  /** 最早一页额度恢复时间（unix 秒；24h 滚动窗口） */
  reset_at?: number | null;
  /** 计费主体是否登录用户（文案分支：登录用户不显示登录引导） */
  user?: boolean;
}

export class BackendError extends Error {
  /** HTTP 状态码（402 = 匿名额度不足，401 = 需登录，403 = 无权访问） */
  readonly status: number;
  /** 402 响应附带的配额结构化字段 */
  readonly quota?: QuotaInfo;

  constructor(message: string, status = 0, quota?: QuotaInfo) {
    super(message);
    this.name = 'BackendError';
    this.status = status;
    this.quota = quota;
  }
}

/** v2.5：额度用尽/熔断（解析 402 或翻译循环 limit_reached SSE 事件） */
export class QuotaLimitError extends Error {
  readonly quota: QuotaInfo;

  constructor(quota: QuotaInfo) {
    super(
      quota.user
        ? `24 小时内最多翻译 ${quota.limit} 页（已用 ${quota.used} 页）`
        : `游客 24 小时内最多翻译 ${quota.limit} 页（已用 ${quota.used} 页），登录后每天可翻译 1000 页`,
    );
    this.name = 'QuotaLimitError';
    this.quota = quota;
  }
}

/** v2.4 T5：翻译被取消（用户停止 / 客户端断开止损） */
export class TranslateCancelledError extends Error {
  readonly translatedPages: number;

  constructor(translatedPages: number) {
    super(`翻译已停止（已译 ${translatedPages} 页）`);
    this.name = 'TranslateCancelledError';
    this.translatedPages = translatedPages;
  }
}

/** 从错误响应体构造 BackendError（402 附加结构化配额字段） */
async function backendErrorOf(res: Response, fallback: string): Promise<BackendError> {
  let message: string | null = null;
  let quota: QuotaInfo | undefined;
  try {
    const body = (await res.json()) as Partial<QuotaInfo> & { error?: string };
    message = body.error ?? null;
    if (res.status === 402 && body.limit != null && body.used != null) {
      quota = {
        limit: body.limit,
        used: body.used,
        required: body.required,
        translated_pages: body.translated_pages,
        reset_at: body.reset_at,
        user: body.user,
      };
    }
  } catch {
    /* 非 JSON 体 */
  }
  return new BackendError(message ?? `${fallback}：HTTP ${res.status}`, res.status, quota);
}

/** v2.5：当前访问者配额状态（GET /api/quota）——游客 5 页/24h，登录 1000 页/24h，本地直连不限 */
export interface QuotaStatus {
  anonymous: boolean;
  unlimited: boolean;
  limit?: number;
  used?: number;
  remaining?: number | null;
  /** 最早一页额度恢复时间（unix 秒；24h 滚动窗口；未用为 null） */
  reset_at?: number | null;
}

export async function fetchQuota(): Promise<QuotaStatus> {
  const res = await fetch(`${API_BASE}/quota`);
  if (!res.ok) throw new BackendError(`配额查询失败：HTTP ${res.status}`, res.status);
  return (await res.json()) as QuotaStatus;
}

/** v2.4 T5：取消进行中的翻译任务 */
export async function cancelTranslate(docId: string): Promise<void> {
  const token = tokenOf(docId);
  const q = token ? `?token=${encodeURIComponent(token)}` : '';
  const res = await fetch(`${API_BASE}/translate/${encodeURIComponent(docId)}/cancel${q}`, {
    method: 'POST',
  });
  // 404 = 任务已结束（幂等忽略）；其余非 2xx 抛出
  if (!res.ok && res.status !== 404) {
    throw await backendErrorOf(res, '取消失败');
  }
}

/** 解析响应到达：登记 token（响应顶层附带 access_token） */
function acceptToken(doc: GDocument): GDocument {
  rememberToken(doc.id, doc.access_token);
  return doc;
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
    throw await backendErrorOf(res, '解析失败');
  }
  const contentType = res.headers.get('content-type') ?? '';
  if (contentType.includes('text/event-stream') && res.body) {
    const reader = res.body.getReader();
    for await (const ev of parseSseStream(reader)) {
      if (ev.event === 'progress') {
        onProgress?.(JSON.parse(ev.data) as ParseProgress);
      } else if (ev.event === 'done') {
        return acceptToken(JSON.parse(ev.data) as GDocument);
      } else if (ev.event === 'error') {
        const payload = JSON.parse(ev.data) as { message?: string };
        throw new BackendError(payload.message ?? 'MinerU 解析失败');
      }
    }
    throw new BackendError('连接中断：未收到 done 事件');
  }
  const parsed = (await res.json()) as GDocument;
  // 200 外壳（require_login 匿名态：仅 doc_id 无 IR）——按需登录处理，
  // 防止无 blocks 的外壳流入渲染层（doc.blocks.map 白屏）
  if (!parsed.blocks) {
    throw new BackendError('请登录后查看与翻译文档', 401);
  }
  return acceptToken(parsed);
}

/** 原始 PDF 文件直链（pdf.js 渲染用；匿名文档附 token） */
export function originalPdfUrl(docId: string): string {
  return `${API_BASE}/original/${encodeURIComponent(docId)}${tokenQuery(docId)}`;
}

/** v2.4 T2：历史/回访文档回载（GET /api/documents/:id，附 token） */
export async function fetchDocument(docId: string): Promise<GDocument> {
  const res = await fetch(
    `${API_BASE}/documents/${encodeURIComponent(docId)}${tokenQuery(docId)}`,
  );
  if (!res.ok) {
    throw await backendErrorOf(res, '文档加载失败');
  }
  return acceptToken((await res.json()) as GDocument);
}

/** 图片等资源直链（匿名文档附 token） */
export function resourceUrl(docId: string, resourceId: string): string {
  return `${API_BASE}/resource/${encodeURIComponent(docId)}/${encodeURIComponent(resourceId)}${tokenQuery(docId)}`;
}

/**
 * POST /api/translate → SSE 流
 * 逐个进度回调 onProgress；complete 事件返回 TranslatedDocument；
 * limit_reached → QuotaLimitError（已译部分经 onProgress 已回填，右栏保留）；
 * cancelled → TranslateCancelledError；error 事件抛 BackendError。
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
    body: JSON.stringify({
      doc_id: docId,
      source_lang: sourceLang,
      target_lang: targetLang,
      access_token: tokenOf(docId) ?? null,
    }),
  });
  if (!res.ok || !res.body) {
    throw await backendErrorOf(res, '翻译失败');
  }

  const reader = res.body.getReader();
  for await (const ev of parseSseStream(reader)) {
    if (ev.event === 'progress') {
      onProgress(JSON.parse(ev.data) as TranslateProgress);
    } else if (ev.event === 'complete') {
      return JSON.parse(ev.data) as TranslatedDocument;
    } else if (ev.event === 'limit_reached') {
      throw new QuotaLimitError(JSON.parse(ev.data) as QuotaInfo);
    } else if (ev.event === 'cancelled') {
      const v = JSON.parse(ev.data) as { translated_pages?: number };
      throw new TranslateCancelledError(v.translated_pages ?? 0);
    } else if (ev.event === 'error') {
      const payload = JSON.parse(ev.data) as { message?: string };
      throw new BackendError(payload.message ?? '翻译失败');
    }
  }
  throw new BackendError('连接中断：未收到 complete 事件');
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
      access_token: tokenOf(docId) ?? null,
    }),
  });
  if (!res.ok) {
    throw await backendErrorOf(res, '重译失败');
  }
  return (await res.json()) as TranslatedBlock;
}
