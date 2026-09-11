// 认证服务（部署安全加固）：预设用户登录 / 会话查询 / 登出。
// 会话由服务端 HttpOnly Cookie 承载，同源 fetch 默认携带。

import { BackendError } from './backend';

const API_BASE = '/api';

export interface Me {
  authenticated: boolean;
  username: string | null;
  /** 服务器是否配置预设用户（false = 登录不可用，本地开发态） */
  login_enabled?: boolean;
  /** v2.4：查看/翻译是否强制登录（GEMINUS_REQUIRE_LOGIN）；缺省视为 false */
  require_login?: boolean;
}

/** GET /api/auth/me → 当前会话 */
export async function fetchMe(): Promise<Me> {
  const res = await fetch(`${API_BASE}/auth/me`);
  if (!res.ok) throw new BackendError(`会话查询失败：HTTP ${res.status}`, res.status);
  return (await res.json()) as Me;
}

/** POST /api/auth/login → 登录成功后的会话信息 */
export async function login(username: string, password: string): Promise<Me> {
  const res = await fetch(`${API_BASE}/auth/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) {
    const msg = await errorOf(res);
    throw new BackendError(msg ?? `登录失败：HTTP ${res.status}`, res.status);
  }
  return { authenticated: true, username };
}

/** POST /api/auth/logout */
export async function logout(): Promise<void> {
  await fetch(`${API_BASE}/auth/logout`, { method: 'POST' });
}

async function errorOf(res: Response): Promise<string | null> {
  try {
    const body = (await res.json()) as { error?: string };
    return body.error ?? null;
  } catch {
    return null;
  }
}
