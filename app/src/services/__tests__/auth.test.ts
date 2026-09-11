import { afterEach, describe, expect, it, vi } from 'vitest';
import { fetchMe, login, logout } from '../auth';
import { BackendError } from '../backend';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('认证服务（部署安全加固）', () => {
  it('fetchMe 返回已登录会话', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ authenticated: true, username: 'alice' }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    const me = await fetchMe();
    expect(me).toEqual({ authenticated: true, username: 'alice' });
    expect(fetchMock).toHaveBeenCalledWith('/api/auth/me');
  });

  it('fetchMe 匿名会话', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ authenticated: false, username: null }), { status: 200 }),
      ),
    );
    const me = await fetchMe();
    expect(me.authenticated).toBe(false);
  });

  it('login 成功返回会话（POST + JSON body）', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    const me = await login('alice', 'pw');
    expect(me).toEqual({ authenticated: true, username: 'alice' });
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/auth/login');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ username: 'alice', password: 'pw' });
  });

  it('login 401 → BackendError 带状态码与服务端文案', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: '用户名或密码错误' }), { status: 401 }),
      ),
    );
    const err = await login('alice', 'bad').catch((e) => e);
    expect(err).toBeInstanceOf(BackendError);
    expect((err as BackendError).status).toBe(401);
    expect((err as BackendError).message).toBe('用户名或密码错误');
  });

  it('login 429（限速）带状态码', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: '尝试过于频繁，请 10 分钟后再试' }), { status: 429 }),
      ),
    );
    const err = await login('alice', 'pw').catch((e) => e);
    expect((err as BackendError).status).toBe(429);
  });

  it('logout 调用 POST /api/auth/logout', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    await logout();
    expect(fetchMock).toHaveBeenCalledWith('/api/auth/logout', { method: 'POST' });
  });
});
