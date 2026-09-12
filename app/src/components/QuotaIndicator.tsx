import { useCallback, useEffect, useState } from 'react';
import { fetchQuota, type QuotaStatus } from '../services/backend';

interface Props {
  /** v2.4.1：点击打开游客翻译记录（额度「用在哪了」明细；仅游客有意义） */
  onViewUsage?: () => void;
}

/**
 * v2.5 配额指示器（顶栏）。
 * - 游客与登录用户都渲染（统一 finite 口径：5 页/24h 与 1000 页/24h）；
 * - 本地直连不限量 / 查询失败 → 隐藏；
 * - 游客点击展开翻译记录明细（额度去向可追溯）。
 */
export default function QuotaIndicator({ onViewUsage }: Props) {
  const [quota, setQuota] = useState<QuotaStatus | null>(null);

  const refresh = useCallback(() => {
    fetchQuota()
      .then((q) => setQuota(q.unlimited || q.remaining == null ? null : q))
      .catch(() => setQuota(null));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!quota) return null;
  const who = quota.anonymous ? '游客额度' : '每日额度';
  const body = (
    <>
      剩余 {quota.remaining} 页
      <span className="ml-1 text-[10px] text-slate-400">
        ·24h{quota.anonymous && onViewUsage ? ' · 去向 ↗' : ''}
      </span>
    </>
  );
  const title = `${who}：已用 ${quota.used ?? 0} / ${quota.limit ?? 0} 页（每 24 小时滚动恢复）${
    quota.anonymous ? '。点击查看翻译记录' : ''
  }`;
  if (quota.anonymous && onViewUsage) {
    return (
      <button
        onClick={onViewUsage}
        className="rounded-full bg-slate-100 px-2 py-1 text-xs tabular-nums text-slate-500 hover:bg-slate-200"
        data-testid="quota-indicator"
        title={title}
      >
        {body}
      </button>
    );
  }
  return (
    <span
      className="rounded-full bg-slate-100 px-2 py-1 text-xs tabular-nums text-slate-500"
      data-testid="quota-indicator"
      title={title}
    >
      {body}
    </span>
  );
}
