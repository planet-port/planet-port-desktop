import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsDrawer from '../components/SettingsDrawer';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn().mockResolvedValue('0.4.1') }));
// daemon 安装行挂了 Tauri 事件监听:jsdom 无 IPC,返回空退订的 no-op mock
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

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

// SettingsDrawer 自身无查询,但内嵌 IdentitySection 走 useQuery/useQueryClient,须挂 Provider。
// 组件为瞬时型(App 条件渲染挂载,内部自持 open),不再收 open prop
function renderDrawer(onClose: () => void = () => {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <SettingsDrawer onClose={onClose} />
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
});
