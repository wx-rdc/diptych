import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useAutoPipeline, PIPELINE_INITIAL } from '../useAutoPipeline';
import * as backend from '../../services/backend';
import type { GDocument, ParseProgress } from '../../types';

vi.mock('../../services/backend', () => ({
  parseDoc: vi.fn(),
  parseDocUpload: vi.fn(),
}));

const doc = (id = 'd1'): GDocument => ({ id, title: 'T', blocks: [], resources: {} });
const progress = (stage: ParseProgress['stage']): ParseProgress => ({ stage });

afterEach(() => {
  vi.clearAllMocks();
});

describe('useAutoPipeline 状态机流转（v2.5 无本地预检）', () => {
  it('直接解析：parsing（progress 落状态）→ parsed 并返回文档', async () => {
    vi.mocked(backend.parseDoc).mockImplementation(async (_p, _parser, onProgress) => {
      onProgress?.(progress('parsing'));
      return doc();
    });
    const { result } = renderHook(() => useAutoPipeline());
    let out: GDocument | null = null;
    await act(async () => {
      out = await result.current.runPath('/tmp/x.pdf', 'auto');
    });
    expect((out as GDocument | null)?.id).toBe('d1');
    expect(result.current.state.stage).toBe('parsed');
  });

  it('解析失败：抛出原始错误并回到 idle', async () => {
    vi.mocked(backend.parseDoc).mockRejectedValue(new Error('上传失败'));
    const { result } = renderHook(() => useAutoPipeline());
    await expect(
      act(async () => {
        await result.current.runPath('/tmp/x.pdf', 'auto');
      }),
    ).rejects.toThrow('上传失败');
    expect(result.current.state).toEqual(PIPELINE_INITIAL);
  });

  it('上传分支 runUpload 同语义（multipart 调用）', async () => {
    vi.mocked(backend.parseDocUpload).mockResolvedValue(doc('d2'));
    const { result } = renderHook(() => useAutoPipeline());
    let out: GDocument | null = null;
    await act(async () => {
      out = await result.current.runUpload(new File(['x'], 'a.pdf'), 'auto');
    });
    expect((out as GDocument | null)?.id).toBe('d2');
    expect(result.current.state.stage).toBe('parsed');
    expect(backend.parseDocUpload).toHaveBeenCalledOnce();
  });

  it('reset 回到 idle 且旧异步结果不再落状态', async () => {
    let release: (() => void) | null = null;
    vi.mocked(backend.parseDoc).mockImplementation(
      () =>
        new Promise<GDocument>((resolve) => {
          release = () => resolve(doc());
        }),
    );
    const { result } = renderHook(() => useAutoPipeline());
    const first = act(async () => {
      const p = result.current.runPath('/tmp/slow.pdf', 'auto');
      // 启动后立刻 reset（模拟用户切换文档）
      act(() => { result.current.reset(); });
      release?.();
      return p;
    });
    const out = await first;
    expect(out).toBeNull(); // 代际号失效：迟到的解析结果被丢弃
    expect(result.current.state).toEqual(PIPELINE_INITIAL);
  });
});
