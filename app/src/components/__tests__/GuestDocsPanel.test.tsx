import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import GuestDocsPanel from '../GuestDocsPanel';
import * as guestDocs from '../../services/guestDocs';

vi.mock('../../services/guestDocs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/guestDocs')>()),
  fetchGuestUsage: vi.fn(),
}));

afterEach(() => {
  sessionStorage.clear();
  vi.clearAllMocks();
});

describe('GuestDocsPanel（v2.4.1 游客翻译记录）', () => {
  it('本地记录 + 服务端明细合并去重，点击回访', async () => {
    sessionStorage.setItem(
      'geminus.guestDocs',
      JSON.stringify([{ doc_id: 'd1', file_name: 'local.pdf', at: 2000 }]),
    );
    vi.mocked(guestDocs.fetchGuestUsage).mockResolvedValue([
      { doc_id: 'd2', file_name: 'remote.pdf', at: 3000 },
      { doc_id: 'd1', file_name: 'local.pdf', at: 1000 }, // 重复：取较新（本地 2000）
    ]);
    const onOpen = vi.fn();
    render(<GuestDocsPanel onOpen={onOpen} onClose={() => {}} />);

    await waitFor(() => expect(screen.getAllByTestId('guest-doc-item')).toHaveLength(2));
    const items = screen.getAllByTestId('guest-doc-item');
    // 新→旧：d2（3000）在前
    expect(items[0].textContent).toContain('remote.pdf');
    fireEvent.click(items[0]);
    expect(onOpen).toHaveBeenCalledWith('d2', 'remote.pdf');
    // d1 保留（去重后仍在）
    expect(screen.getAllByTestId('guest-doc-item')[1].textContent).toContain('local.pdf');
  });

  it('空记录：展示引导文案', async () => {
    vi.mocked(guestDocs.fetchGuestUsage).mockResolvedValue([]);
    render(<GuestDocsPanel onOpen={() => {}} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText(/暂无翻译记录/)).toBeTruthy());
  });

  it('服务端查询失败：降级为本地记录', async () => {
    sessionStorage.setItem(
      'geminus.guestDocs',
      JSON.stringify([{ doc_id: 'd1', file_name: 'local.pdf', at: 1000 }]),
    );
    vi.mocked(guestDocs.fetchGuestUsage).mockRejectedValue(new Error('offline'));
    render(<GuestDocsPanel onOpen={() => {}} onClose={() => {}} />);
    await waitFor(() => expect(screen.getAllByTestId('guest-doc-item')).toHaveLength(1));
  });
});
