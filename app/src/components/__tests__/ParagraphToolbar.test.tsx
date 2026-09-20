import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import ParagraphToolbar from '../ParagraphToolbar';
import type { Block } from '../../types';

vi.mock('../../services/backend', () => ({
  retranslateParagraph: vi.fn(),
}));
import { retranslateParagraph } from '../../services/backend';

const block: Block = {
  type: 'paragraph',
  id: 'b_0001',
  style: {},
  content: [{ type: 'text', text: '原文内容' }],
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.mocked(retranslateParagraph).mockReset();
});

describe('ParagraphToolbar hover 工具条（T7.2）', () => {
  it('渲染三个动作按钮', () => {
    render(
      <ParagraphToolbar
        docId="d"
        block={block}
        translatedText="译文内容"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    expect(screen.getByTestId('copy-source-btn')).toBeTruthy();
    expect(screen.getByTestId('copy-target-btn')).toBeTruthy();
    expect(screen.getByTestId('retranslate-btn')).toBeTruthy();
  });

  it('点击重译 → 调 API → onRetranslated 回调（translated 更新）', async () => {
    vi.mocked(retranslateParagraph).mockResolvedValue({
      block_id: 'b_0001',
      original: block,
      translated: {
        type: 'paragraph',
        id: 'b_0001',
        style: {},
        content: [{ type: 'text', text: '新译文' }],
      },
      fallback: false,
    });
    const onRetranslated = vi.fn();
    render(
      <ParagraphToolbar
        docId="d"
        block={block}
        translatedText="旧译文"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={onRetranslated}
      />,
    );
    fireEvent.click(screen.getByTestId('retranslate-btn'));
    await waitFor(() =>
      expect(retranslateParagraph).toHaveBeenCalledWith('d', 'b_0001', 'en', 'zh'),
    );
    await waitFor(() => expect(onRetranslated).toHaveBeenCalled());
    expect(onRetranslated.mock.calls[0][0].block).toMatchObject({
      type: 'paragraph',
      id: 'b_0001',
    });
  });

  it('复制译文走 clipboard；重译失败不打断（静默）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    vi.mocked(retranslateParagraph).mockRejectedValue(new Error('网络错误'));
    const onRetranslated = vi.fn();
    render(
      <ParagraphToolbar
        docId="d"
        block={block}
        translatedText="要复制的译文"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={onRetranslated}
      />,
    );
    fireEvent.click(screen.getByTestId('copy-target-btn'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('要复制的译文'));
    // 未翻译（translatedText=null）复制原文
    fireEvent.click(screen.getByTestId('copy-source-btn'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('原文内容'));
    // 重译失败：不抛异常、无回调
    fireEvent.click(screen.getByTestId('retranslate-btn'));
    await waitFor(() => expect(retranslateParagraph).toHaveBeenCalled());
    expect(onRetranslated).not.toHaveBeenCalled();
  });
});

describe('ParagraphToolbar（v2.7 右外侧垂直图标列）', () => {
  it('容器定位在段落右外侧（translate-x-full）且垂直排列（flex-col），不遮挡正文', () => {
    render(
      <ParagraphToolbar
        docId="d"
        block={block}
        translatedText="译文内容"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    const bar = screen.getByTestId('paragraph-toolbar-b_0001');
    expect(bar.className).toContain('translate-x-full'); // 整列移出段落右缘
    expect(bar.className).toContain('flex-col'); // 垂直排列
    expect(bar.className).toContain('group-hover:flex'); // hover 显示
    expect(bar.className).toContain('right-0');
  });

  it('三枚图标按钮：svg 图标 + title/aria-label 齐备', () => {
    render(
      <ParagraphToolbar
        docId="d"
        block={block}
        translatedText="译文内容"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    for (const [tid, label] of [
      ['copy-source-btn', '复制原文'],
      ['copy-target-btn', '复制译文'],
      ['retranslate-btn', '重译本段'],
    ] as const) {
      const btn = screen.getByTestId(tid) as HTMLButtonElement;
      expect(btn.title).toBe(label);
      expect(btn.getAttribute('aria-label')).toBe(label);
      expect(btn.querySelector('svg')).toBeTruthy(); // 图标按钮
    }
  });

  it('复制成功后按钮进入已复制态（data-copied + 图标换对勾）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <ParagraphToolbar
        docId="d"
        block={block}
        translatedText="译文内容"
        sourceLang="en"
        targetLang="zh"
        onRetranslated={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('copy-source-btn'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('原文内容'));
    const btn = screen.getByTestId('copy-source-btn');
    expect(btn.dataset.copied).toBe('true');
    expect(btn.title).toBe('已复制');
    // 1.2s 后反馈复位
    await waitFor(
      () => expect(screen.getByTestId('copy-source-btn').dataset.copied).toBeUndefined(),
      { timeout: 2000 },
    );
  });
});
