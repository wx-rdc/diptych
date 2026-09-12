// pdf.js 渲染管理（v2 T8.2）：文档加载 / 页尺寸 / canvas 渲染。
// pdfjs 动态导入隔离——jsdom 测试整体 vi.mock('pdfjs-dist')。

import { useEffect, useRef, useState } from 'react';
import type { PDFDocumentProxy } from 'pdfjs-dist';
// v2.3 P0：worker 构建期 inline 打包（blob）——运行时不外部 fetch
// assets/pdf.worker.min-<hash>.mjs，公网 MIME/404/哈希错配根治。
// 必须顶层静态导入：嵌套动态 import 不会触发 Vite worker 插件的 inline 转换
import PdfjsWorker from 'pdfjs-dist/build/pdf.worker.min.mjs?worker&inline';

export interface PageInfo {
  pageNo: number; // 1-based
  width: number; // PDF 点
  height: number;
}

export interface PdfRenderer {
  pages: PageInfo[];
  ready: boolean;
  error: string | null;
  /** 渲染一页到 canvas（scale = 目标像素宽 / 页宽） */
  renderPage: (pageNo: number, canvas: HTMLCanvasElement, targetWidth: number) => Promise<void>;
  /** v2.2 T2：内容裁切渲染——离屏全页渲染 → drawImage 裁切内容矩形到可见 canvas */
  renderPageCrop: (
    pageNo: number,
    canvas: HTMLCanvasElement,
    targetWidth: number,
    crop: { x: number; y: number; w: number; h: number },
  ) => Promise<void>;
}

type PdfjsModule = typeof import('pdfjs-dist');

let pdfjsPromise: Promise<PdfjsModule> | null = null;

/** pdf.js 模块单例加载（含 inline worker 配置）——预检/渲染共用，避免无 worker 环境静默失败 */
export async function loadPdfjs(): Promise<PdfjsModule> {
  if (!pdfjsPromise) {
    pdfjsPromise = import('pdfjs-dist').then((mod) => {
      mod.GlobalWorkerOptions.workerPort = new PdfjsWorker();
      return mod;
    });
  }
  return pdfjsPromise;
}

/** 从 URL 加载 PDF 并提供页信息与渲染（卸载时自动销毁文档） */
export function usePdfRenderer(url: string | null): PdfRenderer {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [pages, setPages] = useState<PageInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const docRef = useRef<PDFDocumentProxy | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDoc(null);
    setPages([]);
    setError(null);
    if (!url) return;
    (async () => {
      try {
        const pdfjs = await loadPdfjs();
        const task = pdfjs.getDocument({ url });
        const loaded = await task.promise;
        if (cancelled) {
          void loaded.destroy();
          return;
        }
        const infos: PageInfo[] = [];
        for (let i = 1; i <= loaded.numPages; i++) {
          const page = await loaded.getPage(i);
          const vp = page.getViewport({ scale: 1 });
          infos.push({ pageNo: i, width: vp.width, height: vp.height });
        }
        if (cancelled) {
          void loaded.destroy();
          return;
        }
        docRef.current = loaded;
        setDoc(loaded);
        setPages(infos);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
      docRef.current?.destroy();
      docRef.current = null;
    };
  }, [url]);

  const renderPage = async (pageNo: number, canvas: HTMLCanvasElement, targetWidth: number) => {
    if (!doc) throw new Error('PDF 未加载');
    const page = await doc.getPage(pageNo);
    const base = page.getViewport({ scale: 1 });
    const scale = targetWidth / base.width;
    const viewport = page.getViewport({ scale });
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('canvas 2d 上下文不可用');
    canvas.width = Math.floor(viewport.width);
    canvas.height = Math.floor(viewport.height);
    await page.render({ canvasContext: ctx, viewport }).promise;
  };

  const renderPageCrop = async (
    pageNo: number,
    canvas: HTMLCanvasElement,
    targetWidth: number,
    crop: { x: number; y: number; w: number; h: number },
  ) => {
    if (!doc) throw new Error('PDF 未加载');
    const page = await doc.getPage(pageNo);
    const base = page.getViewport({ scale: 1 });
    const scale = targetWidth / crop.w;
    // 离屏全页渲染（pdf.js 只能按 viewport 整页渲染）
    const off = document.createElement('canvas');
    off.width = Math.floor(base.width * scale);
    off.height = Math.floor(base.height * scale);
    const offCtx = off.getContext('2d');
    if (!offCtx) throw new Error('离屏 canvas 2d 上下文不可用');
    const viewport = page.getViewport({ scale });
    await page.render({ canvasContext: offCtx, viewport }).promise;
    // 裁切内容矩形到可见 canvas
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('canvas 2d 上下文不可用');
    canvas.width = Math.max(1, Math.floor(crop.w * scale));
    canvas.height = Math.max(1, Math.floor(crop.h * scale));
    ctx.drawImage(
      off,
      crop.x * scale,
      crop.y * scale,
      crop.w * scale,
      crop.h * scale,
      0,
      0,
      canvas.width,
      canvas.height,
    );
  };

  return { pages, ready: doc !== null, error, renderPage, renderPageCrop };
}
