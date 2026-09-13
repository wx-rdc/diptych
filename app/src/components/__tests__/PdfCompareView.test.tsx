import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { GDocument, Block } from '../../types';
import { progressLabel } from '../toolbar/ToolbarSlim';

// pdfjs-dist 整体 mock：提供最小 getDocument/GlobalWorkerOptions
vi.mock('pdfjs-dist', () => {
  const doc = {
    numPages: 1,
    getPage: async () => ({
      getViewport: ({ scale }: { scale: number }) => ({ width: 612 * scale, height: 792 * scale }),
      render: () => ({ promise: Promise.resolve() }),
    }),
    destroy: async () => {},
  };
  return {
    GlobalWorkerOptions: { workerSrc: '' },
    getDocument: vi.fn(() => ({ promise: Promise.resolve(doc) })),
  };
});

import { getDocument } from 'pdfjs-dist';
import PdfCompareView from '../PdfCompareView';

function pdfDoc(): GDocument {
  const para = (id: string, text: string): Block => ({
    type: 'paragraph',
    id,
    style: {},
    content: [{ type: 'text', text }],
  });
  return {
    id: 'doc1',
    title: 'Sample PDF',
    blocks: [para('b_0000', 'First paragraph'), para('b_0001', 'Second paragraph')],
    resources: {},
    render_meta: {
      b_0000: { page_idx: 0, bbox: [50, 60, 500, 100] },
      b_0001: { page_idx: 0, bbox: [50, 200, 300, 260] },
    },
    pdf_meta: { page_count: 1, page_sizes: [[612, 792]] },
  };
}

describe('progressLabel（MinerU 进度文案）', () => {
  it('各阶段生成人类可读文案', () => {
    expect(progressLabel({ stage: 'splitting', chunks: 3 })).toContain('3 个片段');
    expect(progressLabel({ stage: 'uploading', current: 1, total: 3 })).toContain('1/3');
    expect(progressLabel({ stage: 'parsing', done_files: 2, total_files: 3, pages_done: 8, pages_total: 10 })).toContain('2/3');
    expect(progressLabel({ stage: 'parsing', done_files: 2, total_files: 3, pages_done: 8, pages_total: 10 })).toContain('8/10');
    expect(progressLabel({ stage: 'merging' })).toContain('合并');
    expect(progressLabel({ stage: 'fallback' })).toContain('回退');
    expect(progressLabel({ stage: 'cached' })).toContain('缓存');
  });
});

describe('PdfCompareView 页码标签（v2.1 T6.3）', () => {
  it('右栏按页组分隔线出现；左栏页码 sticky 标签渲染', async () => {
    const doc = pdfDoc();
    // 将 b_0001 移到第 2 页（render_meta page_idx=1）
    doc.render_meta!.b_0001 = { page_idx: 1, bbox: [50, 200, 300, 260] };
    render(
      <PdfCompareView doc={doc} translatedMap={{}} activeId={null} onSelect={() => {}} />,
    );
    await waitFor(() => expect(screen.getByTestId('pdf-original-viewer')).toBeTruthy());
    // 右栏：第 0 页与第 1 页各一条分隔线
    expect(screen.getByTestId('translated-page-divider-0')).toBeTruthy();
    expect(screen.getByTestId('translated-page-divider-1')).toBeTruthy();
    // 左栏：页码标签（1 页文档）
    expect(screen.getByTestId('page-label-1')).toBeTruthy();
  });
});

describe('PdfCompareView（mock pdfjs）', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('加载 pdf.js 渲染页容器 + 区域叠加层 + 译文块', async () => {
    const doc = pdfDoc();
    render(
      <PdfCompareView
        doc={doc}
        translatedMap={{}}
        activeId={null}
        onSelect={() => {}}
      />,
    );
    // 页容器出现（jsdom 无 IntersectionObserver → 直接渲染）
    const page = await waitFor(() => screen.getByTestId('pdf-page-1'));
    expect(page).toBeTruthy();
    // 译文侧两个块
    expect(screen.getByText('First paragraph')).toBeTruthy();
    expect(screen.getByText('Second paragraph')).toBeTruthy();
    // 区域叠加
    await waitFor(() => expect(screen.getByTestId('region-b_0000')).toBeTruthy());
  });

  it('点击区域触发 onSelect；activeId 区域高亮', async () => {
    const doc = pdfDoc();
    const onSelect = vi.fn();
    const { rerender } = render(
      <PdfCompareView doc={doc} translatedMap={{}} activeId={null} onSelect={onSelect} />,
    );
    await waitFor(() => expect(screen.getByTestId('region-b_0001')).toBeTruthy());
    fireEvent.click(screen.getByTestId('region-b_0001'));
    expect(onSelect).toHaveBeenCalledWith('b_0001');

    rerender(
      <PdfCompareView doc={doc} translatedMap={{}} activeId="b_0001" onSelect={onSelect} />,
    );
    expect(screen.getByTestId('region-b_0001').dataset.active).toBe('true');
  });

  it('左栏致命错误触发 onOriginalFatalError（v2.3 C1 App 降级依据）', async () => {
    vi.mocked(getDocument).mockImplementationOnce(
      () =>
        ({ promise: Promise.reject(new Error('Setting up fake worker failed')) }) as unknown as ReturnType<
          typeof getDocument
        >,
    );
    const onFatal = vi.fn();
    render(
      <PdfCompareView
        doc={pdfDoc()}
        translatedMap={{}}
        activeId={null}
        onSelect={() => {}}
        onOriginalFatalError={onFatal}
      />,
    );
    await waitFor(() =>
      expect(onFatal).toHaveBeenCalledWith(expect.stringContaining('fake worker')),
    );
    // 红字错误 UI 仍在（降级前一帧提示）
    expect(screen.getByTestId('pdf-viewer-error')).toBeTruthy();
  });
});
