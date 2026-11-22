import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
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

describe('App 单页布局', () => {
  it('在线时渲染顶栏 logo 与产品名，不渲染侧边导航', async () => {
    renderApp();
    expect(await screen.findByText('daemon 在线')).toBeInTheDocument();
    expect(screen.getByAltText('logo')).toBeInTheDocument();
    expect(screen.getByText('Planet Port')).toBeInTheDocument();
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument();
  });

  it('空态：无行星时显示导入引导空态', async () => {
    renderApp();
    expect(await screen.findByText(/导入/)).toBeInTheDocument();
  });

  it('navigate 事件 focusJoin 打开添加网络抽屉（与顶栏同一入口）', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    emit('navigate', { focusJoin: true });
    expect(await screen.findByTestId('add-network-drawer')).toBeInTheDocument();
  });

  it('顶栏三按钮:添加网络/添加行星/设置各开对应抽屉,同一时刻仅一个', async () => {
    renderApp();
    fireEvent.click(await screen.findByText('添加网络'));
    expect(screen.getByTestId('add-network-drawer')).toBeInTheDocument();
    fireEvent.click(screen.getByText('添加行星'));
    expect(screen.queryByTestId('add-network-drawer')).not.toBeInTheDocument();
    expect(screen.getByTestId('add-planet-drawer')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('设置'));
    expect(screen.queryByTestId('add-planet-drawer')).not.toBeInTheDocument();
    expect(screen.getByTestId('settings-drawer')).toBeInTheDocument();
  });

  it('daemon-offline 事件切换离线引导', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    emit('daemon-offline', undefined);
    expect(await screen.findByText('daemon 已安装但未运行')).toBeInTheDocument();
  });

  it('tray-error 弹错误 toast', async () => {
    renderApp();
    await screen.findByText('daemon 在线');
    emit('tray-error', '托盘动作失败：network_not_found');
    expect(await screen.findByText('托盘动作失败：network_not_found')).toHaveClass('error');
  });

  it('卸载后解除全部事件监听', async () => {
    const { unmount } = renderApp();
    await screen.findByText('daemon 在线');
    unmount();
    await act(async () => {});
    for (const event of ['daemon-online', 'daemon-offline', 'navigate', 'tray-error']) {
      expect(listeners.get(event), event).toHaveLength(0);
    }
  });
});
