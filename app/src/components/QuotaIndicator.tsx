import { useCallback, useEffect, useState } from 'react';
import { fetchQuota, type QuotaStatus } from '../services/backend';

interface Props {
  /** v2.4.1：点击打开游客翻译记录（额度「用在哪了」明细） */
  onViewUsage?: () => void;
}

/**
 * v2.4 T6.1：配额指示器（顶栏匿名区）。
 * - 游客：剩余 N 页（挂载时拉取；不限量/查询失败时隐藏数字）；
 * - 点击展开翻译记录明细（额度去向可追溯）；
 * - 登录：不渲染（用户名徽章即身份标识，翻译不限量）。
 */
export default function QuotaIndicator({ onViewUsage }: Props) {
  const [quota, setQuota] = useState<QuotaStatus | null>(null);

  const refresh = useCallback(() => {
    fetchQuota()
      .then((q) => setQuota(q.anonymous ? q : null))
      .catch(() => setQuota(null));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!quota) return null;
  const body = (
    <>
      剩余 {quota.remaining ?? '?'} 页
      <span className="ml-1 text-[10px] text-slate-400">{onViewUsage ? '· 去向 ↗' : ''}</span>
    </>
  );
  if (onViewUsage) {
    return (
      <button
        onClick={onViewUsage}
        className="rounded-full bg-slate-100 px-2 py-1 text-xs tabular-nums text-slate-500 hover:bg-slate-200"
        data-testid="quota-indicator"
        title={`游客额度：已用 ${quota.used ?? 0} / ${quota.limit ?? 0} 页。点击查看翻译记录`}
      >
        {body}
      </button>
    );
  }
  return (
    <span
      className="rounded-full bg-slate-100 px-2 py-1 text-xs tabular-nums text-slate-500"
      data-testid="quota-indicator"
      title={`游客额度：已用 ${quota.used ?? 0} / ${quota.limit ?? 0} 页`}
    >
      {body}
    </span>
  );
}
