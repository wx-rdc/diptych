import { afterEach, describe, expect, it, vi } from 'vitest';
import { fetchHistory } from '../history';

afterEach(() => vi.restoreAllMocks());

describe('fetchHistory', () => {
  it('GET /api/history → items 数组', async () => {
    const items = [
      { doc_id: 'd1', file_name: 'a.pdf', file_type: 'pdf', title: 'A', pages: 3, uploaded_at: 1, status: 'parsed' },
    ];
    const mock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(new Response(JSON.stringify({ items }), { status: 200 }));
    await expect(fetchHistory()).resolves.toEqual(items);
    expect(mock).toHaveBeenCalledWith('/api/history');
  });

  it('401 → BackendError(status=401)', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ error: '请登录后查看历史记录' }), { status: 401 }),
    );
    await expect(fetchHistory()).rejects.toMatchObject({ status: 401 });
  });
});
