import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DrawerShell from '../components/DrawerShell';
import { initTestI18n } from './i18n';

beforeAll(async () => {
  await initTestI18n('zh');
});

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

  it('onClosed 在 250ms 卸载时回调一次(open=true 全程不触发)', () => {
    const onClosed = vi.fn();
    const { rerender } = render(
      <DrawerShell open onClose={() => {}} onClosed={onClosed} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(onClosed).not.toHaveBeenCalled();
    rerender(
      <DrawerShell open={false} onClose={() => {}} onClosed={onClosed} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    act(() => {
      vi.advanceTimersByTime(249);
    });
    expect(onClosed).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onClosed).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId('shell')).not.toBeInTheDocument();
  });

  it('父层每渲染换新 onClosed 引用不重置卸载定时,且只回调最新闭包', () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = render(
      <DrawerShell open onClose={() => {}} onClosed={first} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    rerender(
      <DrawerShell open={false} onClose={() => {}} onClosed={second} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    // 关闭动画期父层可能再渲染(内联箭头换新引用),不得重开计时
    rerender(
      <DrawerShell open={false} onClose={() => {}} onClosed={second} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    act(() => {
      vi.advanceTimersByTime(250);
    });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
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

  it('右上角统一渲染关闭按钮,点击触发 onClose', () => {
    const onClose = vi.fn();
    render(
      <DrawerShell open onClose={onClose} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('多层抽屉按挂载序取层号 --stack:首个 0,第二个 1(右偏层叠)', async () => {
    render(
      <>
        <DrawerShell open onClose={() => {}} testId="first">
          <p>第一层</p>
        </DrawerShell>
        <DrawerShell open onClose={() => {}} testId="second">
          <p>第二层</p>
        </DrawerShell>
      </>,
    );
    // MutationObserver 回调在 microtask 后触发,flush 后层号落位
    await act(async () => {});
    expect(screen.getByTestId('first').style.getPropertyValue('--stack')).toBe('0');
    expect(screen.getByTestId('second').style.getPropertyValue('--stack')).toBe('1');
  });

  it('上层抽屉卸载后,下层层号回落(层叠重排)', async () => {
    const { rerender } = render(
      <>
        <DrawerShell open onClose={() => {}} testId="first">
          <p>第一层</p>
        </DrawerShell>
        <DrawerShell open onClose={() => {}} testId="second">
          <p>第二层</p>
        </DrawerShell>
      </>,
    );
    await act(async () => {});
    expect(screen.getByTestId('second').style.getPropertyValue('--stack')).toBe('1');
    // 关闭第二层:250ms 动画期仍占位,卸载后第一层回落 0
    rerender(
      <>
        <DrawerShell open onClose={() => {}} testId="first">
          <p>第一层</p>
        </DrawerShell>
        <DrawerShell open={false} onClose={() => {}} testId="second">
          <p>第二层</p>
        </DrawerShell>
      </>,
    );
    act(() => {
      vi.advanceTimersByTime(250);
    });
    await act(async () => {});
    expect(screen.queryByTestId('second')).not.toBeInTheDocument();
    expect(screen.getByTestId('first').style.getPropertyValue('--stack')).toBe('0');
  });
});
