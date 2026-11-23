import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import IdentitySection from '../components/IdentitySection';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

const identity = {
  global: { exists: true, nodeId: '1792e3382e' },
  mode: 'shared',
  planets: [{ id: '1', name: 'gozi25', nodeId: '1792e3382e', running: true }],
};

beforeAll(async () => {
  await initTestI18n('zh');
});

const identityIdle = { ...identity, planets: [{ ...identity.planets[0], running: false }] };
const identityIndependent = { ...identityIdle, mode: 'independent' };

function renderSection() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidateSpy = vi.spyOn(qc, 'invalidateQueries');
  const view = render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <IdentitySection />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { ...view, invalidateSpy };
}

describe('IdentitySection', () => {
  it('shared 态值行显示「共享」,foot 显示共享模式说明', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identityIdle) : Promise.resolve(null));
    renderSection();
    expect(await screen.findByText(/所有行星使用同一节点/)).toBeInTheDocument();
    // 值行右侧为弱化值(基线 .small 对译后的 Tailwind 形态);基线用子串匹配
    // (toHaveTextContent),这里以正则保持同口径,断言锚定行内文本
    expect(within(screen.getByTestId('id-mode')).getByText(/共享身份/)).toBeInTheDocument();
    // 交互形态回归保护:身份切换须显式确认,不得回到行内 switch
    expect(screen.getByTestId('id-mode').querySelector('[data-slot="switch"]')).toBeNull();
  });

  it('independent 态值行显示「独立」,foot 显示独立模式说明', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identityIndependent) : Promise.resolve(null));
    renderSection();
    expect(await screen.findByText(/每行星各自节点/)).toBeInTheDocument();
    expect(within(screen.getByTestId('id-mode')).getByText(/独立身份/)).toBeInTheDocument();
  });

  it('点击值行弹确认弹窗,取消不切换,确认后 set+toast+invalidate', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'identity_get') return Promise.resolve(identityIdle);
      if (cmd === 'identity_mode_set') return Promise.resolve({ result: 'ok', mode: 'independent' });
      return Promise.resolve(null);
    });
    const { invalidateSpy } = renderSection();
    await screen.findByText(/所有行星使用同一节点/);
    fireEvent.click(screen.getByTestId('id-mode'));
    // 弹窗明示重新授权后果,未确认前不触发切换
    expect(await screen.findByText(/重新获得授权/)).toBeInTheDocument();
    expect(invokeMock.mock.calls.map(c => c[0])).not.toContain('identity_mode_set');
    fireEvent.click(screen.getByText('取消'));
    expect(invokeMock.mock.calls.map(c => c[0])).not.toContain('identity_mode_set');
    fireEvent.click(screen.getByTestId('id-mode'));
    fireEvent.click(await screen.findByText('确定'));
    expect(await screen.findByText('已切换为独立身份')).toBeInTheDocument();
    const setCall = invokeMock.mock.calls.find(c => c[0] === 'identity_mode_set');
    expect(setCall?.[1]).toEqual({ mode: 'independent' });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['identity'] });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['status'] });
  });

  it('有运行中行星时行禁用,foot 显示受阻提示且不触发切换', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identity) : Promise.resolve(null));
    renderSection();
    expect(await screen.findByText(/停止全部行星后才能切换/)).toBeInTheDocument();
    expect(screen.getByTestId('id-mode')).toBeDisabled();
    expect(invokeMock.mock.calls.map(c => c[0])).not.toContain('identity_mode_set');
  });
});
