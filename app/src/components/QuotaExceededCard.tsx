import type { QuotaInfo } from '../services/backend';

interface Props {
  quota: QuotaInfo;
  /**
   * 卡片来源（v2.4.1 拆分——此前预检场景也显示「仅查看已译部分」，点击后无内容可看）：
   * - 'limit'：翻译中熔断（当前文档已有已译部分，真实可读可续译）
   * - 'precheck'：解析预检拦截（本次文档未打开，不存在「已译部分」）
   */
  variant: 'limit' | 'precheck';
  onLogin: () => void;
  /** 仅 limit：关闭卡片并展示已译部分（滚动到已译区域 / 打开游客记录面板） */
  onViewTranslated?: () => void;
  /** 关闭卡片（预检场景 = 「我知道了」；limit 场景同） */
  onDismiss: () => void;
  /** 仅 precheck：重新选择一个更小的文件 */
  onPickAnother?: () => void;
}

/**
 * v2.4 T6：游客额度引导卡片（软性引导，不阻断阅读）。
 * 文案随 variant 区分——precheck 不承诺「已译部分可读」（没有），limit 才承诺且可兑现。
 */
export default function QuotaExceededCard({
  quota,
  variant,
  onLogin,
  onViewTranslated,
  onDismiss,
  onPickAnother,
}: Props) {
  const remaining = Math.max(quota.limit - quota.used, 0);
  return (
    <div
      className="fixed bottom-16 left-1/2 z-40 w-[min(92vw, 30rem)] -translate-x-1/2 rounded-lg border border-amber-300 bg-amber-50 p-4 shadow-lg"
      data-testid="quota-exceeded-card"
      role="alert"
    >
      <div className="flex items-start gap-2">
        <span className="text-lg">🔒</span>
        <div className="flex-1">
          {variant === 'limit' ? (
            <>
              <p className="text-sm font-medium text-amber-800">游客翻译额度已用完</p>
              <p className="mt-1 text-xs text-amber-700">
                游客最多可翻译 {quota.limit} 页（已用 {quota.used} 页）。已翻译的部分仍可继续阅读，
                也可点「查看已译部分」回访；登录后不限量，并可继续翻译剩余内容（已译段落不重复计费）。
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                <button
                  onClick={onLogin}
                  className="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500"
                  data-testid="quota-login-btn"
                >
                  立即登录
                </button>
                {onViewTranslated ? (
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
            </>
          ) : (
            <>
              <p className="text-sm font-medium text-amber-800">超出游客额度，该文件未开始翻译</p>
              <p className="mt-1 text-xs text-amber-700">
                游客额度剩余 {remaining} 页，该文件需要 {quota.required} 页（额度 {quota.limit} 页已被之前的使用消耗）。
                登录后不限量；也可以换一个较小的文件在剩余额度内试用。
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                <button
                  onClick={onLogin}
                  className="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500"
                  data-testid="quota-login-btn"
                >
                  立即登录
                </button>
                {onPickAnother ? (
                  <button
                    onClick={onPickAnother}
                    className="rounded border border-amber-300 bg-white px-3 py-1.5 text-xs text-amber-700 hover:bg-amber-100"
                    data-testid="quota-pick-another-btn"
                  >
                    换个文件试试
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
            </>
          )}
        </div>
      </div>
    </div>
  );
}
