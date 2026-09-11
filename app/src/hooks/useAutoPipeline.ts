// v2.4 T3：零点击自动化流水线状态机。
// 选择文件 → 本地配额预检 → 自动上传解析 → 自动翻译（缓存命中秒回）。
// 移除「打开 / 开始翻译」按钮后的唯一驱动链；语言/解析器沿用用户当前选择。

import { useCallback, useRef, useState } from 'react';
import { parseDoc, parseDocUpload } from '../services/backend';
import type { GDocument, ParseProgress, ParserKind } from '../types';

export type PipelineStage =
  | 'idle' // 等待选择文件
  | 'prechecking' // 本地配额预检（读 PDF 页数）
  | 'parsing' // 上传 + 解析（SSE 进度）
  | 'parsed' // 解析完成（即将自动翻译）
  | 'translating' // 翻译进行中（调用方接管）
  | 'blocked'; // 预检拦截（不上传）

export interface PipelineState {
  stage: PipelineStage;
  parseProgress: ParseProgress | null;
  /** blocked 时的展示文案 */
  blockMessage: string | null;
}

export const PIPELINE_INITIAL: PipelineState = {
  stage: 'idle',
  parseProgress: null,
  blockMessage: null,
};

export interface UseAutoPipeline {
  state: PipelineState;
  /**
   * 启动流水线（本地路径开发分支）。
   * @returns 解析成功 → GDocument（调用方继续自动翻译）；预检拦截 → null
   */
  runPath: (filePath: string, parser: ParserKind, precheck: (name: string) => Promise<boolean>) => Promise<GDocument | null>;
  /**
   * 启动流水线（上传公网分支）。
   * @returns 同 runPath
   */
  runUpload: (file: File, parser: ParserKind, precheck: (name: string) => Promise<boolean>) => Promise<GDocument | null>;
  /** 中断/出错后回到 idle */
  reset: () => void;
}

export function useAutoPipeline(): UseAutoPipeline {
  const [state, setState] = useState<PipelineState>(PIPELINE_INITIAL);
  // 流水线代际号：组件重置（切换文档）后旧异步结果不再落状态
  const runIdRef = useRef(0);

  const run = useCallback(
    async (
      precheck: (name: string) => Promise<boolean>,
      blockMessage: string,
      parse: () => Promise<GDocument>,
    ): Promise<GDocument | null> => {
      const runId = ++runIdRef.current;
      setState({ stage: 'prechecking', parseProgress: null, blockMessage: null });
      const pass = await precheck('');
      if (runId !== runIdRef.current) return null; // 已被新一轮启动取代
      if (!pass) {
        setState({ stage: 'blocked', parseProgress: null, blockMessage });
        return null;
      }
      setState({ stage: 'parsing', parseProgress: null, blockMessage: null });
      try {
        const doc = await parse();
        if (runId !== runIdRef.current) return null;
        setState({ stage: 'parsed', parseProgress: null, blockMessage: null });
        return doc;
      } catch (e) {
        if (runId !== runIdRef.current) return null;
        setState(PIPELINE_INITIAL);
        throw e;
      }
    },
    [],
  );

  const runPath = useCallback(
    async (
      filePath: string,
      parser: ParserKind,
      precheck: (name: string) => Promise<boolean>,
    ): Promise<GDocument | null> =>
      run(precheck, '文件超出游客剩余额度，登录后不限量使用', () =>
        parseDoc(filePath, parser, (p) =>
          setState((s) => (s.stage === 'parsing' ? { ...s, parseProgress: p } : s)),
        ),
      ),
    [run],
  );

  const runUpload = useCallback(
    async (
      file: File,
      parser: ParserKind,
      precheck: (name: string) => Promise<boolean>,
    ): Promise<GDocument | null> =>
      run(precheck, '文件超出游客剩余额度，登录后不限量使用', () =>
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
