import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import { act } from 'react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { toast } from 'sonner';
import App from '../App';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';
import type { EmbeddedManifest } from '../api/types';

// listen mock 把回调收集进 listeners，由测试侧手动触发事件
const { invokeMock, listeners, getVersionMock } = vi.hoisted(() => ({
  invokeMock: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
  listeners: new Map<string, Array<(e: { payload: unknown }) => void>>(),
  getVersionMock: vi.fn<() => Promise<string | null>>(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: getVersionMock }));

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

/// toast 断言锚点:自绘内容根元素带 kind(ok/error)类,文案包在其 span 子元素内
/// (见 ui-x/Toaster 头注释)
async function findToast(text: string | RegExp, kind: 'ok' | 'error') {
  const el = await screen.findByText(text);
  expect(el.parentElement).toHaveClass(kind);
  return el;
}

/// 版本差异检测三探针定向 mock:其余命令沿用 beforeEach 的页面级兜底
function mockVersionProbes(manifest: EmbeddedManifest | null, installedDaemon: string, installedCli: string) {
  invokeMock.mockImplementation((cmd: string) => {
    switch (cmd) {
      case 'get_daemon_state':
        return Promise.resolve(daemonState);
      case 'get_status':
        return Promise.resolve({ planets: [] });
      case 'get_networks':
        return Promise.resolve({ networks: [] });
      case 'embedded_manifest':
        return Promise.resolve(manifest);
      case 'get_installed_daemon_version':
        return Promise.resolve(installedDaemon);
      case 'get_cli_version':
        return Promise.resolve(installedCli);
      default:
        return Promise.resolve(null);
    }
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
  // sonner 的 toast 队列挂在模块级 store,不随组件卸载清空:每例前清队,
  // 防上一例的 toast 文案/动作按钮混入下一例断言(visibleToasts=99 且 4s 才自动消失)
  act(() => {
    toast.dismiss();
  });
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
  getVersionMock.mockReset();
  getVersionMock.mockResolvedValue('0.6.1');
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
    await findToast('托盘动作失败：network_not_found', 'error');
  });

  it('install-done 兜底 toast 常驻 App 层:设置抽屉未开也回报', async () => {
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('install-done', undefined);
    await findToast('daemon 安装完成', 'ok');
  });

  it('install-failed 兜底 toast:枚举原因映射文案,表外原因走通用失败', async () => {
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('install-failed', 'install-exit-nonzero');
    await findToast('安装程序退出码非零，请查看日志后重试。', 'error');
    emit('install-failed', 'copy staging: ENOSPC');
    await findToast('安装启动失败', 'error');
  });

  it('卸载后解除全部事件监听', async () => {
    const { unmount } = renderApp();
    await screen.findAllByText('daemon 在线');
    unmount();
    await act(async () => {});
    for (const event of ['daemon-online', 'daemon-offline', 'navigate', 'tray-error', 'install-done', 'install-failed', 'check-update']) {
      expect(listeners.get(event), event).toHaveLength(0);
    }
  });
});

describe('启动版本差异探测(被动提示)', () => {
  const manifest: EmbeddedManifest = { daemon: '0.6.0', cli: '0.6.0', target: 'aarch64-apple-darwin' };

  /// 页面渲染 + 冲刷版本探测微任务,排除异步未落的假阴性
  async function renderAndFlush() {
    renderApp();
    await screen.findAllByText('daemon 在线');
    await act(async () => {});
  }

  it('已装旧于内嵌 → 状态栏右下角渲染「可升级」', async () => {
    mockVersionProbes(manifest, '0.5.1', '0.5.1');
    await renderAndFlush();
    expect(screen.getByTestId('statusbar-upgradable')).toHaveTextContent('可升级');
  });

  it('单侧差异:仅 daemon 旧于内嵌(cli 同版)→ 同样点亮', async () => {
    mockVersionProbes(manifest, '0.5.1', '0.6.0');
    await renderAndFlush();
    expect(screen.getByTestId('statusbar-upgradable')).toBeInTheDocument();
  });

  it('已装新于内嵌(compat)→ 不渲染(被动化后无风险提示)', async () => {
    mockVersionProbes(manifest, '0.6.1', '0.6.1');
    await renderAndFlush();
    expect(screen.queryByTestId('statusbar-upgradable')).not.toBeInTheDocument();
  });

  it('两侧同版 → 不渲染', async () => {
    mockVersionProbes(manifest, '0.6.0', '0.6.0');
    await renderAndFlush();
    expect(screen.queryByTestId('statusbar-upgradable')).not.toBeInTheDocument();
  });

  it('无内嵌 manifest(dev/未组包)→ 不渲染', async () => {
    await renderAndFlush(); // beforeEach 兜底:embedded_manifest 等三探针均返回 null
    expect(screen.queryByTestId('statusbar-upgradable')).not.toBeInTheDocument();
  });

  it('install-done 后状态栏可升级提示清除:装完磁盘必等于内嵌,置 false 恒正确', async () => {
    mockVersionProbes(manifest, '0.5.1', '0.5.1');
    await renderAndFlush();
    expect(screen.getByTestId('statusbar-upgradable')).toBeInTheDocument();
    emit('install-done', undefined);
    await act(async () => {});
    expect(screen.queryByTestId('statusbar-upgradable')).not.toBeInTheDocument();
  });
});

