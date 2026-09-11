import type { QuotaInfo } from '../services/backend';

interface Props {
  quota: QuotaInfo;
  onLogin: () => void;
  onDismiss: () => void;
}

/**
 * v2.4 T6：匿名额度用尽引导卡片（熔断后底部浮层）。
 * 文案基调：软性引导——已翻译部分保持可读，登录解锁全文。
 */
export default function QuotaExceededCard({ quota, onLogin, onDismiss }: Props) {
  return (
    <div
      className="fixed bottom-16 left-1/2 z-40 w-[min(92vw, 30rem)] -translate-x-1/2 rounded-lg border border-amber-300 bg-amber-50 p-4 shadow-lg"
      data-testid="quota-exceeded-card"
      role="alert"
    >
      <div className="flex items-start gap-2">
        <span className="text-lg">🔒</span>
        <div className="flex-1">
          <p className="text-sm font-medium text-amber-800">游客翻译额度已用完</p>
          <p className="mt-1 text-xs text-amber-700">
            游客最多可翻译 {quota.limit} 页（已用 {quota.used} 页）。已翻译的部分仍可继续阅读；
            登录后不限量，并可继续翻译本文档剩余内容（已译段落不会重复计费）。
          </p>
          <div className="mt-3 flex gap-2">
            <button
              onClick={onLogin}
              className="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500"
              data-testid="quota-login-btn"
            >
              立即登录
            </button>
            <button
              onClick={onDismiss}
              className="rounded border border-amber-300 bg-white px-3 py-1.5 text-xs text-amber-700 hover:bg-amber-100"
              data-testid="quota-dismiss-btn"
            >
              仅查看已译部分
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
