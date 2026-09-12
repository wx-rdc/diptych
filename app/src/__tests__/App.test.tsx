import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import App from '../App';
import { BackendError, parseDoc, translateDoc, fetchQuota as backendFetchQuota } from '../services/backend';
import { fetchMe } from '../services/auth';
import type { GDocument, TranslateProgress, TranslatedDocument } from '../types';

// mock backend 模块：保留 resourceUrl 真实形状（图片 src 断言依赖）
vi.mock('../services/backend', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../services/backend')>();
  return {
    ...actual,
    parseDoc: vi.fn(),
    translateDoc: vi.fn(),
    fetchQuota: vi.fn().mockResolvedValue({ anonymous: false, unlimited: true, remaining: null }),
  };
});

// mock pdf.js 模块加载（预检读页受控：统一 11 页）
vi.mock('../hooks/usePdfRenderer', () => ({
  loadPdfjs: async () => ({
    // 真实 pdfjs getDocument 同步返回 task（含 .promise），mock 语义保持一致
    getDocument: () => ({
      promise: Promise.resolve({ numPages: 11, destroy: async () => {} }),
    }),
  }),
}));

// mock 认证服务：默认匿名
vi.mock('../services/auth', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../services/auth')>();
  return {
    ...actual,
    fetchMe: vi.fn().mockResolvedValue({ authenticated: false, username: null }),
    login: vi.fn().mockResolvedValue({ authenticated: true, username: 'alice' }),
    logout: vi.fn().mockResolvedValue(undefined),
  };
});

const doc: GDocument = {
  id: 'd1',
  title: 'Test Doc',
  blocks: [
    { type: 'paragraph', id: 'b1', style: { heading_level: 1 }, content: [{ type: 'text', text: 'Introduction' }] },
    { type: 'paragraph', id: 'b2', style: {}, content: [{ type: 'text', text: 'Machine learning is fun.' }] },
    { type: 'image', id: 'b3', resource_id: 'r01_image', width: 100, height: 60, alt_text: 'fig' },
    { type: 'empty', id: 'b4' },
  ],
  resources: { r01_image: { kind: 'image', mime: 'image/png' } },
};

const translated: TranslatedDocument = {
  doc_id: 'd1',
  pairs: [
    { block_id: 'b1', original: doc.blocks[0], translated: { type: 'paragraph', id: 'b1', style: { heading_level: 1 }, content: [{ type: 'text', text: '引言' }] } },
    { block_id: 'b2', original: doc.blocks[1], translated: { type: 'paragraph', id: 'b2', style: {}, content: [{ type: 'text', text: '机器学习很有趣。' }] } },
    { block_id: 'b3', original: doc.blocks[2], translated: null },
    { block_id: 'b4', original: doc.blocks[3], translated: null },
  ],
};

/** 挂起的翻译（默认：自动翻译不产生结果，原文渲染断言不受干扰） */
const pendingTranslate = () => new Promise<TranslatedDocument>(() => {});

beforeEach(() => {
  vi.mocked(parseDoc).mockReset();
  vi.mocked(translateDoc).mockReset();
  vi.mocked(translateDoc).mockImplementation(pendingTranslate);
  vi.mocked(fetchMe).mockResolvedValue({ authenticated: false, username: null });
  vi.mocked(backendFetchQuota).mockResolvedValue({ anonymous: false, unlimited: true, remaining: null });
});

afterEach(cleanup);

/**
 * v2.4 T3 零点击：输入本地路径 + Enter → 自动解析（+自动翻译挂起）。
 * 「打开」按钮已移除——Enter 是本地路径开发分支的流水线触发方式。
 */
async function openDoc(path = '/tmp/test.docx') {
  render(<App />);
  const input = screen.getByTestId('file-path-input');
  fireEvent.change(input, { target: { value: path } });
  fireEvent.keyDown(input, { key: 'Enter' });
  // 双栏各渲染一次原文
  await waitFor(() => expect(screen.getAllByText('Introduction').length).toBe(2));
}