describe('GUI 更新检查(手动触发:托盘事件/关于区入口)', () => {
  /// check_gui_update 定向 mock:其余命令沿用 beforeEach 的页面级兜底
  function mockUpdateCheck(result: () => Promise<string>) {
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case 'get_daemon_state':
          return Promise.resolve(daemonState);
        case 'get_status':
          return Promise.resolve({ planets: [] });
        case 'get_networks':
          return Promise.resolve({ networks: [] });
        case 'check_gui_update':
          return result();
        default:
          return Promise.resolve(null);
      }
    });
  }

  it('托盘 check-update 事件:当前旧于最新 → found 弹窗(标题/正文含双版本)', async () => {
    mockUpdateCheck(() => Promise.resolve('0.7.0'));
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('check-update', undefined);
    expect(await screen.findByTestId('update-dialog')).toBeInTheDocument();
    expect(screen.getByText('发现新版本')).toBeInTheDocument();
    expect(screen.getByText('当前 v0.6.1，最新 v0.7.0。')).toBeInTheDocument();
  });

  it('查得同版 → latest 态(已是最新 + 当前版本)', async () => {
    mockUpdateCheck(() => Promise.resolve('0.6.1'));
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('check-update', undefined);
    expect(await screen.findByTestId('update-dialog')).toBeInTheDocument();
    expect(screen.getByText('已是最新')).toBeInTheDocument();
    expect(screen.getByText('当前版本 v0.6.1')).toBeInTheDocument();
  });

  it('检查失败 → failed 态呈现错误原文', async () => {
    mockUpdateCheck(() => Promise.reject('GitHub 限流 (429)'));
    renderApp();
    await screen.findAllByText('daemon 在线');
    emit('check-update', undefined);
    expect(await screen.findByTestId('update-dialog')).toBeInTheDocument();
    expect(screen.getByText('检查更新失败')).toBeInTheDocument();
    expect(screen.getByText('GitHub 限流 (429)')).toBeInTheDocument();
  });

  it('found 后重开设置抽屉:gui 行点亮升级箭头,检查更新行点击同样触发检查', async () => {
    mockUpdateCheck(() => Promise.resolve('0.7.0'));
    renderApp();
    await screen.findAllByText('daemon 在线');
    // 关于区入口:点检查更新行触发(与托盘事件同一 runUpdateCheck)
    fireEvent.click(screen.getByTestId('topbar-settings'));
    fireEvent.click(await screen.findByTestId('settings-check-update'));
    expect(await screen.findByTestId('update-dialog')).toBeInTheDocument();
    expect(screen.getByText('发现新版本')).toBeInTheDocument();
    // 关弹窗后 gui 行升级图标仍在(会话内状态,不随弹窗关闭消失)
    fireEvent.click(screen.getByRole('button', { name: '稍后' }));
    expect(screen.queryByTestId('update-dialog')).not.toBeInTheDocument();
    const marks = await screen.findAllByLabelText('可升级');
    expect(marks).toHaveLength(1);
    const row = marks[0].closest('div')?.textContent ?? '';
    expect(row).toContain('planet-port-gui');
    expect(row).not.toContain('planet-portd');
  });
});
