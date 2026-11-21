import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { act } from 'react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

// listen mock 把回调收集进 listeners，由测试侧手动触发事件
const { invokeMock, listeners } = vi.hoisted(() => ({
  invokeMock: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
  listeners: new Map<string, Array<(e: { payload: unknown }) => void>>(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

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

/// get_daemon_state 的返回由用例按需指定；其余命令给页面级查询兜底数据
let daemonState: { online: boolean; installed: boolean };

function emit(event: string, payload: unknown) {
  act(() => {
    for (const cb of listeners.get(event) ?? []) cb({ payload });
  });
}

function renderApp() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <App />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

beforeAll(async () => {
  await initTestI18n('zh');
});

beforeEach(() => {
  listeners.clear();
  daemonState = { online: true, installed: true };
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    switch (cmd) {
      case 'get_daemon_state':
        return Promise.resolve(daemonState);
      case 'get_status':
        return Promise.resolve({ planets: [] });
      case 'get_networks':
        return Promise.resolve({ networks: [] });
      default:
        return Promise.resolve(null);
    }
  });
});

const invokedCommands = () => invokeMock.mock.calls.map(c => c[0]);

describe('App', () => {
  it('首屏注册全部四类事件监听并拉取 daemon 状态', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    expect([...listeners.keys()].sort()).toEqual(['daemon-offline', 'daemon-online', 'navigate', 'tray-error']);
    expect(invokedCommands()).toContain('get_daemon_state');
  });

  it('daemonState 为 online 时不渲染离线引导，进入概览空态', async () => {
    renderApp();
    expect(await screen.findByText('daemon 在线')).toBeInTheDocument();
    expect(screen.queryByText(/已安装但未运行|未检测到 planet-port daemon/)).not.toBeInTheDocument();
    expect(await screen.findByText('还没有 planet。到「Planet」页导入一个。')).toBeInTheDocument();
  });

  it('daemon-offline 事件触发后切换到离线引导页', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    emit('daemon-offline', undefined);
    expect(await screen.findByText('daemon 已安装但未运行')).toBeInTheDocument();
    expect(screen.queryByText('daemon 在线')).not.toBeInTheDocument();
  });

  it('navigate 事件 {to:networks, focusJoin:true} 切页并聚焦加入输入框', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    emit('navigate', { to: 'networks', focusJoin: true });
    const input = await screen.findByPlaceholderText('网络 ID，如 9536600adcf2b350');
    expect(input).toHaveFocus();
    expect(screen.getByRole('button', { name: '网络' })).toHaveClass('active');
    expect(screen.getByRole('button', { name: '概览' })).not.toHaveClass('active');
  });

  it('tray-error 事件以错误 toast 弹出 payload 文案', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    emit('tray-error', '托盘动作失败：network_not_found');
    expect(await screen.findByText('托盘动作失败：network_not_found')).toHaveClass('error');
  });

  it('卸载后解除全部事件监听（unlisten 真正执行，不泄漏）', async () => {
    const { unmount } = renderApp();
    await screen.findByText('daemon 在线');
    expect(listeners.get('navigate')).toHaveLength(1);
    unmount();
    await act(async () => {}); // unlisten 经 Promise 返回，等微任务排空
    for (const event of ['daemon-online', 'daemon-offline', 'navigate', 'tray-error']) {
      expect(listeners.get(event), event).toHaveLength(0);
    }
  });
});
