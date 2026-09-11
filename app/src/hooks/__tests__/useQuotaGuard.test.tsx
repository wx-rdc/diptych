import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useQuotaGuard, QUOTA_GUARD_PASS, type PagesReader } from '../useQuotaGuard';
import * as backend from '../../services/backend';

vi.mock('../../services/backend', async (importOriginal) => {
  return {
    ...(await importOriginal<typeof import('../../services/backend')>()),
    fetchQuota: vi.fn(),
  };
});

function quota(remaining: number | null, anonymous = remaining != null) {
  return {
    anonymous,
    unlimited: remaining == null,
    limit: anonymous ? 5 : undefined,
    used: anonymous ? 5 - (remaining ?? 0) : undefined,
    remaining,
  };
}

/** 受控页数读取器（替代 pdf.js 本地实现） */
function pagesReader(n: number | null): PagesReader {
  return async () => n;
}

afterEach(() => {
  vi.clearAllMocks();
});

describe('useQuotaGuard 预检边界（T3.6）', () => {
  it('登录 / 不限额：任何文件放行（不读页数）', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue(quota(null, false));
    const reader = vi.fn(pagesReader(999));
    const { result } = renderHook(() => useQuotaGuard(reader));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'big.pdf'))).toBe(true);
    });
    expect(result.current.result).toEqual(QUOTA_GUARD_PASS);
    expect(reader).not.toHaveBeenCalled();
  });

  it('DOCX 不做本地预检（交后端）→ 放行', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue(quota(1));
    const reader = vi.fn(pagesReader(999));
    const { result } = renderHook(() => useQuotaGuard(reader));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'a.docx'))).toBe(true);
    });
    expect(reader).not.toHaveBeenCalled(); // DOCX 不触发页数读取
  });

  it('配额端点失败 → 放行（后端 402 兜底）', async () => {
    vi.mocked(backend.fetchQuota).mockRejectedValue(new Error('offline'));
    const { result } = renderHook(() => useQuotaGuard(pagesReader(99)));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'a.pdf'))).toBe(true);
    });
  });

  it('页数读取失败（null）→ 放行交后端复核', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue(quota(1));
    const { result } = renderHook(() => useQuotaGuard(pagesReader(null)));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'a.pdf'))).toBe(true);
    });
    expect(result.current.result.blocked).toBe(false);
  });

  it('边界：pages == remaining 放行（恰好用满）', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue(quota(3));
    const { result } = renderHook(() => useQuotaGuard(pagesReader(3)));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'a.pdf'))).toBe(true);
    });
    expect(result.current.result.blocked).toBe(false);
  });

  it('边界：pages = remaining + 1 拦截（不上传 + 文案含页数与剩余）', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue(quota(3));
    const { result } = renderHook(() => useQuotaGuard(pagesReader(4)));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'a.pdf'))).toBe(false);
    });
    expect(result.current.result.blocked).toBe(true);
    expect(result.current.result.pages).toBe(4);
    expect(result.current.result.remaining).toBe(3);
    expect(result.current.result.message).toContain('4 页');
    expect(result.current.result.message).toContain('剩余 3 页');
  });

  it('reset 清除拦截态', async () => {
    vi.mocked(backend.fetchQuota).mockResolvedValue(quota(1));
    const { result } = renderHook(() => useQuotaGuard(pagesReader(9)));
    await act(async () => {
      expect(await result.current.check(new File(['x'], 'a.pdf'))).toBe(false);
    });
    act(() => { result.current.reset(); });
    expect(result.current.result).toEqual(QUOTA_GUARD_PASS);
  });
});
