// v2.5 零点击自动化流水线状态机。
// 选择文件 → 自动上传解析 → 自动翻译（缓存命中秒回）。
// v2.5 起无本地配额预检：超页文件由服务端部分处理（只译额度内页数），
// 移除「打开 / 开始翻译」按钮后的唯一驱动链；语言/解析器沿用用户当前选择。

import { useCallback, useRef, useState } from 'react';
import { parseDoc, parseDocUpload } from '../services/backend';
import type { GDocument, ParseProgress, ParserKind } from '../types';

export type PipelineStage =
  | 'idle' // 等待选择文件
  | 'parsing' // 上传 + 解析（SSE 进度）
  | 'parsed' // 解析完成（即将自动翻译）
  | 'translating'; // 翻译进行中（调用方接管）

export interface PipelineState {
  stage: PipelineStage;
  parseProgress: ParseProgress | null;
}

export const PIPELINE_INITIAL: PipelineState = {
  stage: 'idle',
  parseProgress: null,
};

export interface UseAutoPipeline {
  state: PipelineState;
  /**
   * 启动流水线（本地路径开发分支）。
   * @returns 解析成功 → GDocument（调用方继续自动翻译）
   */
  runPath: (filePath: string, parser: ParserKind) => Promise<GDocument | null>;
  /**
   * 启动流水线（上传公网分支）。
   * @returns 同 runPath
   */
  runUpload: (file: File, parser: ParserKind) => Promise<GDocument | null>;
  /** 中断/出错后回到 idle */
  reset: () => void;
}

export function useAutoPipeline(): UseAutoPipeline {
  const [state, setState] = useState<PipelineState>(PIPELINE_INITIAL);
  // 流水线代际号：组件重置（切换文档）后旧异步结果不再落状态
  const runIdRef = useRef(0);

  const run = useCallback(async (parse: () => Promise<GDocument>): Promise<GDocument | null> => {
    const runId = ++runIdRef.current;
    setState({ stage: 'parsing', parseProgress: null });
    try {
      const doc = await parse();
      if (runId !== runIdRef.current) return null;
      setState({ stage: 'parsed', parseProgress: null });
      return doc;
    } catch (e) {
      if (runId !== runIdRef.current) return null;
      setState(PIPELINE_INITIAL);
      throw e;
    }
  }, []);

  const runPath = useCallback(
    async (filePath: string, parser: ParserKind): Promise<GDocument | null> =>
      run(() =>
        parseDoc(filePath, parser, (p) =>
          setState((s) => (s.stage === 'parsing' ? { ...s, parseProgress: p } : s)),
        ),
      ),
    [run],
  );

  const runUpload = useCallback(
    async (file: File, parser: ParserKind): Promise<GDocument | null> =>
      run(() =>
        parseDocUpload(file, parser, (p) =>
          setState((s) => (s.stage === 'parsing' ? { ...s, parseProgress: p } : s)),
        ),
      ),
    [run],
  );

  const reset = useCallback(() => {
    runIdRef.current++;
    setState(PIPELINE_INITIAL);
  }, []);

  return { state, runPath, runUpload, reset };
}
