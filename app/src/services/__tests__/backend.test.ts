import { afterEach, describe, expect, it, vi } from 'vitest';
import { BackendError, originalPdfUrl, parseDoc, resourceUrl, translateDoc } from '../backend';

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
