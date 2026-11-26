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
let daemonState: { online: boolean; installed: boolean; daemonVersion: string | null };

/// 一行星一网络的最小页面数据:供离线常驻断言(列表区不消失)使用
const planet = {
  id: 'p1', name: '默认行星', icon: '', color: '#0ea5e9', type: 'official', status: 'running',
  enabled: true, online: true, nodeId: null, coreVersion: null, latencyMs: null,
  networkCount: 1, port: 9993, transport: null, tcpFallbackRelay: null,
};
const network = {
  planet: 'p1', nwid: '9536600adcf2b350', name: '测试网', status: 'OK', conflict: null,
  type: 'PUBLIC', mac: 'aa:bb:cc:dd:ee:ff', interface: 'feth0', mtu: 2800,
  assignedAddrs: ['10.0.0.1'], managedRoutes: [], dnsServers: [],
  routeViaZeroTier: false, routeViaLocked: false, dnsMode: 'none', customDns: [],
};

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
  daemonState = { online: true, installed: true, daemonVersion: null };
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
    // 'daemon 在线' 出现两处:顶栏在线态 + 底部工具带本机卡
    expect((await screen.findAllByText('daemon 在线')).length).toBeGreaterThan(0);
    expect(screen.getByAltText('logo')).toBeInTheDocument();
    expect(screen.getByText('Planet Port')).toBeInTheDocument();
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument();
  });

  it('空态：无行星时显示加入行星引导空态', async () => {
    renderApp();
    expect(await screen.findByText(/加入行星/)).toBeInTheDocument();
  });

  it('navigate 事件 focusJoin 打开添加网络抽屉（与托盘同一入口）', async () => {
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('navigate', { focusJoin: true });
    expect(await screen.findByTestId('add-network-drawer')).toBeInTheDocument();
  });

  it('顶栏按钮:添加行星/设置各开对应抽屉,同一时刻仅一个(添加网络入口在首页空卡)', async () => {
    renderApp();
    const addBtn = await screen.findByTestId('topbar-add-planet');
    expect(addBtn).not.toBeDisabled();
    fireEvent.click(addBtn);
    expect(screen.getByTestId('add-planet-drawer')).toBeInTheDocument();
    expect(screen.getByTestId('topbar-settings')).not.toBeDisabled();
    fireEvent.click(screen.getByLabelText('设置'));
    expect(screen.queryByTestId('add-planet-drawer')).not.toBeInTheDocument();
    expect(screen.getByTestId('settings-drawer')).toBeInTheDocument();
  });

  it('daemon-offline 后首页常驻:网络列表仍在、控制组件禁用,设置仍可开抽屉', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case 'get_daemon_state':
          return Promise.resolve(daemonState);
        case 'get_status':
          return Promise.resolve({ planets: [planet] });
        case 'get_networks':
          return Promise.resolve({ networks: [network] });
        default:
          return Promise.resolve(null);
      }
    });
    renderApp();
    expect(await screen.findByTestId('network-card')).toBeInTheDocument();
    emit('daemon-offline', undefined);
    // 整页替换改为常驻:网络列表区不消失,卡片开关禁用(Radix disabled)
    expect(screen.getByTestId('network-card')).toBeInTheDocument();
    expect(screen.getByRole('switch')).toBeDisabled();
    expect(screen.getByTestId('topbar-add-planet')).toBeDisabled();
    // 设置常显:离线仍可打开抽屉(安装能力在内)
    fireEvent.click(screen.getByTestId('topbar-settings'));
    expect(await screen.findByTestId('settings-drawer')).toBeInTheDocument();
  });

  it('tray-error 弹错误 toast', async () => {
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('tray-error', '托盘动作失败：network_not_found');
    expect(await screen.findByText('托盘动作失败：network_not_found')).toHaveClass('error');
  });

  it('卸载后解除全部事件监听', async () => {
    const { unmount } = renderApp();
    await screen.findAllByText('daemon 在线');
    unmount();
    await act(async () => {});
    for (const event of ['daemon-online', 'daemon-offline', 'navigate', 'tray-error']) {
      expect(listeners.get(event), event).toHaveLength(0);
    }
  });
});
