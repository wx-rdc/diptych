import { afterEach, describe, expect, it, vi } from 'vitest';
import { BackendError, cancelTranslate, fetchQuota, originalPdfUrl, parseDoc, resourceUrl, translateDoc } from '../backend';

function sseResponse(frames: string[], init?: ResponseInit): Response {
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const f of frames) controller.enqueue(encoder.encode(f));
      controller.close();
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { 'content-type': 'text/event-stream' },
    ...init,
  });
}

const doc = { id: 'd1', title: 'T', blocks: [], resources: {} };

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('parseDoc', () => {
  it('成功返回 Document', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(doc), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    const result = await parseDoc('/tmp/x.docx');
    expect(result.id).toBe('d1');
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/parse',
      expect.objectContaining({ method: 'POST' }),
    );
  });

  it('200 外壳响应（无 blocks，require_login 匿名态）→ 抛 401 引导登录', async () => {
    // v2.6 修复：外壳曾被视为文档渲染导致 doc.blocks.map 白屏崩溃
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation(async () =>
        new Response(JSON.stringify({ id: 'shell1', title: 'x.pdf' }), { status: 200 })),
    );
    const e = await parseDoc('/tmp/x.pdf').catch((x) => x);
    expect(e).toBeInstanceOf(BackendError);
    expect(e.status).toBe(401);
    expect(e.message).toContain('登录');
  });

  it('HTTP 错误抛 BackendError 并带 error 字段', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation(() =>
        Promise.resolve(new Response(JSON.stringify({ error: '文件不存在' }), { status: 422 })),
      ),
    );
    await expect(parseDoc('/tmp/none.docx')).rejects.toThrow(BackendError);
    await expect(parseDoc('/tmp/none.docx')).rejects.toThrow('文件不存在');
  });

  it('SSE 响应按事件分流：progress 回调 + done 解析为 Document', async () => {
    const pdfDoc = {
      ...doc,
      pdf_meta: { page_count: 2, page_sizes: [[612, 792], [612, 792]] },
      render_meta: {},
    };
    const frames = [
      'event: progress\ndata: {"stage":"splitting","chunks":1}\n\n',
      'event: progress\ndata: {"stage":"parsing","done_files":0,"total_files":1,"pages_done":0,"pages_total":2}\n\n',
      `event: done\ndata: ${JSON.stringify(JSON.stringify(pdfDoc))}\n\n`.replace(
        /^event: done\ndata: "/,
        'event: done\ndata: ',
      ),
    ];
    // done data 需要是 JSON 字符串（SSE data 行为纯文本）
    frames[2] = `event: done\ndata: ${JSON.stringify(pdfDoc)}\n\n`;
    const fetchMock = vi.fn().mockResolvedValue(sseResponse(frames));
    vi.stubGlobal('fetch', fetchMock);

    const seen: string[] = [];
    const result = await parseDoc('/tmp/x.pdf', 'mineru', (p) => seen.push(p.stage ?? ''));
    expect(seen).toEqual(['splitting', 'parsing']);
    expect(result.pdf_meta?.page_count).toBe(2);
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.parser).toBe('mineru');
  });

  it('SSE error 事件抛 BackendError', async () => {
    const frames = [
      'event: progress\ndata: {"stage":"parsing"}\n\n',
      'event: error\ndata: {"code":"MINERU_AUTH","message":"Token 无效"}\n\n',
    ];
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse(frames)));
    await expect(parseDoc('/tmp/x.pdf', 'mineru')).rejects.toThrow('Token 无效');
  });

  it('originalPdfUrl 生成直链', () => {
    expect(originalPdfUrl('d/1')).toBe('/api/original/d%2F1');
  });
});

