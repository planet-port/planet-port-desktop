import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsDrawer from '../components/SettingsDrawer';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn().mockResolvedValue('0.4.1') }));

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
    cmd === 'identity_get' ? Promise.resolve(identity) : Promise.resolve(null));
  localStorage.removeItem('theme');
  mockMatchMedia(false);
});

// SettingsDrawer 自身无查询,但内嵌 IdentitySection 走 useQuery/useQueryClient,须挂 Provider
function renderDrawer(props: { open?: boolean; onClose?: () => void } = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <SettingsDrawer open={props.open ?? true} onClose={props.onClose ?? (() => {})} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('SettingsDrawer', () => {
  it('打开后渲染五区:身份/语言/外观/关于/高级,版本来自 getVersion', async () => {
    renderDrawer();
    expect(await screen.findByText('节点身份')).toBeInTheDocument();
    expect(screen.getByText('语言')).toBeInTheDocument();
    expect(screen.getByText('外观')).toBeInTheDocument();
    expect(screen.getByText('关于')).toBeInTheDocument();
    expect(screen.getByText(/planet-port 0\.4\.1/)).toBeInTheDocument();
    expect(screen.getByTestId('settings-drawer')).toBeInTheDocument();
  });

  it('open=false 不渲染', () => {
    renderDrawer({ open: false });
    // DrawerShell 经 portal 挂 body,断言面向整个文档
    expect(document.querySelector('.drawer')).not.toBeInTheDocument();
  });

  it('点深色立即落 data-theme=dark 且按钮高亮', () => {
    renderDrawer();
    // fireEvent(经 act)而非原生 .click():高亮断言依赖 React state 同步刷新
    fireEvent.click(screen.getByTestId('theme-dark'));
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    expect(screen.getByTestId('theme-dark')).toHaveClass('active');
    fireEvent.click(screen.getByTestId('theme-system')); // 还原
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
  });

  it('点遮罩关闭抽屉', () => {
    const onClose = vi.fn();
    renderDrawer({ onClose });
    fireEvent.click(document.querySelector('.drawer-mask')!);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('语言分段切换中文/English', () => {
    renderDrawer();
    fireEvent.click(screen.getByTestId('lang-en'));
    expect(screen.getByTestId('lang-en')).toHaveClass('active');
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('lang-zh'));
    expect(screen.getByTestId('lang-zh')).toHaveClass('active');
  });
});
