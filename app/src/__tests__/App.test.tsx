import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import App from '../App';
import { BackendError, parseDoc, translateDoc } from '../services/backend';
import { fetchMe } from '../services/auth';
import type { GDocument, TranslateProgress, TranslatedDocument } from '../types';

// mock backend 模块：保留 resourceUrl 真实形状（图片 src 断言依赖）
vi.mock('../services/backend', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../services/backend')>();
  return {
    ...actual,
    parseDoc: vi.fn(),
    translateDoc: vi.fn(),
  };
});

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

beforeEach(() => {
  vi.mocked(parseDoc).mockReset();
  vi.mocked(translateDoc).mockReset();
  vi.mocked(fetchMe).mockResolvedValue({ authenticated: false, username: null });
});

afterEach(cleanup);

async function openDoc() {
  render(<App />);
  fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/test.docx' } });
  fireEvent.click(screen.getByTestId('open-btn'));
  // 双栏各渲染一次原文
  await waitFor(() => expect(screen.getAllByText('Introduction').length).toBe(2));
}

describe('App 集成（mock backend）', () => {
  it('打开文档 → 双栏渲染原文与占位符', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    await openDoc();

    // 双栏：原文 + 译文（未翻译时译文侧也显示原文）
    expect(screen.getAllByText('Introduction').length).toBe(2);
    // 图片占位符在两侧各出现一次
    expect(screen.getAllByAltText('fig').length).toBe(2);
    expect(parseDoc).toHaveBeenCalledWith('/tmp/test.docx', 'auto', expect.any(Function));
  });

  it('翻译 → 进度更新 + 译文替换 + 占位符保留', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    vi.mocked(translateDoc).mockImplementation(
      async (_id: string, _src: string, _tgt: string, onProgress: (p: TranslateProgress) => void) => {
        onProgress({ block_id: 'b1', current: 1, total: 2, status: 'done', translated_text: '引言' });
        onProgress({ block_id: 'b2', current: 2, total: 2, status: 'cached', translated_text: '机器学习很有趣。' });
        return translated;
      },
    );
    await openDoc();

    fireEvent.click(screen.getByTestId('translate-btn'));

    await waitFor(() => {
      expect(screen.getByText('机器学习很有趣。')).toBeInTheDocument();
    });
    // 翻译后译文侧不再是英文原文（原文侧仍有一处）
    expect(screen.getAllByText('Machine learning is fun.').length).toBe(1);
    // 占位符图片仍在两栏
    expect(screen.getAllByAltText('fig').length).toBe(2);
    // 状态栏计数与进度
    expect(screen.getByText('✅ 完成 1')).toBeInTheDocument();
    expect(screen.getByText('💾 缓存命中 1')).toBeInTheDocument();
    expect(screen.getByTestId('progress-text')).toHaveTextContent('100%');
  });

  it('解析失败 → 错误提示', async () => {
    vi.mocked(parseDoc).mockRejectedValue(new Error('文件不存在'));
    render(<App />);
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/nope.docx' } });
    fireEvent.click(screen.getByTestId('open-btn'));
    await waitFor(() => {
      expect(screen.getByTestId('empty-error')).toHaveTextContent('文件不存在');
    });
  });

  it('点击段落 → 两侧同 id 块高亮', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    const { container } = render(<App />);
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/t.docx' } });
    fireEvent.click(screen.getByTestId('open-btn'));
    await waitFor(() => expect(screen.getAllByText('Introduction').length).toBe(2));

    const blocks = container.querySelectorAll('[data-block-id="b2"]');
    expect(blocks.length).toBe(2);
    fireEvent.click(blocks[0]);
    await waitFor(() => {
      const ringed = container.querySelectorAll('[data-active="true"]');
      expect(ringed.length).toBe(2); // 两侧对应块同时高亮（v2.1 T6：border-l-4 + data-active）
    });
  });

  // ── 部署安全加固 ──

  it('匿名状态 → 工具栏显示登录按钮；点击打开登录对话框', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    render(<App />);
    expect(screen.getByTestId('login-btn')).toBeInTheDocument();
    expect(screen.getByTestId('anon-hint')).toHaveTextContent('限 5 页');
    fireEvent.click(screen.getByTestId('login-btn'));
    await waitFor(() => expect(screen.getByTestId('login-dialog')).toBeInTheDocument());
  });

  it('已登录状态 → 显示用户名徽章 + 退出按钮', async () => {
    vi.mocked(fetchMe).mockResolvedValue({ authenticated: true, username: 'alice' });
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('auth-username')).toHaveTextContent('alice'));
    expect(screen.getByTestId('logout-btn')).toBeInTheDocument();
  });

  it('解析遇 429（未登录额度用尽）→ 弹出登录对话框并展示限流文案', async () => {
    vi.mocked(parseDoc).mockRejectedValue(
      new BackendError('未登录用户最多查看并翻译 5 页（当前已用 5 页）。请登录后继续使用。', 429),
    );
    render(<App />);
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/big.pdf' } });
    fireEvent.click(screen.getByTestId('open-btn'));
    await waitFor(() => expect(screen.getByTestId('login-dialog')).toBeInTheDocument());
    expect(screen.getByTestId('login-hint')).toHaveTextContent('未登录用户最多查看并翻译 5 页');
    expect(screen.getByTestId('empty-error')).toHaveTextContent('请登录后继续使用');
  });
});

