// v2.4 T1：匿名文档访问凭证的客户端留存。
// - 内存 Map + sessionStorage 双写（刷新页面后匿名用户仍可凭 token 回访，24h 服务端 TTL 内）；
// - 上传/解析响应携带 access_token 时自动登记。

const STORAGE_KEY = 'geminus.docTokens';

const memory = new Map<string, string>();

function loadPersisted(): void {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    for (const [docId, token] of Object.entries(JSON.parse(raw) as Record<string, string>)) {
      memory.set(docId, token);
    }
  } catch {
    /* 损坏数据忽略 */
  }
}

function persist(): void {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(memory)));
  } catch {
    /* 隐私模式等写失败静默 */
  }
}

loadPersisted();

/** 登记 token（上传响应到达时调用；空值忽略） */
export function rememberToken(docId: string, token: unknown): void {
  if (typeof token === 'string' && token) {
    memory.set(docId, token);
    persist();
  }
}

/** 取该文档的 token（无则 undefined） */
export function tokenOf(docId: string): string | undefined {
  return memory.get(docId);
}

/** 便捷拼接 query（无 token 返回空串，有则 `?token=…` / `&token=…`） */
export function tokenQuery(docId: string, separator = '?'): string {
  const t = tokenOf(docId);
  return t ? `${separator}token=${encodeURIComponent(t)}` : '';
}
