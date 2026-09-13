import { useCallback, useEffect, useState } from 'react';
import Popover from '../Popover';
import { fetchQuota, QUOTA_CHANGED_EVENT, type QuotaStatus } from '../../services/backend';

interface Props {
  onLoginClick: () => void;
  /** 游客：查看翻译记录（额度去向） */
  onViewUsage?: () => void;
}

/**
 * 配额 Chip ◔ N页（ui_design_0913 §三⑥/§四）：
 * 顶栏五个碎片文字（游客模式/免登录/剩余/24h/去向）收敛为一个 Chip；
 * unlimited（本地直连）/查询失败 → 整体隐藏。
 */
export default function QuotaChip({ onLoginClick, onViewUsage }: Props) {
  const [quota, setQuota] = useState<QuotaStatus | null>(null);

  const refresh = useCallback(() => {
    fetchQuota()
      .then((q) => setQuota(q.unlimited || q.remaining == null ? null : q))
      .catch(() => setQuota(null));
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  // v2.6.1：额度变化（翻译批次结束/402 熔断/登录登出）与回访时即时刷新，消除陈旧「剩余 N 页」
  useEffect(() => {
    const onFocus = () => refresh();
    window.addEventListener(QUOTA_CHANGED_EVENT, refresh);
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onFocus);
    return () => {
      window.removeEventListener(QUOTA_CHANGED_EVENT, refresh);
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onFocus);
    };
  }, [refresh]);

  if (!quota) return null;
  const who = quota.anonymous ? '游客额度' : '每日额度';
  const exhausted = quota.remaining === 0;
  return (
    <Popover
      panelTestId="quota-popover"
      panelClassName="w-60"
      trigger={({ toggle }) => (
        <button
          onClick={toggle}
          data-testid="quota-indicator"
          title={`${who}：已用 ${quota.used ?? 0} / ${quota.limit ?? 0} 页（每 24 小时滚动恢复）`}
          className={`flex h-6 shrink-0 items-center gap-1 rounded-full px-2 text-[11px] font-medium tabular-nums hover:brightness-95 ${exhausted ? 'bg-amber-50 text-amber-700' : 'bg-slate-100 text-slate-500'}`}
        >
          ◔ {quota.remaining}页
        </button>
      )}
      panel={(close) => (
        <div>
          <p className="mb-1 text-[11px] font-semibold text-slate-400">{who}</p>
          <p className="text-xs text-slate-700">
            剩余 {quota.remaining} 页 / 每 24h {quota.limit ?? 0} 页
          </p>
          <p className="mt-0.5 text-[11px] text-slate-400">已用 {quota.used ?? 0} 页 · 滚动恢复</p>
          {quota.anonymous ? <p className="mt-0.5 text-[11px] text-slate-400">匿名文档 24h 后自动清理</p> : null}
          <div className="mt-2 flex flex-col gap-1.5 border-t border-slate-100 pt-2">
            {quota.anonymous && onViewUsage ? (
              <button
                onClick={() => { onViewUsage(); close(); }}
                data-testid="quota-popover-usage-btn"
                className="rounded border border-slate-300 px-2 py-1 font-medium text-slate-600 hover:bg-slate-100"
              >
                🕘 查看翻译记录
              </button>
            ) : null}
            {quota.anonymous ? (
              <button
                onClick={() => { onLoginClick(); close(); }}
                data-testid="quota-popover-login-btn"
                className="rounded bg-blue-600 px-2 py-1 font-medium text-white hover:bg-blue-500"
              >
                登录解锁每日 1000 页
              </button>
            ) : null}
          </div>
        </div>
      )}
    />
  );
}
