// v2.4 T2：用户文档历史服务
import { BackendError } from './backend';

export interface HistoryItem {
  doc_id: string;
  file_name: string;
  file_type: string;
  title: string;
  pages: number;
  uploaded_at: number;
  status: string;
}

/** GET /api/history → 当前用户历史（时间倒序）；401 → BackendError 引导登录 */
export async function fetchHistory(): Promise<HistoryItem[]> {
  const res = await fetch('/api/history');
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as { error?: string } | null;
    throw new BackendError(body?.error ?? `历史记录获取失败：HTTP ${res.status}`, res.status);
  }
  const data = (await res.json()) as { items: HistoryItem[] };
  return data.items;
}
