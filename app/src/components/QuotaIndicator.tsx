import { useEffect, useState } from 'react';
import { fetchQuota, type QuotaStatus } from '../services/backend';

/**
 * v2.4 T6.1：配额指示器（顶栏匿名区）。
 * - 游客：剩余 N 页（挂载时拉取；不限量/查询失败时隐藏数字）；
 * - 登录：不渲染（用户名徽章即身份标识，翻译不限量）。
 */
export default function QuotaIndicator() {
  const [quota, setQuota] = useState<QuotaStatus | null>(null);

  useEffect(() => {
    fetchQuota()
      .then((q) => setQuota(q.anonymous ? q : null))
      .catch(() => setQuota(null));
  }, []);

  if (!quota) return null;
  const remaining = quota.remaining;
  return (
    <span
      className="rounded-full bg-slate-100 px-2 py-1 text-xs tabular-nums text-slate-500"
      data-testid="quota-indicator"
      title={`游客额度：已用 ${quota.used ?? 0} / ${quota.limit ?? 0} 页`}
    >
      剩余 {remaining ?? '?'} 页
    </span>
  );
}
