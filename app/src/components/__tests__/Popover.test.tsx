import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import Popover from '../Popover';

afterEach(cleanup);

describe('Popover 原语', () => {
  it('点击触发器开面板；再点关闭', () => {
    render(
      <Popover
        panelTestId="p1"
        trigger={({ toggle }) => <button onClick={toggle} data-testid="trigger">触发</button>}
        panel={(close) => <button onClick={close} data-testid="act">动作</button>}
      />,
    );
    expect(screen.queryByTestId('p1')).toBeNull();
    fireEvent.click(screen.getByTestId('trigger'));
    expect(screen.getByTestId('p1')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('trigger'));
    expect(screen.queryByTestId('p1')).toBeNull();
  });

  it('面板动作 close() 关闭面板', () => {
    render(
      <Popover
        panelTestId="p2"
        trigger={({ toggle }) => <button onClick={toggle}>t</button>}
        panel={(close) => <button onClick={close} data-testid="act">x</button>}
      />,
    );
    fireEvent.click(screen.getByText('t'));
    fireEvent.click(screen.getByTestId('act'));
    expect(screen.queryByTestId('p2')).toBeNull();
  });

  it('Esc 关闭；点击面板外部关闭', () => {
    render(
      <Popover
        panelTestId="p3"
        trigger={({ toggle }) => <button onClick={toggle}>t</button>}
        panel={() => <span>content</span>}
      />,
    );
    fireEvent.click(screen.getByText('t'));
    expect(screen.getByTestId('p3')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('p3')).toBeNull();
    // 外部点击
    fireEvent.click(screen.getByText('t'));
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId('p3')).toBeNull();
  });
});
