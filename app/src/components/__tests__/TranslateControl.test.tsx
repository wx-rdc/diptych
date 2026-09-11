import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import TranslateControl from '../TranslateControl';
import * as backend from '../../services/backend';
import type { GDocument } from '../../types';

vi.mock('../../services/backend', () => ({
  cancelTranslate: vi.fn(),
}));

const doc = { id: 'd1', title: 'T', blocks: [], resources: {} } as unknown as GDocument;

afterEach(() => {
  vi.clearAllMocks();
});

describe('TranslateControl（T6）', () => {
  it('idle（非翻译/暂停/完成）不渲染', () => {
    const { container } = render(
      <TranslateControl doc={doc} translating={false} paused={false} done={false} progress={null} onContinue={() => {}} onRetranslate={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it('翻译中：进度条 + 比例文案 + [停止] 触发 cancelTranslate', async () => {
    vi.mocked(backend.cancelTranslate).mockResolvedValue(undefined);
    render(
      <TranslateControl
        doc={doc}
        translating
        paused={false}
        done={false}
        progress={{ block_id: 'b1', current: 3, total: 10, status: 'done' }}
        onContinue={() => {}}
        onRetranslate={() => {}}
      />,
    );
    expect(screen.getByTestId('translate-progress-text').textContent).toContain('3/10');
    expect(screen.getByTestId('translate-progress-text').textContent).toContain('30%');
    fireEvent.click(screen.getByTestId('translate-stop-btn'));
    await waitFor(() => expect(backend.cancelTranslate).toHaveBeenCalledWith('d1'));
  });

  it('暂停态：提示 + [继续翻译] 回调', () => {
    const onContinue = vi.fn();
    render(
      <TranslateControl doc={doc} translating={false} paused done={false} progress={null} onContinue={onContinue} onRetranslate={() => {}} />,
    );
    expect(screen.getByTestId('translate-paused-hint').textContent).toContain('已译部分可阅读');
    fireEvent.click(screen.getByTestId('translate-continue-btn'));
    expect(onContinue).toHaveBeenCalledOnce();
  });

  it('完成态：[重新翻译] 回调', () => {
    const onRetranslate = vi.fn();
    render(
      <TranslateControl doc={doc} translating={false} paused={false} done progress={null} onContinue={() => {}} onRetranslate={onRetranslate} />,
    );
    fireEvent.click(screen.getByTestId('translate-redo-btn'));
    expect(onRetranslate).toHaveBeenCalledOnce();
  });
});