describe('translateDoc', () => {
  it('progress 回调触发并以 complete 结果 resolve', async () => {
    const frames = [
      'event: progress\ndata: {"block_id":"b1","current":1,"total":2,"status":"done"}\n\n',
      'event: progress\ndata: {"block_id":"b2","current":2,"total":2,"status":"cached"}\n\n',
      'event: complete\ndata: {"doc_id":"d1","pairs":[]}\n\n',
    ];
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse(frames)));

    const seen: string[] = [];
    const result = await translateDoc('d1', 'en', 'zh', (p) => seen.push(`${p.block_id}:${p.status}`));
    expect(seen).toEqual(['b1:done', 'b2:cached']);
    expect(result.doc_id).toBe('d1');
  });

  it('error 事件抛 BackendError', async () => {
    const frames = ['event: error\ndata: {"message":"上游不可用"}\n\n'];
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse(frames)));
    await expect(translateDoc('d1', 'en', 'zh', () => {})).rejects.toThrow('上游不可用');
  });

  it('未收到 complete 抛中断错误', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse(['event: progress\ndata: {"current":1}\n\n'])));
    await expect(translateDoc('d1', 'en', 'zh', () => {})).rejects.toThrow('连接中断');
  });

  it('非 2xx 抛错', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: '文档不存在: d1' }), { status: 404 })),
    );
    await expect(translateDoc('d1', 'en', 'zh', () => {})).rejects.toThrow('文档不存在');
  });
});

describe('resourceUrl', () => {
  it('生成编码后的资源直链', () => {
    expect(resourceUrl('doc 1', 'r01_image')).toBe('/api/resource/doc%201/r01_image');
  });
});

// ────────────────── v2.4：配额查询 / 取消 / token 附带 ──────────────────

describe('fetchQuota（v2.4 T3）', () => {
  it('返回匿名配额状态', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ anonymous: true, unlimited: false, limit: 5, used: 2, remaining: 3 }), { status: 200 }),
      ),
    );
    const q = await fetchQuota();
    expect(q).toEqual({ anonymous: true, unlimited: false, limit: 5, used: 2, remaining: 3 });
  });
});

describe('cancelTranslate（v2.4 T5）', () => {
  it('POST cancel 附带 token query；404 幂等忽略', async () => {
    const { rememberToken } = await import('../docToken');
    rememberToken('docX', 'tok-123');
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 404 }));
    vi.stubGlobal('fetch', fetchMock);
    await expect(cancelTranslate('docX')).resolves.toBeUndefined();
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/translate/docX/cancel?token=tok-123',
      expect.objectContaining({ method: 'POST' }),
    );
  });

  it('非 404 错误抛 BackendError', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: '无权访问' }), { status: 403 })),
    );
    await expect(cancelTranslate('docNoToken')).rejects.toThrow('无权访问');
  });
});

describe('translateDoc SSE 新事件（v2.4 T5）', () => {
  it('limit_reached → QuotaLimitError（结构化字段）', async () => {
    const frames = ['event: limit_reached\ndata: {"limit":5,"used":5,"required":1}\n\n'];
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse(frames)));
    await expect(translateDoc('d1', 'en', 'zh', () => {})).rejects.toMatchObject({
      name: 'QuotaLimitError',
      quota: { limit: 5, used: 5, required: 1 },
    });
  });

  it('cancelled → TranslateCancelledError（translated_pages）', async () => {
    const frames = ['event: cancelled\ndata: {"translated_pages":3}\n\n'];
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse(frames)));
    await expect(translateDoc('d1', 'en', 'zh', () => {})).rejects.toMatchObject({
      name: 'TranslateCancelledError',
      translatedPages: 3,
    });
  });

  it('请求体携带 access_token（docToken 已登记时）', async () => {
    const { rememberToken } = await import('../docToken');
    rememberToken('d-tok', 'tok-abc');
    const frames = ['event: complete\ndata: {"doc_id":"d-tok","pairs":[]}\n\n'];
    const fetchMock = vi.fn().mockResolvedValue(sseResponse(frames));
    vi.stubGlobal('fetch', fetchMock);
    await translateDoc('d-tok', 'en', 'zh', () => {});
    const body = JSON.parse(fetchMock.mock.calls[0][1].body);
    expect(body.access_token).toBe('tok-abc');
  });

  it('402 响应 → BackendError 携带 quota 字段', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: '额度不足', limit: 5, used: 5, required: 10 }), { status: 402 }),
      ),
    );
    await expect(translateDoc('d1', 'en', 'zh', () => {})).rejects.toMatchObject({
      name: 'BackendError',
      status: 402,
      quota: { limit: 5, used: 5, required: 10 },
    });
  });
});
