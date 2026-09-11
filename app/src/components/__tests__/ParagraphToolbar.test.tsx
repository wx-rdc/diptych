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

afterEach(cleanup);

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
