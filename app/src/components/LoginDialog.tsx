import { useEffect, useRef, useState } from 'react';
import { login } from '../services/auth';
import { BackendError } from '../services/backend';

interface Props {
  /** 打开时预填的提示（如 429 限流文案） */
  hint?: string | null;
  onClose: () => void;
  onLoggedIn: (username: string) => void;
}

/** 登录对话框：预设用户登录（无自助注册）。Esc 关闭，Enter 提交。 */
export default function LoginDialog({ hint, onClose, onLoggedIn }: Props) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const userRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    userRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (submitting || !username.trim() || !password) return;
    setSubmitting(true);
    setError(null);
    try {
      const me = await login(username.trim(), password);
      onLoggedIn(me.username ?? username.trim());
    } catch (err) {
      setError(err instanceof BackendError || err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40"
      data-testid="login-overlay"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <form
        onSubmit={submit}
        className="w-[22rem] rounded-lg bg-white p-5 shadow-xl"
        data-testid="login-dialog"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-bold text-slate-800">登录 geminus</h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 text-slate-400 hover:bg-slate-100 hover:text-slate-600"
            aria-label="关闭"
            data-testid="login-close"
          >
            ✕
          </button>
        </div>

        {hint ? (
          <p className="mb-3 rounded border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700" data-testid="login-hint">
            {hint}
          </p>
        ) : null}

        <label className="mb-1 block text-xs text-slate-500" htmlFor="login-username">
          用户名
        </label>
        <input
          id="login-username"
          ref={userRef}
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          autoComplete="username"
          className="mb-3 w-full rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-blue-400 focus:outline-none"
          data-testid="login-username"
        />

        <label className="mb-1 block text-xs text-slate-500" htmlFor="login-password">
          密码
        </label>
        <input
          id="login-password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="current-password"
          className="mb-3 w-full rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-blue-400 focus:outline-none"
          data-testid="login-password"
        />

        {error ? (
          <p className="mb-3 text-xs text-red-500" data-testid="login-error">
            {error}
          </p>
        ) : null}

        <button
          type="submit"
          disabled={submitting || !username.trim() || !password}
          className="w-full rounded bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-40"
          data-testid="login-submit"
        >
          {submitting ? '登录中…' : '登录'}
        </button>

        <p className="mt-3 text-center text-[11px] text-slate-400">
          本站不开放注册，仅限预先设置的账号登录
        </p>
      </form>
    </div>
  );
}
