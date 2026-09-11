import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import LoginDialog from '../LoginDialog';
import { login } from '../../services/auth';
import { BackendError } from '../../services/backend';

vi.mock('../../services/auth', () => ({
  login: vi.fn(),
}));

describe('LoginDialog（部署安全加固）', () => {
  it('展示 hint 并渲染表单', () => {
    render(
      <LoginDialog hint="未登录用户最多查看并翻译 5 页" onClose={() => {}} onLoggedIn={() => {}} />,
    );
    expect(screen.getByTestId('login-hint')).toHaveTextContent('未登录用户最多');
    expect(screen.getByTestId('login-username')).toBeInTheDocument();
    expect(screen.getByTestId('login-password')).toBeInTheDocument();
  });

  it('登录成功 → onLoggedIn 回调携带用户名', async () => {
    vi.mocked(login).mockResolvedValue({ authenticated: true, username: 'alice' });
    const onLoggedIn = vi.fn();
    render(<LoginDialog onClose={() => {}} onLoggedIn={onLoggedIn} />);

    fireEvent.change(screen.getByTestId('login-username'), { target: { value: 'alice' } });
    fireEvent.change(screen.getByTestId('login-password'), { target: { value: 'pw' } });
    fireEvent.click(screen.getByTestId('login-submit'));

    await waitFor(() => expect(onLoggedIn).toHaveBeenCalledWith('alice'));
  });

  it('登录失败 → 显示服务端错误文案', async () => {
    vi.mocked(login).mockRejectedValue(new BackendError('用户名或密码错误', 401));
    render(<LoginDialog onClose={() => {}} onLoggedIn={() => {}} />);

    fireEvent.change(screen.getByTestId('login-username'), { target: { value: 'alice' } });
    fireEvent.change(screen.getByTestId('login-password'), { target: { value: 'bad' } });
    fireEvent.click(screen.getByTestId('login-submit'));

    await waitFor(() => {
      expect(screen.getByTestId('login-error')).toHaveTextContent('用户名或密码错误');
    });
  });

  it('关闭按钮触发 onClose', () => {
    const onClose = vi.fn();
    render(<LoginDialog onClose={onClose} onLoggedIn={() => {}} />);
    fireEvent.click(screen.getByTestId('login-close'));
    expect(onClose).toHaveBeenCalled();
  });
});
