import { describe, expect, it } from 'vitest';
import { parseSseStream } from '../sse';

function streamOf(chunks: string[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  let i = 0;
  return new ReadableStream<Uint8Array>({
    pull(controller) {
      if (i < chunks.length) {
        controller.enqueue(encoder.encode(chunks[i++]));
      } else {
        controller.close();
      }
    },
  });
}

async function collect(chunks: string[]) {
  const events = [];
  for await (const ev of parseSseStream(streamOf(chunks).getReader())) {
    events.push(ev);
  }
  return events;
}

describe('parseSseStream', () => {
  it('解析单帧 event+data', async () => {
    const events = await collect(['event: progress\ndata: {"a":1}\n\n']);
    expect(events).toEqual([{ event: 'progress', data: '{"a":1}' }]);
  });

  it('跨 chunk 的帧正确拼接', async () => {
    const events = await collect(['event: prog', 'ress\ndata: {"cur', 'rent":3}\n\nevent: complete\ndata: {"ok"', ':true}\n\n']);
    expect(events).toEqual([
      { event: 'progress', data: '{"current":3}' },
      { event: 'complete', data: '{"ok":true}' },
    ]);
  });

  it('多行 data 以换行拼接', async () => {
    const events = await collect(['event: x\ndata: line1\ndata: line2\n\n']);
    expect(events[0].data).toBe('line1\nline2');
  });

  it('CRLF 分隔兼容', async () => {
    const events = await collect(['event: p\r\ndata: 1\r\n\r\nevent: c\r\ndata: 2\r\n\r\n']);
    expect(events).toEqual([
      { event: 'p', data: '1' },
      { event: 'c', data: '2' },
    ]);
  });

  it('无 event 字段默认 message；注释行忽略；无 data 帧跳过', async () => {
    const events = await collect([': keepalive\ndata: hi\n\n\n\n: ping\n\n']);
    expect(events).toEqual([{ event: 'message', data: 'hi' }]);
  });

  it('流结束时残留无空行帧仍产出', async () => {
    const events = await collect(['event: complete\ndata: {"z":9}\n\nevent: error\ndata: boom']);
    expect(events).toEqual([
      { event: 'complete', data: '{"z":9}' },
      { event: 'error', data: 'boom' },
    ]);
  });
});