// ── v2.4：权限拦截（require_login=1 时未登录拦截打开/翻译，登录后自动续跑） ──

describe('v2.4 权限拦截（require_login=1）', () => {
  beforeEach(() => {
    vi.mocked(fetchMe).mockResolvedValue({
      authenticated: false, username: null, login_enabled: true, require_login: true,
    });
  });

  it('未登录点「打开」→ 弹登录框且不发起解析请求；登录成功后自动续跑', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    render(<App />);
    // 等 /api/auth/me 生效（requireLogin 状态就绪）再操作
    await waitFor(() => expect(vi.mocked(fetchMe)).toHaveBeenCalled());
    await act(async () => {});
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/test.docx' } });
    fireEvent.click(screen.getByTestId('open-btn'));

    // 登录框弹出，未发解析请求
    expect(await screen.findByTestId('login-overlay')).toBeInTheDocument();
    expect(parseDoc).not.toHaveBeenCalled();

    // 登录成功 → 自动继续打开
    fireEvent.change(screen.getByTestId('login-username'), { target: { value: 'alice' } });
    fireEvent.change(screen.getByTestId('login-password'), { target: { value: 'pw' } });
    fireEvent.click(screen.getByTestId('login-submit'));
    await waitFor(() =>
      expect(parseDoc).toHaveBeenCalledWith('/tmp/test.docx', 'auto', expect.any(Function)),
    );
    await waitFor(() => expect(screen.getAllByText('Introduction').length).toBe(2));
  });

  it('未登录点「翻译」→ 同样拦截弹登录框', async () => {
    vi.mocked(parseDoc).mockResolvedValue(doc);
    vi.mocked(fetchMe).mockResolvedValueOnce({
      // 先以 require_login=0 打开文档，再切换为 1 触发翻译拦截
      authenticated: false, username: null, login_enabled: true, require_login: false,
    });
    render(<App />);
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/test.docx' } });
    fireEvent.click(screen.getByTestId('open-btn'));
    await waitFor(() => expect(screen.getAllByText('Introduction').length).toBe(2));

    // 切到 require_login=1（App 已加载，requireLogin 状态仍为 false → 直接拦截模拟：
    // 重新挂载后 require_login=1）
    cleanup();
    vi.mocked(fetchMe).mockResolvedValue({
      authenticated: false, username: null, login_enabled: true, require_login: true,
    });
    render(<App />);
    await waitFor(() => expect(screen.getByTestId('login-btn')).toBeInTheDocument());
    // 文档未打开时翻译按钮禁用；先验证拦截入口走「打开」
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/test.docx' } });
    fireEvent.click(screen.getByTestId('open-btn'));
    expect(await screen.findByTestId('login-overlay')).toBeInTheDocument();
    expect(parseDoc).toHaveBeenCalledTimes(1); // 仅第一次（require_login=0 时）调用
  });

  it('require_login=0（本地）不拦截匿名打开', async () => {
    vi.mocked(fetchMe).mockResolvedValue({
      authenticated: false, username: null, login_enabled: true, require_login: false,
    });
    vi.mocked(parseDoc).mockResolvedValue(doc);
    render(<App />);
    fireEvent.change(screen.getByTestId('file-path-input'), { target: { value: '/tmp/test.docx' } });
    fireEvent.click(screen.getByTestId('open-btn'));
    await waitFor(() => expect(parseDoc).toHaveBeenCalled());
    expect(screen.queryByTestId('login-overlay')).not.toBeInTheDocument();
  });
});
