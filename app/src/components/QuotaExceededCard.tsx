import type { QuotaInfo } from '../services/backend';

interface Props {
  quota: QuotaInfo;
  onLogin: () => void;
  /** 已有已译部分（translated_pages > 0）时展示「查看已译部分」 */
  onViewTranslated?: () => void;
  onDismiss: () => void;
}

/** reset_at（unix 秒）→ 本地 HH:MM 展示 */
function resetHint(resetAt?: number | null): string {
  if (!resetAt) return '';
  const t = new Date(resetAt * 1000);
  const hh = String(t.getHours()).padStart(2, '0');
  const mm = String(t.getMinutes()).padStart(2, '0');
  return `（最早 ${hh}:${mm} 起逐页恢复）`;
}

/**
 * v2.5 额度引导卡片：部分处理语义下的统一文案。
 * - 游客 5 页 / 登录 1000 页，均为 24 小时滚动窗口；
 * - 熔断时已译部分保留可读（translated_pages > 0 → 可查看）；
 * - 游客展示登录引导；登录用户只提示恢复时间。
 */
export default function QuotaExceededCard({
  quota,
  onLogin,
  onViewTranslated,
  onDismiss,
}: Props) {
  const isUser = quota.user === true;
  const translated = quota.translated_pages ?? 0;
  const partial = translated > 0 ? `已翻译的前 ${translated} 页仍可继续阅读，` : '';
  return (
    <div
      className="fixed bottom-16 left-1/2 z-40 w-[min(92vw, 30rem)] -translate-x-1/2 rounded-lg border border-amber-300 bg-amber-50 p-4 shadow-lg"
      data-testid="quota-exceeded-card"
      role="alert"
    >
      <div className="flex items-start gap-2">
        <span className="text-lg">🔒</span>
        <div className="flex-1">
          <p className="text-sm font-medium text-amber-800">
            {isUser ? '24 小时翻译额度已用完' : '游客翻译额度已用完'}
          </p>
          <p className="mt-1 text-xs text-amber-700">
            {isUser
              ? `每 24 小时最多翻译 ${quota.limit} 页（已用 ${quota.used} 页）。${partial}额度随时间逐页恢复${resetHint(quota.reset_at)}。`
              : `游客每 24 小时最多翻译 ${quota.limit} 页（已用 ${quota.used} 页）。${partial}额度随时间逐页恢复${resetHint(quota.reset_at)}；登录后每天可翻译 1000 页，并继续翻译剩余内容。`}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            {!isUser ? (
              <button
                onClick={onLogin}
                className="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500"
                data-testid="quota-login-btn"
              >
                立即登录
              </button>
            ) : null}
            {onViewTranslated && translated > 0 ? (
              <button
                onClick={onViewTranslated}
                className="rounded border border-amber-300 bg-white px-3 py-1.5 text-xs text-amber-700 hover:bg-amber-100"
                data-testid="quota-view-translated-btn"
              >
                查看已译部分
              </button>
            ) : null}
            <button
              onClick={onDismiss}
              className="rounded px-2 py-1.5 text-xs text-amber-600 hover:text-amber-800"
              data-testid="quota-dismiss-btn"
            >
              知道了
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
