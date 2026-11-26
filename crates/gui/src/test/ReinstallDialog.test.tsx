import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import ReinstallDialog from '../components/ReinstallDialog';
import { initTestI18n } from './i18n';

// listen mock 收集回调由测试侧手动触发(同 App.test.tsx 手法);startInstall 单独
// mock 便于按用例编排成败(resolve / already in progress / 其他 rejection)
const { startInstallMock, listeners } = vi.hoisted(() => ({
  startInstallMock: vi.fn<() => Promise<void>>(),
  listeners: new Map<string, Array<(e: { payload: unknown }) => void>>(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, cb: (e: { payload: unknown }) => void) => {
    const arr = listeners.get(event) ?? [];
    arr.push(cb);
    listeners.set(event, arr);
    return Promise.resolve(() => {
      const list = listeners.get(event);
      if (!list) return;
      const i = list.indexOf(cb);
      if (i >= 0) list.splice(i, 1);
    });
  }),
}));

vi.mock('../api/client', () => ({ api: { startInstall: startInstallMock } }));

function emit(event: string, payload: unknown) {
  act(() => {
    for (const cb of listeners.get(event) ?? []) cb({ payload });
  });
}

beforeAll(async () => {
  await initTestI18n('zh');
});

beforeEach(() => {
  listeners.clear();
  startInstallMock.mockReset();
  startInstallMock.mockResolvedValue(undefined);
});

function renderDialog(overrides: { onClose?: () => void; onDone?: () => void } = {}) {
  const onClose = overrides.onClose ?? vi.fn();
  const onDone = overrides.onDone ?? vi.fn();
  const view = render(
    <ReinstallDialog open embeddedDaemon="0.7.0" installed onClose={onClose} onDone={onDone} />,
  );
  return { view, onClose, onDone };
}

/// fake timers 标配:推进入场帧(ui-x/Dialog 的 16ms entered)后冲刷 listen 注册
/// 微任务,事件派发才有 handler 可调
async function setup(overrides: { onClose?: () => void; onDone?: () => void } = {}) {
  vi.useFakeTimers();
  const r = renderDialog(overrides);
  act(() => {
    vi.advanceTimersByTime(16);
  });
  await act(async () => {});
  return r;
}

function confirm() {
  fireEvent.click(screen.getByTestId('reinstall-confirm'));
}

describe('ReinstallDialog', () => {
  it('初始为确认态:覆盖安装说明与确定/取消两钮,尚未调 startInstall', async () => {
    await setup();
    expect(screen.getByTestId('reinstall-dialog')).toBeInTheDocument();
    expect(screen.getByText(/覆盖安装 daemon 与 CLI/)).toBeInTheDocument();
    expect(screen.getByText(/v0\.7\.0/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '确定' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument();
    expect(startInstallMock).not.toHaveBeenCalled();
  });

  it('点确定进入等待态:确认钮消失,出现安装提示,startInstall 调一次', async () => {
    await setup();
    confirm();
    expect(screen.queryByRole('button', { name: '确定' })).not.toBeInTheDocument();
    expect(screen.getByTestId('reinstall-waiting')).toBeInTheDocument();
    expect(screen.getByText('正在安装，请留意系统提权弹窗…')).toBeInTheDocument();
    expect(startInstallMock).toHaveBeenCalledTimes(1);
  });

  it('install-done → 成功态,onDone 即时回调,1.5s 后自动 onClose', async () => {
    const { onClose, onDone } = await setup();
    confirm();
    emit('install-done', undefined);
    expect(screen.getByTestId('reinstall-done')).toBeInTheDocument();
    expect(screen.getByText(/安装完成/)).toBeInTheDocument();
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1_500);
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    // 成功后等待兜底计时已清,再推进不产生二次回调
    act(() => {
      vi.advanceTimersByTime(180_000);
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('install-failed(daemon-timeout) → failTimeout 文案;重试回等待态并再次 startInstall', async () => {
    await setup();
    confirm();
    emit('install-failed', 'daemon-timeout');
    expect(screen.getByTestId('reinstall-failed')).toBeInTheDocument();
    expect(screen.getByText('安装已完成但服务未在预期时间内上线，请查看日志后重试。')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '重试' }));
    expect(screen.getByTestId('reinstall-waiting')).toBeInTheDocument();
    expect(startInstallMock).toHaveBeenCalledTimes(2);
  });

  it('提权前 rejection → failGeneric + 原始原因,等待兜底计时停止', async () => {
    startInstallMock.mockRejectedValue('copy staging: ENOSPC');
    const { onClose } = await setup();
    confirm();
    await act(async () => {}); // 冲刷 rejection 微任务
    expect(screen.getByTestId('reinstall-failed')).toBeInTheDocument();
    expect(screen.getByText('安装启动失败')).toBeInTheDocument();
    expect(screen.getByText('copy staging: ENOSPC')).toBeInTheDocument();
    // 失败态不再有等待兜底计时:推进 180s 无转场
    act(() => {
      vi.advanceTimersByTime(180_000);
    });
    expect(screen.getByTestId('reinstall-failed')).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('等待 180s 无事件 → 超时问询;继续等待重置计时,取消等待只调 onClose', async () => {
    const { onClose, onDone } = await setup();
    confirm();
    act(() => {
      vi.advanceTimersByTime(180_000);
    });
    expect(screen.getByTestId('reinstall-timeout')).toBeInTheDocument();
    expect(screen.getByText('安装耗时异常，可能仍在后台进行。')).toBeInTheDocument();
    // 继续等待:兜底计时从头再来,提前 1ms 不得转超时
    fireEvent.click(screen.getByRole('button', { name: '继续等待' }));
    expect(screen.getByTestId('reinstall-waiting')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(179_999);
    });
    expect(screen.queryByTestId('reinstall-timeout')).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByTestId('reinstall-timeout')).toBeInTheDocument();
    // 取消等待 = 仅停止等待(后台安装不受影响),不视作失败
    fireEvent.click(screen.getByRole('button', { name: '取消等待' }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onDone).not.toHaveBeenCalled();
  });

  it('startInstall reject already in progress → 保持等待态,兜底计时不受影响', async () => {
    startInstallMock.mockRejectedValue('install already in progress');
    await setup();
    confirm();
    await act(async () => {});
    expect(screen.getByTestId('reinstall-waiting')).toBeInTheDocument();
    expect(screen.queryByTestId('reinstall-failed')).not.toBeInTheDocument();
    // 计时未被 rejection 打断:满 180s 照常转超时问询
    act(() => {
      vi.advanceTimersByTime(180_000);
    });
    expect(screen.getByTestId('reinstall-timeout')).toBeInTheDocument();
  });

  it('卸载后清定时器并解除事件监听:推进 180s 无回调无报错', async () => {
    const { view, onClose, onDone } = await setup();
    confirm();
    act(() => {
      view.unmount();
    });
    await act(async () => {}); // 冲刷 unlisten 微任务
    act(() => {
      vi.advanceTimersByTime(180_000);
    });
    expect(onClose).not.toHaveBeenCalled();
    expect(onDone).not.toHaveBeenCalled();
    for (const event of ['install-progress', 'install-done', 'install-failed']) {
      expect(listeners.get(event), event).toHaveLength(0);
    }
  });
});
