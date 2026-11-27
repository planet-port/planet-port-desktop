import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsDrawer from '../components/SettingsDrawer';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

// install-done/failed 事件回调收集进 listeners 由测试侧触发(同 App.test.tsx 手法)
const { invokeMock, listeners } = vi.hoisted(() => ({
  invokeMock: vi.fn<(cmd: string) => Promise<unknown>>(),
  listeners: new Map<string, Array<(e: { payload: unknown }) => void>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn().mockResolvedValue('0.4.1') }));
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

function emit(event: string, payload: unknown) {
  act(() => {
    for (const cb of listeners.get(event) ?? []) cb({ payload });
  });
}

/// 抽屉壳 DOM 锚点(Radix Dialog 原语 + ui-x/Sheet 封装层)
const mask = () => document.querySelector('[data-slot="sheet-overlay"]');

/// matchMedia mock(theme 三态 apply 依赖 system 深色探测;jsdom 未实现,同 theme.test.ts 范式)
function mockMatchMedia(dark: boolean) {
  vi.stubGlobal('matchMedia', (q: string) => ({
    matches: q.includes('dark') ? dark : false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

const identity = {
  global: { exists: true, nodeId: '1792e3382e' },
  mode: 'shared',
  planets: [{ id: '1', name: 'gozi25', nodeId: '1792e3382e', running: true }],
};

beforeAll(async () => {
  await initTestI18n('zh');
});

beforeEach(() => {
  listeners.clear();
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) =>
    cmd === 'identity_get'
      ? Promise.resolve(identity)
      : cmd === 'get_cli_version'
        ? Promise.resolve('0.6.0')
        : Promise.resolve(null));
  localStorage.removeItem('theme');
  mockMatchMedia(false);
});

/// 底部安装区用例的 invoke 定向:内嵌 manifest(非 null 时安装区渲染)与
/// 已装 daemon 磁盘版本探测值;daemonInstalled 缺省不拦 get_daemon_state,
/// 探测落 null 按组件缺省已装口径,只有「未装」用例显式传 false
function mockManifest(
  manifest: { daemon: string; cli: string; target: string } | null,
  installedDaemon: string | null,
  daemonInstalled?: boolean,
) {
  invokeMock.mockImplementation((cmd: string) =>
    cmd === 'identity_get'
      ? Promise.resolve(identity)
      : cmd === 'get_cli_version'
        ? Promise.resolve('0.6.0')
        : cmd === 'embedded_manifest'
          ? Promise.resolve(manifest)
          : cmd === 'get_installed_daemon_version'
            ? Promise.resolve(installedDaemon)
            : cmd === 'get_daemon_state' && daemonInstalled !== undefined
              ? Promise.resolve({ online: false, installed: daemonInstalled, daemonVersion: null })
              : Promise.resolve(null));
}

// SettingsDrawer 自身无查询,但内嵌 IdentitySection 走 useQuery/useQueryClient,须挂 Provider。
// 组件为瞬时型(App 条件渲染挂载,内部自持 open),不再收 open prop。
// 第二参透传检查更新相关 props(guiUpdate/checking/onCheckUpdate),默认不传即全缺省
function renderDrawer(
  onClose: () => void = () => {},
  updateProps: { guiUpdate?: string | null; checking?: boolean; onCheckUpdate?: () => void } = {},
) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <SettingsDrawer onClose={onClose} {...updateProps} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('SettingsDrawer', () => {
  it('打开后渲染五区:身份/语言/外观/关于/高级,版本来自 getVersion', async () => {
    renderDrawer();
    expect(await screen.findByText('节点身份')).toBeInTheDocument();
    // 「语言」「外观」同时是组标题与值行 label(iOS 值行与组同名),断言至少一处
    expect(screen.getAllByText('语言').length).toBeGreaterThan(0);
    expect(screen.getAllByText('外观').length).toBeGreaterThan(0);
    expect(screen.getByText('关于')).toBeInTheDocument();
    // 关于区三行分列 gui/cli/daemon 组件版本,cli 行取已装探测值
    expect(screen.getByText('planet-port-gui')).toBeInTheDocument();
    expect(screen.getByText('planet-port')).toBeInTheDocument();
    expect(screen.getByText('planet-portd')).toBeInTheDocument();
    expect(screen.getByText('0.4.1')).toBeInTheDocument();
    expect(screen.getByText('0.6.0')).toBeInTheDocument();
    expect(screen.getByTestId('settings-drawer')).toBeInTheDocument();
  });

  it('关闭走滑出动画:撤 open 后内容保留,250ms 后整树卸载且抽屉不在文档', () => {
    // jsdom 不跑 transition,关闭卸载由 250ms 定时承载,fake timers 精确推进
    vi.useFakeTimers();
    try {
      renderDrawer();
      // 先推进 16ms 进入帧:entered 翻 open;Radix 受控门控下交互须在进入帧后
      // (真实浏览器该窗口内 content pointer-events:none 不可点)
      act(() => {
        vi.advanceTimersByTime(16);
      });
      expect(screen.getByTestId('settings-drawer')).toHaveAttribute('data-state', 'open');
      // Radix 关闭链路:pointerdown → click → 延迟派发 pointerDownOutside
      fireEvent.pointerDown(mask()!);
      fireEvent.click(mask()!);
      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(screen.getByTestId('settings-drawer')).toBeInTheDocument();
      expect(screen.getByTestId('settings-drawer')).toHaveAttribute('data-state', 'closed');
      act(() => {
        vi.advanceTimersByTime(250);
      });
      // DrawerShell 经 portal 挂 body,断言面向整个文档
      expect(document.querySelector('[data-slot="sheet-content"]')).not.toBeInTheDocument();
      expect(screen.queryByTestId('settings-drawer')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('外观走值行进下级勾选页:点深色立即落 data-theme=dark 且当前值更新', () => {
    renderDrawer();
    // 值行(右侧当前值+›)开下级页,点选项即生效(iOS 设置形态)
    fireEvent.click(screen.getByRole('button', { name: /^外观/ }));
    fireEvent.click(screen.getByTestId('theme-dark'));
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    fireEvent.click(screen.getByTestId('theme-light')); // 还原
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
  });

  it('点遮罩关闭抽屉,onClose 在滑出动画结束(250ms)时回调一次', () => {
    vi.useFakeTimers();
    try {
      const onClose = vi.fn();
      renderDrawer(onClose);
      // 进入帧后交互(见上一用例注释):16ms 前 Radix 受控 open 未翻转,关闭不可达
      act(() => {
        vi.advanceTimersByTime(16);
      });
      fireEvent.pointerDown(mask()!);
      fireEvent.click(mask()!);
      // 延迟派发未发生,父层回调尚未触发
      expect(onClose).not.toHaveBeenCalled();
      act(() => {
        vi.advanceTimersByTime(1);
      });
      // 派发完成 → 组件自持 open 翻 false,父层回调仍等滑出结束
      expect(onClose).not.toHaveBeenCalled();
      act(() => {
        vi.advanceTimersByTime(250);
      });
      // 滑出动画走完才回调父层,且全程只回调一次(不走 249/1 边界切口:
      // 卸载定时器调度点与推进窗口重合会让边界判定随 ±1ms 漂移)
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(screen.queryByTestId('settings-drawer')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('语言走值行进下级勾选页:切 English 后界面语言切换,切回中文还原', () => {
    renderDrawer();
    fireEvent.click(screen.getByRole('button', { name: /^语言/ }));
    fireEvent.click(screen.getByTestId('lang-en'));
    // 语言弹层开启时设置抽屉被 Radix 标记 aria-hidden,role 查询会漏;走文本断言
    expect(screen.getByText('Settings')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('lang-zh')); // 还原
    expect(screen.getByText('设置')).toBeInTheDocument();
  });

  it('关于区:cli 与 daemon 磁盘版本均旧于内嵌 → 两行值列均带可升级箭头', async () => {
    mockManifest({ daemon: '0.7.0', cli: '0.7.0', target: 'aarch64-apple-darwin' }, '0.6.0');
    renderDrawer();
    expect(await screen.findByTestId('settings-install-block')).toBeInTheDocument();
    const marks = screen.getAllByLabelText('可升级');
    expect(marks).toHaveLength(2);
    // 规格要求 aria-label + title 双标注:lucide-react 类型不再收 title 属性,
    // 悬停提示走 SVG 原生 <title> 子元素
    for (const mark of marks) {
      expect(mark.querySelector('title')).toHaveTextContent('可升级');
    }
    // 箭头分别落在 planet-port 与 planet-portd 两行值列(cli 行在前;
    // 'planet-port' 是 'planet-portd' 的前缀,须排除包含误判)
    const rowOf = (el: HTMLElement) => el.closest('div')?.textContent ?? '';
    expect(rowOf(marks[0])).toContain('planet-port');
    expect(rowOf(marks[0])).not.toContain('planet-portd');
    expect(rowOf(marks[1])).toContain('planet-portd');
  });

  it('底部安装区:版本提示行已隐藏,daemon 旧于内嵌时按钮为升级文案', async () => {
    mockManifest({ daemon: '0.7.0', cli: '0.6.0', target: 'aarch64-apple-darwin' }, '0.6.0');
    renderDrawer();
    expect(await screen.findByTestId('settings-install-block')).toBeInTheDocument();
    // 2026-11-27 产品口径:已装/内嵌提示行不再展示(代码注释保留)
    expect(screen.queryByTestId('settings-ver-daemon')).not.toBeInTheDocument();
    expect(screen.queryByTestId('settings-ver-cli')).not.toBeInTheDocument();
    // 整体状态语已废(升级信息收敛到关于区箭头/状态栏),行不存在
    expect(screen.queryByTestId('settings-version-state')).not.toBeInTheDocument();
    const btn = screen.getByTestId('settings-install-daemon');
    expect(btn).toHaveAttribute('data-variant', 'primary');
    expect(btn.className).toContain('w-full');
    // daemonState 探测失败按已装处理;磁盘版本旧于内嵌 → 升级文案
    expect(btn.textContent).toBe('升级 daemon');
    // daemon 磁盘旧于内嵌 → planet-portd 行带箭头;cli 同版无箭头
    expect(screen.getAllByLabelText('可升级')).toHaveLength(1);
  });

  it('两侧同版 → 无状态语、无可升级箭头,按钮为重装文案', async () => {
    mockManifest({ daemon: '0.6.0', cli: '0.6.0', target: 'aarch64-apple-darwin' }, '0.6.0');
    renderDrawer();
    await screen.findByTestId('settings-install-block');
    expect(screen.queryByTestId('settings-version-state')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('可升级')).not.toBeInTheDocument();
    expect(screen.getByTestId('settings-install-daemon').textContent).toBe('重新安装 daemon');
  });

  it('daemon 未装(daemonState.installed=false)→ 按钮为安装文案', async () => {
    // 未装时磁盘探测为 null,安装态是文案唯一判据(版本比对落 unknown 不影响)
    mockManifest({ daemon: '0.6.0', cli: '0.6.0', target: 'aarch64-apple-darwin' }, null, false);
    renderDrawer();
    await screen.findByTestId('settings-install-block');
    expect(screen.getByTestId('settings-install-daemon').textContent).toBe('安装 daemon');
    expect(screen.queryByLabelText('可升级')).not.toBeInTheDocument();
  });

  it('无内嵌资源(manifest 为 null)时底部安装区整块不渲染', async () => {
    renderDrawer();
    expect(await screen.findByText('节点身份')).toBeInTheDocument();
    expect(screen.queryByTestId('settings-install-block')).not.toBeInTheDocument();
    expect(screen.queryByTestId('settings-install-daemon')).not.toBeInTheDocument();
  });

  it('点安装按钮打开重装确认弹窗(默认关闭,确认态含覆盖安装说明)', async () => {
    mockManifest({ daemon: '0.7.0', cli: '0.6.0', target: 'aarch64-apple-darwin' }, '0.6.0');
    renderDrawer();
    expect(screen.queryByTestId('reinstall-dialog')).not.toBeInTheDocument();
    fireEvent.click(await screen.findByTestId('settings-install-daemon'));
    expect(await screen.findByTestId('reinstall-dialog')).toBeInTheDocument();
    expect(screen.getByText(/覆盖安装 daemon 与 CLI/)).toBeInTheDocument();
  });

  it('重装完成(install-done 在弹窗开着时到达)→ 弹窗转成功态并经 onDone 重拉探测刷新常驻文案;兜底 toast 归 App 层,抽屉自身不再弹', async () => {
    mockManifest({ daemon: '0.7.0', cli: '0.6.0', target: 'aarch64-apple-darwin' }, '0.6.0');
    renderDrawer();
    expect(await screen.findByTestId('settings-install-block')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('settings-install-daemon'));
    fireEvent.click(await screen.findByTestId('reinstall-confirm'));
    // 探测改回报新版本,模拟安装落盘后的磁盘状态
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get'
        ? Promise.resolve(identity)
        : cmd === 'get_cli_version'
          ? Promise.resolve('0.7.0')
          : cmd === 'embedded_manifest'
            ? Promise.resolve({ daemon: '0.7.0', cli: '0.6.0', target: 'aarch64-apple-darwin' })
            : cmd === 'get_installed_daemon_version'
              ? Promise.resolve('0.7.0')
              : Promise.resolve(null));
    emit('install-done', undefined);
    expect(await screen.findByTestId('reinstall-done')).toBeInTheDocument();
    // 本文件未挂 App:抽屉层不再自弹安装完成 toast(该职责在 App 层用例覆盖)
    expect(screen.queryByText('daemon 安装完成')).not.toBeInTheDocument();
  });

  it('检查更新行:位于关于区顶部,点击回调 onCheckUpdate', async () => {
    const onCheckUpdate = vi.fn();
    renderDrawer(undefined, { onCheckUpdate });
    expect(await screen.findByText('节点身份')).toBeInTheDocument();
    const row = screen.getByTestId('settings-check-update');
    expect(row.textContent).toContain('检查更新');
    // 关于区首行:gui 版本行紧随其后
    expect(row.nextSibling?.textContent).toContain('planet-port-gui');
    fireEvent.click(row);
    expect(onCheckUpdate).toHaveBeenCalledTimes(1);
  });

  it('检查中(checking=true):行内呈现 spinner 且点击不再触发', async () => {
    const onCheckUpdate = vi.fn();
    renderDrawer(undefined, { onCheckUpdate, checking: true });
    expect(await screen.findByLabelText('checking')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('settings-check-update'));
    expect(onCheckUpdate).not.toHaveBeenCalled();
  });

  it('gui 行升级图标随 guiUpdate 显隐:查得新版点亮,未查得不渲染', async () => {
    const withMark = renderDrawer(undefined, { guiUpdate: '0.7.0' });
    expect(await screen.findByText('节点身份')).toBeInTheDocument();
    const marks = screen.getAllByLabelText('可升级');
    expect(marks).toHaveLength(1);
    expect(marks[0].closest('div')?.textContent ?? '').toContain('planet-port-gui');
    // 两次 render 共挂 body:先卸载前者,再断言无 guiUpdate 时图标不渲染
    withMark.unmount();
    renderDrawer(undefined, { guiUpdate: null });
    await screen.findAllByText('节点身份');
    expect(screen.queryByLabelText('可升级')).not.toBeInTheDocument();
  });
});
