// v2.4.1：游客会话内翻译记录（回答「已使用的 N 页在哪里」）。
// - 只存 doc_id + file_name + 时间（译文不落盘——回访时 fetchDocument + 翻译，
//   已译段落命中服务端缓存秒回，零配额消耗）；
// - sessionStorage 持久（标签页内刷新不丢）；登录用户的「🕘 历史」走服务端，不受此影响。

const STORAGE_KEY = 'geminus.guestDocs';
const MAX_ITEMS = 50;

export interface GuestDocRecord {
  doc_id: string;
  file_name: string;
  /** 最近一次翻译时间（epoch ms） */
  at: number;
}

function load(): GuestDocRecord[] {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    const list = raw ? (JSON.parse(raw) as GuestDocRecord[]) : [];
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

function save(list: GuestDocRecord[]): void {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(list.slice(0, MAX_ITEMS)));
  } catch {
    /* 隐私模式写失败静默 */
  }
}

/** 登记一次翻译（幂等：同 doc_id 刷新时间并置顶） */
export function recordGuestDoc(docId: string, fileName: string): void {
  const list = load().filter((r) => r.doc_id !== docId);
  list.unshift({ doc_id: docId, file_name: fileName, at: Date.now() });
  save(list);
}

/** 会话内游客翻译记录（新→旧） */
export function listGuestDocs(): GuestDocRecord[] {
  return load();
}

/** 服务端额度消耗明细（GET /api/quota/usage）——跨会话/同 IP 维度，与本地记录合并 */
export async function fetchGuestUsage(): Promise<GuestDocRecord[]> {
  const res = await fetch('/api/quota/usage');
  if (!res.ok) return [];
  const body = (await res.json()) as {
    items?: { doc_id: string; file_name: string; uploaded_at: number }[];
  };
  return (body.items ?? []).map((i) => ({
    doc_id: i.doc_id,
    file_name: i.file_name || '(未命名文档)',
    at: i.uploaded_at * 1000,
  }));
}

/** 本地记录 ∪ 服务端明细（按 doc_id 去重，取较新时间），新→旧 */
export function mergeGuestDocs(local: GuestDocRecord[], remote: GuestDocRecord[]): GuestDocRecord[] {
  const byId = new Map<string, GuestDocRecord>();
  for (const r of [...remote, ...local]) {
    const prev = byId.get(r.doc_id);
    if (!prev || r.at > prev.at) byId.set(r.doc_id, r);
  }
  return [...byId.values()].sort((a, b) => b.at - a.at);
}
