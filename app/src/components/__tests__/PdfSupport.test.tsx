import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import App from '../../App';
import DocumentViewer from '../DocumentViewer';
import { parseDoc, translateDoc } from '../../services/backend';
import type { GDocument } from '../../types';

vi.mock('../../services/backend', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../services/backend')>();
  return {
    ...actual,
    parseDoc: vi.fn(),
    translateDoc: vi.fn(),
  };
});

/** 模拟 PDF 解析器输出的 IR（two_column.pdf 形状） */
const pdfDoc: GDocument = {
  id: 'd-pdf',
  title: 'two_column',
  blocks: [
    { type: 'paragraph', id: 'b_0000', style: { heading_level: 1 }, content: [{ type: 'text', text: 'A Two Column Study' }] },
    { type: 'paragraph', id: 'b_0001', style: {}, content: [{ type: 'text', text: 'LEFTCOL-MARKER first paragraph in left column.' }] },
    { type: 'paragraph', id: 'b_0002', style: { is_list_item: true }, content: [{ type: 'text', text: '• a bullet from pdf' }] },
    { type: 'image', id: 'b_0003', resource_id: 'pdf_img_p0_0', width: 160, height: 100, alt_text: null },
    { type: 'paragraph', id: 'b_0004', style: {}, content: [{ type: 'text', text: 'RIGHTCOL-MARKER right column text.' }] },
    { type: 'empty', id: 'b_0005' },
  ],
  resources: { pdf_img_p0_0: { kind: 'image', mime: 'image/jpeg' } },
};

beforeEach(() => {
  vi.mocked(parseDoc).mockReset();
  // v2.4 零点击：解析成功会自动翻译——挂起不影响渲染断言
  vi.mocked(translateDoc).mockReset();
  vi.mocked(translateDoc).mockImplementation(() => new Promise(() => {}));
});

afterEach(cleanup);

describe('DocumentViewer 文件类型徽章（T10.2）', () => {
  it('fileType=pdf 时显示 PDF 徽章', () => {
    render(<DocumentViewer doc={pdfDoc} translatedMap={{}} activeId={null} onSelect={() => {}} fileType="pdf" />);
    const badges = screen.getAllByTestId('file-type-badge');
    expect(badges).toHaveLength(1); // 仅原文栏
    expect(badges[0]).toHaveTextContent('PDF');
  });

  it('fileType=docx 时显示 DOCX 徽章', () => {
    render(<DocumentViewer doc={pdfDoc} translatedMap={{}} activeId={null} onSelect={() => {}} fileType="docx" />);
    expect(screen.getByTestId('file-type-badge')).toHaveTextContent('DOCX');
  });

  it('fileType 缺省时不显示徽章', () => {
    render(<DocumentViewer doc={pdfDoc} translatedMap={{}} activeId={null} onSelect={() => {}} />);
    expect(screen.queryByTestId('file-type-badge')).toBeNull();
  });

  it('PDF 文档块序列完整渲染（标题/列表/图片占位/空段）', () => {
    render(<DocumentViewer doc={pdfDoc} translatedMap={{}} activeId={null} onSelect={() => {}} fileType="pdf" />);
    // 双栏各一次
    expect(screen.getAllByText('A Two Column Study').length).toBe(2);
    expect(screen.getAllByText('• a bullet from pdf').length).toBe(2);
    expect(screen.getAllByText('RIGHTCOL-MARKER right column text.').length).toBe(2);
    // PDF 图片（无 alt_text）：占位 img 两侧各一
    const imgs = screen.getAllByRole('img');
    expect(imgs.length).toBe(2);
    expect(imgs[0].getAttribute('src')).toContain('/api/resource/d-pdf/pdf_img_p0_0');
  });
});

describe('App PDF 流程（T10.1/T10.3）', () => {
  it('打开 .pdf 路径 → PDF 徽章 + 内容渲染', async () => {
    vi.mocked(parseDoc).mockResolvedValue(pdfDoc);
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/home/u/papers/two_column.pdf' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getAllByText('A Two Column Study').length).toBe(2));
    expect(screen.getByTestId('file-type-badge')).toHaveTextContent('PDF');
    expect(parseDoc).toHaveBeenCalledWith('/home/u/papers/two_column.pdf', 'auto', expect.any(Function));
  });

  it('打开 .docx 路径 → DOCX 徽章（回归）', async () => {
    vi.mocked(parseDoc).mockResolvedValue(pdfDoc);
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/rich.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getByTestId('file-type-badge')).toHaveTextContent('DOCX'));
  });

  it('路径输入框提示包含 .pdf', () => {
    render(<App />);
    expect(screen.getByTestId('file-path-input').getAttribute('placeholder')).toContain('.pdf');
  });

  it('解析失败 → 错误仍正常展示（加密/损坏 PDF 的 422 文案）', async () => {
    vi.mocked(parseDoc).mockRejectedValue(new Error('加密 PDF 暂不支持，请先解密后重试'));
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/x/enc.pdf' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => {
      expect(screen.getByTestId('empty-error')).toHaveTextContent('加密 PDF 暂不支持');
    });
  });
});
