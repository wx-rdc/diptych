import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HistoryPanel from '../HistoryPanel';

afterEach(cleanup);

const items = [
  { doc_id: 'd1', file_name: 'paper.pdf', file_type: 'pdf', title: 'Paper', pages: 9, uploaded_at: 1700000000, status: 'translated' },
  { doc_id: 'd2', file_name: 'notes.docx', file_type: 'docx', title: 'Notes', pages: 2, uploaded_at: 1700000100, status: 'parsed' },
];

describe('HistoryPanel', () => {
  it('渲染条目：文件名/状态文案，点击回调 onOpen', () => {
    const onOpen = vi.fn();
    render(<HistoryPanel items={items} loading={false} onOpen={onOpen} onClose={() => {}} />);
    expect(screen.getByText('paper.pdf')).toBeInTheDocument();
    expect(screen.getByText('notes.docx')).toBeInTheDocument();
    expect(screen.getByText('已翻译')).toBeInTheDocument();
    expect(screen.getByText('已解析')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('history-item-d1'));
    expect(onOpen).toHaveBeenCalledWith(items[0]);
  });

  it('loading 态与空态', () => {
    const { rerender } = render(<HistoryPanel items={[]} loading onOpen={() => {}} onClose={() => {}} />);
    expect(screen.getByTestId('history-loading')).toBeInTheDocument();
    rerender(<HistoryPanel items={[]} loading={false} onOpen={() => {}} onClose={() => {}} />);
    expect(screen.getByTestId('history-empty')).toBeInTheDocument();
  });
});
