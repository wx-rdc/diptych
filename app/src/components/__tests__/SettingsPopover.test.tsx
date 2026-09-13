import { afterEach, describe, expect, it, vi } from 'vitest';
import { useState } from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import SettingsPopover from '../toolbar/SettingsPopover';

afterEach(cleanup);

const base = {
  fontSize: 18,
  onFontIncrease: vi.fn(),
  onFontDecrease: vi.fn(),
  sourceLang: 'en',
  targetLang: 'zh',
  onSourceLangChange: vi.fn(),
  onTargetLangChange: vi.fn(),
  devPath: '',
  onDevPathChange: vi.fn(),
  onParseLocal: vi.fn(),
};

function open() {
  render(<SettingsPopover {...base} />);
  fireEvent.click(screen.getByTestId('settings-btn'));
}

describe('SettingsPopover（字号/语言对/开发路径收纳）', () => {
  it('点击 ⚙ 展开面板：字号控件 + 语言对 + 提示', () => {
    open();
    expect(screen.getByTestId('settings-popover')).toBeInTheDocument();
    expect(screen.getByTestId('font-size-control')).toBeInTheDocument();
    expect(screen.getByTestId('font-size-value').textContent).toBe('18');
    expect(screen.getByLabelText('源语言')).toBeInTheDocument();
    expect(screen.getByLabelText('目标语言')).toBeInTheDocument();
    expect(screen.getByTestId('settings-popover').textContent).toContain('更改语言对');
  });

  it('字号增减回调', () => {
    open();
    fireEvent.click(screen.getByTestId('font-size-inc'));
    expect(base.onFontIncrease).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('font-size-dec'));
    expect(base.onFontDecrease).toHaveBeenCalledOnce();
  });

  it('语言对变更回调', () => {
    open();
    fireEvent.change(screen.getByLabelText('源语言'), { target: { value: 'fr' } });
    expect(base.onSourceLangChange).toHaveBeenCalledWith('fr');
    fireEvent.change(screen.getByLabelText('目标语言'), { target: { value: 'ja' } });
    expect(base.onTargetLangChange).toHaveBeenCalledWith('ja');
  });

  it('开发小节：本地路径输入 Enter 触发 onParseLocal', () => {
    // 受控输入需真实状态流（mock onChange 不重渲染，React 会回滚 DOM 值）
    function Host() {
      const [devPath, setDevPath] = useState('');
      return <SettingsPopover {...base} devPath={devPath} onDevPathChange={setDevPath} />;
    }
    render(<Host />);
    fireEvent.click(screen.getByTestId('settings-btn'));
    const input = screen.getByTestId('file-path-input');
    fireEvent.change(input, { target: { value: '/tmp/a.pdf' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(base.onParseLocal).toHaveBeenCalledOnce();
  });
});
