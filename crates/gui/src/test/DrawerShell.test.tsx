import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DrawerShell from '../components/DrawerShell';
import { initTestI18n } from './i18n';

/// 抽屉壳 DOM 锚点(Radix Dialog 原语 + ui-x/Sheet 封装层):
/// 遮罩 data-slot=sheet-overlay、抽屉 aside[data-slot=sheet-content]
const mask = () => document.querySelector('[data-slot="sheet-overlay"]');
const surface = () => document.querySelector('[data-slot="sheet-content"]');

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

/// Radix 关闭走「pointerdown 捕获 → click → 延迟派发 pointerDownOutside」
/// 链路(deferPointerDownOutside),遮罩点击须三步走:按下、点击、推进 0ms 定时。
/// 交互前先推进 16ms:进入帧内 Radix 受控 open(entered)尚未翻转,关闭链路
/// 全部不可达;真实浏览器该窗口内 content pointer-events:none 不可点,窗口对齐
function enterThen(fn: () => void) {
  act(() => {
    vi.advanceTimersByTime(16);
  });
  fn();
}

function clickMask() {
  enterThen(() => {
    fireEvent.pointerDown(mask()!);
    fireEvent.click(mask()!);
    act(() => {
      vi.advanceTimersByTime(1);
    });
  });
}

describe('DrawerShell', () => {
  it('open=true 立即挂载内容,16ms 后 data-state 翻 open 触发滑入;遮罩/抽屉均挂 body(portal)', () => {
    render(
      <DrawerShell open onClose={() => {}} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(screen.getByTestId('shell')).toBeInTheDocument();
    expect(screen.getByText('抽屉内容')).toBeInTheDocument();
    // 挂载即入树但仍处于 closed 态(全量右移),双帧后翻 open 触发过渡
    expect(screen.getByTestId('shell')).toHaveAttribute('data-state', 'closed');
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(screen.getByTestId('shell')).toHaveAttribute('data-state', 'open');
    expect(screen.getByTestId('shell').closest('body')).not.toBeNull();
    expect(mask()).not.toBeNull();
    // Radix modal 语义:抽屉以 role=dialog 呈现
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('遮罩随进入态挂 data-state:漏挂则 data-[state=open]:opacity-100 永不生效,遮罩全程透明不可见', () => {
    const { rerender } = render(
      <DrawerShell open onClose={() => {}} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    // 挂载帧 closed(淡入前),16ms 后翻 open 淡入
    expect(mask()).toHaveAttribute('data-state', 'closed');
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(mask()).toHaveAttribute('data-state', 'open');
    // 关闭期回 closed:遮罩淡出且关闭动画期禁点(data-[state=closed]:pointer-events-none)
    rerender(
      <DrawerShell open={false} onClose={() => {}} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    expect(mask()).toHaveAttribute('data-state', 'closed');
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
    // open 撤销即翻 closed(滑出),但 forceMount 下内容在动画期保持挂载
    expect(screen.getByTestId('shell')).toHaveAttribute('data-state', 'closed');
    act(() => {
      vi.advanceTimersByTime(249);
    });
    expect(screen.getByTestId('shell')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByTestId('shell')).not.toBeInTheDocument();
    expect(mask()).toBeNull();
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
    expect(mask()).toBeNull();
  });

  it('点遮罩触发 onClose(onClose 由壳内事件直触发,卸载与否由 open 控制)', () => {
    const onClose = vi.fn();
    render(
      <DrawerShell open onClose={onClose} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    clickMask();
    expect(onClose).toHaveBeenCalledTimes(1);
    // open 未翻转时不自行卸载
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId('shell')).toBeInTheDocument();
  });

  it('Esc 触发 onClose(Radix dismissable 语义,基线新增的可访问性能力)', () => {
    const onClose = vi.fn();
    render(
      <DrawerShell open onClose={onClose} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    enterThen(() => {
      fireEvent.keyDown(surface()!, { key: 'Escape' });
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('右上角统一渲染关闭按钮,点击触发 onClose', () => {
    const onClose = vi.fn();
    render(
      <DrawerShell open onClose={onClose} testId="shell">
        <p>抽屉内容</p>
      </DrawerShell>,
    );
    enterThen(() => {
      fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('多层抽屉反向取号 --stack:最新层 0 贴右,底层 1 往里推;--layer 挂载序居上', async () => {
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
    expect(screen.getByTestId('first').style.getPropertyValue('--stack')).toBe('1');
    expect(screen.getByTestId('second').style.getPropertyValue('--stack')).toBe('0');
    expect(screen.getByTestId('first').style.getPropertyValue('--layer')).toBe('0');
    expect(screen.getByTestId('second').style.getPropertyValue('--layer')).toBe('1');
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
    expect(screen.getByTestId('second').style.getPropertyValue('--stack')).toBe('0');
    // 关闭第二层:250ms 动画期仍占位,卸载后第一层从 1 回落到 0(贴右)
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