describe('App 集成（mock backend，零点击流水线）', () => {
  it('Enter 打开文档 → 双栏渲染原文与占位符（自动解析）', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    await openDoc();

    expect(screen.getAllByText('Introduction').length).toBe(2);
    expect(screen.getAllByAltText('fig').length).toBe(2);
    expect(parseDoc).toHaveBeenCalledWith('/tmp/test.docx', 'auto', expect.any(Function));
  });

  it('零点击自动翻译：解析完成 → 译文替换 + 占位符保留 + 进度计数', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    vi.mocked(translateDoc).mockImplementation(
      async (_id: string, _src: string, _tgt: string, onProgress: (p: TranslateProgress) => void) => {
        onProgress({ block_id: 'b1', current: 1, total: 2, status: 'done', translated_text: '引言' });
        onProgress({ block_id: 'b2', current: 2, total: 2, status: 'cached', translated_text: '机器学习很有趣。' });
        return translated;
      },
    );
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/test.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    // 无任何额外点击，译文自动出现
    await waitFor(() => {
      expect(screen.getByText('机器学习很有趣。')).toBeInTheDocument();
    });
    expect(screen.getAllByText('Machine learning is fun.').length).toBe(1);
    expect(screen.getAllByAltText('fig').length).toBe(2);
    expect(screen.getByText('✅ 完成 1')).toBeInTheDocument();
    expect(screen.getByText('💾 缓存命中 1')).toBeInTheDocument();
    expect(screen.getByTestId('progress-text')).toHaveTextContent('100%');
    expect(translateDoc).toHaveBeenCalledWith('d1', 'en', 'zh', expect.any(Function));
  });

  it('解析失败 → 错误提示', async () => {
    vi.mocked(parseDoc).mockRejectedValue(new Error('文件不存在'));
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/nope.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => {
      expect(screen.getByTestId('empty-error')).toHaveTextContent('文件不存在');
    });
  });

  it('点击段落 → 两侧同 id 块高亮', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    const { container } = render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/t.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getAllByText('Introduction').length).toBe(2));

    const blocks = container.querySelectorAll('[data-block-id="b2"]');
    expect(blocks.length).toBe(2);
    fireEvent.click(blocks[0]);
    await waitFor(() => {
      const ringed = container.querySelectorAll('[data-active="true"]');
      expect(ringed.length).toBe(2); // 两侧对应块同时高亮（v2.1 T6）
    });
  });

  // ── 部署安全加固 ──

  it('匿名状态 → 工具栏显示登录按钮；点击打开登录对话框', async () => {
    render(<App />);
    expect(screen.getByTestId('login-btn')).toBeInTheDocument();
    expect(screen.getByTestId('anon-hint')).toHaveTextContent('游客模式');
    fireEvent.click(screen.getByTestId('login-btn'));
    await waitFor(() => expect(screen.getByTestId('login-dialog')).toBeInTheDocument());
  });

  it('已登录状态 → 显示用户名徽章 + 退出按钮', async () => {
    vi.mocked(fetchMe).mockResolvedValue({ authenticated: true, username: 'alice' });
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('auth-username')).toHaveTextContent('alice'));
    expect(screen.getByTestId('logout-btn')).toBeInTheDocument();
  });
});

// ── v2.4：402/401 软性引导（替代旧 require_login 前端拦截） ──

describe('v2.4 配额与权限引导', () => {
  it('解析遇 402（额度不足）→ QuotaExceededCard 引导登录（非弹框拦截）', async () => {
    vi.mocked(parseDoc).mockRejectedValue(
      new BackendError('游客最多可翻译 5 页（剩余 0 页，本文件需 10 页）', 402, {
        limit: 5,
        used: 5,
        required: 10,
      }),
    );
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/big.pdf' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getByTestId('quota-exceeded-card')).toBeInTheDocument());
    expect(screen.getByTestId('quota-exceeded-card').textContent).toContain('需要 10 页');
    expect(screen.getByTestId('quota-login-btn')).toBeInTheDocument();
    expect(screen.getByTestId('quota-pick-another-btn')).toBeInTheDocument();
    // precheck 不承诺已译可读
    expect(screen.queryByTestId('quota-view-translated-btn')).toBeNull();
  });

  it('本地预检拦截（11 页 > 剩余 2）→ precheck 卡片且不上传', async () => {
    vi.mocked(backendFetchQuota).mockResolvedValue({
      anonymous: true, unlimited: false, limit: 5, used: 3, remaining: 2,
    });
    vi.mocked(parseDoc).mockResolvedValue(doc);
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/big.pdf' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getByTestId('quota-exceeded-card')).toBeInTheDocument());
    const cardText = screen.getByTestId('quota-exceeded-card').textContent ?? '';
    expect(cardText).toContain('该文件未开始翻译');
    expect(cardText).toContain('需要 11 页');
    expect(cardText).toContain('剩余 2 页');
    // 未上传（本地预检拦截在上传之前）
    expect(parseDoc).not.toHaveBeenCalled();
    // 「换个文件试试」→ 关卡片并触发 App 层文件选择
    fireEvent.click(screen.getByTestId('quota-pick-another-btn'));
    expect(screen.queryByTestId('quota-exceeded-card')).toBeNull();
  });

  it('解析遇 401/403（需登录/无权访问）→ 弹出登录对话框并展示文案', async () => {
    vi.mocked(parseDoc).mockRejectedValue(
      new BackendError('请登录后查看与翻译文档', 401),
    );
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/x.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getByTestId('login-dialog')).toBeInTheDocument());
    expect(screen.getByTestId('login-hint')).toHaveTextContent('请登录后查看与翻译文档');
  });

  it('翻译熔断（limit_reached）→ QuotaExceededCard，已译部分保留', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    vi.mocked(translateDoc).mockImplementation(
      async (_id, _src, _tgt, onProgress: (p: TranslateProgress) => void) => {
        onProgress({ block_id: 'b1', current: 1, total: 4, status: 'done', translated_text: '引言' });
        // 第二块熔断
        const { QuotaLimitError } = await import('../services/backend');
        throw new QuotaLimitError({ limit: 5, used: 5, required: 1 });
      },
    );
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/test.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    // 已译部分保留
    await waitFor(() => expect(screen.getByText('引言')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByTestId('quota-exceeded-card')).toBeInTheDocument());
    // limit variant：承诺已译可读 + [查看已译部分] 打开游客记录面板
    fireEvent.click(screen.getByTestId('quota-view-translated-btn'));
    expect(screen.queryByTestId('quota-exceeded-card')).toBeNull();
    expect(await screen.findByTestId('guest-docs-panel')).toBeInTheDocument();
  });

  it('翻译取消（cancelled）→ 暂停态 + [继续翻译] 按钮', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    vi.mocked(translateDoc).mockImplementation(async () => {
      const { TranslateCancelledError } = await import('../services/backend');
      throw new TranslateCancelledError(2);
    });
    render(<App />);
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/test.docx' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getByTestId('translate-control')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByTestId('translate-continue-btn')).toBeInTheDocument());
  });
});
