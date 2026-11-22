import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DrawerShell from '../components/DrawerShell';

/// jsdom 不跑 CSS transition,开合动画的挂载/卸载时序由定时器承载,
/// 用 fake timers 精确推进 16ms(进)/250ms(出)两个节点
beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('DrawerShell', () => {
  it('open=true 立即挂载内容,16ms 后加 in 类触发滑入;遮罩/抽屉均挂 body(portal)', () => {
    render(
      <DrawerShell open onClose={() => {}} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(screen.getByTestId('shell')).toBeInTheDocument();
    expect(screen.getByText('抽屉内容')).toBeInTheDocument();
    expect(screen.getByTestId('shell')).not.toHaveClass('in');
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(screen.getByTestId('shell')).toHaveClass('in');
    expect(screen.getByTestId('shell').closest('body')).not.toBeNull();
    expect(document.querySelector('.drawer-mask')).not.toBeNull();
  });

  it('open=false 推进 250ms 后卸载(关闭动画期内容保留)', () => {
    const { rerender } = render(
      <DrawerShell open onClose={() => {}} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    rerender(
      <DrawerShell open={false} onClose={() => {}} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(screen.getByTestId('shell')).toBeInTheDocument();
    expect(screen.getByTestId('shell')).not.toHaveClass('in');
    act(() => {
      vi.advanceTimersByTime(249);
    });
    expect(screen.getByTestId('shell')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByTestId('shell')).not.toBeInTheDocument();
    expect(document.querySelector('.drawer-mask')).toBeNull();
  });

  it('初始 open=false 不渲染任何 DOM', () => {
    const { container } = render(
      <DrawerShell open={false} onClose={() => {}}>
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(container).toBeEmptyDOMElement();
    expect(document.querySelector('.drawer-mask')).toBeNull();
  });

  it('点遮罩触发 onClose(onClose 由壳内事件直触发,卸载与否由 open 控制)', () => {
    const onClose = vi.fn();
    render(
      <DrawerShell open onClose={onClose} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    fireEvent.click(document.querySelector('.drawer-mask')!);
    expect(onClose).toHaveBeenCalledTimes(1);
    // open 未翻转时不自行卸载
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId('shell')).toBeInTheDocument();
  });

  it('wide 加宽档,width 内联覆盖默认宽', () => {
    const { rerender } = render(
      <DrawerShell open onClose={() => {}} testId="shell" wide>
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(screen.getByTestId('shell')).toHaveClass('wide');
    rerender(
      <DrawerShell open onClose={() => {}} testId="shell" width={320}>
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(screen.getByTestId('shell')).not.toHaveClass('wide');
    expect(screen.getByTestId('shell')).toHaveStyle({ width: '320px' });
  });
});
