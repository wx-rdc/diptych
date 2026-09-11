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
  /** 拦截时的展示文案 */
  message: string | null;
}

export const QUOTA_GUARD_PASS: QuotaGuardResult = {
  blocked: false,
  pages: null,
  remaining: null,
  message: null,
};

/** pdf.js 读 PDF 页数（纯本地；失败返回 null 交后端复核） */
export async function localPdfPages(file: File): Promise<number | null> {
  try {
    const { getDocument } = await import('pdfjs-dist');
    const data = new Uint8Array(await file.arrayBuffer());
    const task = getDocument({ data });
    const pdf = await task.promise;
    const pages = pdf.numPages;
    await pdf.destroy();
    return pages;
  } catch {
    return null;
  }
}

export interface UseQuotaGuard {
  /** 最近一次预检结果 */
  result: QuotaGuardResult;
  /** 预检：拦截返回 false（调用方阻断上传），放行返回 true */
  check: (file: File) => Promise<boolean>;
  /** 重置 */
  reset: () => void;
}

/** 页数读取器（默认 pdf.js 本地实现；测试可注入） */
export type PagesReader = (file: File) => Promise<number | null>;

export function useQuotaGuard(readPages: PagesReader = localPdfPages): UseQuotaGuard {
  const [result, setResult] = useState<QuotaGuardResult>(QUOTA_GUARD_PASS);

  const check = useCallback(
    async (file: File): Promise<boolean> => {
      let quota: QuotaStatus;
      try {
        quota = await fetchQuota();
      } catch {
        // 配额端点不可用（旧后端/离线）→ 放行交后端兜底
        setResult(QUOTA_GUARD_PASS);
        return true;
      }
      if (quota.unlimited || quota.remaining == null) {
        setResult(QUOTA_GUARD_PASS);
        return true;
      }
      const remaining = quota.remaining;
      if (file.name.toLowerCase().endsWith('.pdf')) {
        const pages = await readPages(file);
        if (pages != null && pages > remaining) {
          setResult({
            blocked: true,
            pages,
            remaining,
            message: `该文件 ${pages} 页，超出游客剩余 ${remaining} 页额度。登录后不限量使用。`,
          });
          return false;
        }
      }
      // DOCX / 页数未知：放行（后端预检 402 兜底）
      setResult(QUOTA_GUARD_PASS);
      return true;
    },
    [readPages],
  );

  const reset = useCallback(() => setResult(QUOTA_GUARD_PASS), []);

  return { result, check, reset };
}
