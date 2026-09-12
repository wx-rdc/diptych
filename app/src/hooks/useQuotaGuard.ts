// v2.4 T3：前端本地配额预检（选择文件后、上传前）。
// - PDF：pdf.js 纯本地读页数（零网络）；
// - DOCX：跳过本地预检（页数估算交后端，见 lib.rs anon_precheck_pages）；
// - 剩余额度来源 GET /api/quota（登录 / 本地开发 unlimited → 恒放行）。
// 边界：pages == remaining 放行；pages > remaining 拦截。

import { useCallback, useState } from 'react';
import { fetchQuota, type QuotaStatus } from '../services/backend';

export interface QuotaGuardResult {
  /** 本次预检是否拦截 */
  blocked: boolean;
  /** 文件页数（PDF 本地读取；未知为 null） */
  pages: number | null;
  /** 剩余额度（unlimited 时为 null） */
  remaining: number | null;
  /** 总限额（unlimited 时为 null）——拦截卡片展示用 */
  limit: number | null;
  /** 已用额度（unlimited 时为 null） */
  used: number | null;
  /** 拦截时的展示文案 */
  message: string | null;
}

export const QUOTA_GUARD_PASS: QuotaGuardResult = {
  blocked: false,
  pages: null,
  remaining: null,
  limit: null,
  used: null,
  message: null,
};

/**
 * pdf.js 读 PDF 页数（纯本地；失败返回 null 交后端复核）。
 * 必须经 loadPdfjs 拿模块——直接 import('pdfjs-dist') 会用默认 workerSrc，
 * 生产环境加载失败 → 页数未知 → 本地预检静默放行（v2.4.1 缺陷回归点）。
 */
export async function localPdfPages(file: File): Promise<number | null> {
  try {
    const { loadPdfjs } = await import('./usePdfRenderer');
    const pdfjs = await loadPdfjs();
    const data = await fileBytes(file);
    const task = pdfjs.getDocument({ data });
    const pdf = await task.promise;
    const pages = pdf.numPages;
    await pdf.destroy();
    return pages;
  } catch {
    return null;
  }
}

/** File → bytes（兼容缺 arrayBuffer 的环境，如 jsdom/旧内核） */
async function fileBytes(file: File): Promise<Uint8Array> {
  if (typeof file.arrayBuffer === 'function') {
    return new Uint8Array(await file.arrayBuffer());
  }
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error ?? new Error('文件读取失败'));
    reader.readAsArrayBuffer(file);
  });
}

export interface UseQuotaGuard {
  /** 最近一次预检结果（渲染态；异步链路请用 check 的返回值） */
  result: QuotaGuardResult;
  /** 预检：返回本次结果（.blocked=true = 拦截，调用方阻断上传） */
  check: (file: File) => Promise<QuotaGuardResult>;
  /** 重置 */
  reset: () => void;
}

/** 页数读取器（默认 pdf.js 本地实现；测试可注入） */
export type PagesReader = (file: File) => Promise<number | null>;

export function useQuotaGuard(readPages: PagesReader = localPdfPages): UseQuotaGuard {
  const [result, setResult] = useState<QuotaGuardResult>(QUOTA_GUARD_PASS);

  const check = useCallback(
    async (file: File): Promise<QuotaGuardResult> => {
      let quota: QuotaStatus;
      try {
        quota = await fetchQuota();
      } catch {
        // 配额端点不可用（旧后端/离线）→ 放行交后端兜底
        setResult(QUOTA_GUARD_PASS);
        return QUOTA_GUARD_PASS;
      }
      if (quota.unlimited || quota.remaining == null) {
        setResult(QUOTA_GUARD_PASS);
        return QUOTA_GUARD_PASS;
      }
      const remaining = quota.remaining;
      if (file.name.toLowerCase().endsWith('.pdf')) {
        const pages = await readPages(file);
        if (pages != null && pages > remaining) {
          const blocked: QuotaGuardResult = {
            blocked: true,
            pages,
            remaining,
            limit: quota.limit ?? null,
            used: quota.used ?? null,
            message: `该文件 ${pages} 页，超出游客剩余 ${remaining} 页额度。登录后不限量使用。`,
          };
          setResult(blocked);
          return blocked;
        }
      }
      // DOCX / 页数未知：放行（后端预检 402 兜底）
      setResult(QUOTA_GUARD_PASS);
      return QUOTA_GUARD_PASS;
    },
    [readPages],
  );

  const reset = useCallback(() => setResult(QUOTA_GUARD_PASS), []);

  return { result, check, reset };
}
