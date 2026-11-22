import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import IdentitySection from '../components/IdentitySection';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue('/tmp/identity.secret') }));

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
  it('展示全局 nodeId 与各行星身份', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identity) : Promise.resolve(null));
    renderSection();
    expect(await screen.findAllByText('1792e3382e')).not.toHaveLength(0);
    expect(await screen.findByText('gozi25')).toBeInTheDocument();
  });

  it('预检→覆盖确认→import 带 force:true(不带 mode,daemon 缺省保持现有模式)', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'identity_get') return Promise.resolve(identity);
      if (cmd === 'identity_import_preflight')
        return Promise.resolve({ preflight: true, nodeId: 'aaaabbbbcc', globalIdentity: { exists: true, nodeId: '1792e3382e' } });
      if (cmd === 'identity_import') return Promise.resolve({ nodeId: 'aaaabbbbcc' });
      return Promise.resolve(null);
    });
    renderSection();
    await screen.findByText('gozi25');
    screen.getByRole('button', { name: /导入身份/ }).click();
    // 覆盖确认弹层展示将导入的新地址
    expect(await screen.findByText(/aaaabbbbcc/)).toBeInTheDocument();
    screen.getByRole('button', { name: '确定' }).click();
    expect(await screen.findByText('身份已导入')).toBeInTheDocument();
    const importCall = invokeMock.mock.calls.find(c => c[0] === 'identity_import');
    expect(importCall?.[1]).toEqual({ path: '/tmp/identity.secret', force: true });
  });

  it('无既有身份时确认导入不带 force(仅覆盖才置 true)', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'identity_get') return Promise.resolve(identity);
      if (cmd === 'identity_import_preflight')
        return Promise.resolve({ preflight: true, nodeId: 'aaaabbbbcc', globalIdentity: { exists: false, nodeId: null } });
      if (cmd === 'identity_import') return Promise.resolve({ nodeId: 'aaaabbbbcc' });
      return Promise.resolve(null);
    });
    renderSection();
    await screen.findByText('gozi25');
    screen.getByRole('button', { name: /导入身份/ }).click();
    expect(await screen.findByText(/aaaabbbbcc/)).toBeInTheDocument();
    screen.getByRole('button', { name: '确定' }).click();
    expect(await screen.findByText('身份已导入')).toBeInTheDocument();
    const importCall = invokeMock.mock.calls.find(c => c[0] === 'identity_import');
    expect(importCall?.[1]).toEqual({ path: '/tmp/identity.secret', force: false });
  });

  it('preflight 响应缺 preflight:true 标记 → 提示升级 daemon 且不 import', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'identity_get') return Promise.resolve(identity);
      if (cmd === 'identity_import_preflight') return Promise.resolve({});
      return Promise.resolve(null);
    });
    renderSection();
    await screen.findByText('gozi25');
    screen.getByRole('button', { name: /导入身份/ }).click();
    expect(await screen.findByText(/版本过旧/)).toBeInTheDocument();
    expect(invokeMock.mock.calls.map(c => c[0])).not.toContain('identity_import');
  });

  it('模式 seg 渲染当前态:shared 高亮且展示共享描述', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identity) : Promise.resolve(null));
    renderSection();
    await screen.findByText('gozi25');
    expect(screen.getByTestId('id-mode-shared')).toHaveClass('active');
    expect(screen.getByTestId('id-mode-independent')).not.toHaveClass('active');
    expect(screen.getByText('共享身份：所有行星使用同一节点')).toBeInTheDocument();
    expect(screen.queryByText('独立身份：每行星各自节点')).not.toBeInTheDocument();
  });

  it('independent 模式高亮独立项且描述行同步', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identityIndependent) : Promise.resolve(null));
    renderSection();
    await screen.findByText('gozi25');
    expect(screen.getByTestId('id-mode-independent')).toHaveClass('active');
    expect(screen.getByTestId('id-mode-shared')).not.toHaveClass('active');
    expect(screen.getByText('独立身份：每行星各自节点')).toBeInTheDocument();
  });

  it('有运行中行星时模式按钮禁用并提示,点击不弹确认', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identity) : Promise.resolve(null));
    renderSection();
    await screen.findByText('gozi25');
    expect(screen.getByTestId('id-mode-shared')).toBeDisabled();
    expect(screen.getByTestId('id-mode-independent')).toBeDisabled();
    expect(screen.getByText(/有 1 个行星运行中/)).toBeInTheDocument();
    screen.getByTestId('id-mode-independent').click();
    expect(screen.queryByText(/切换到「独立身份」/)).not.toBeInTheDocument();
    expect(invokeMock.mock.calls.map(c => c[0])).not.toContain('identity_mode_set');
  });

  it('全停后点击弹确认,确认后调 identity_mode_set 并 toast+invalidate', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'identity_get') return Promise.resolve(identityIdle);
      if (cmd === 'identity_mode_set') return Promise.resolve({ result: 'ok', mode: 'independent' });
      return Promise.resolve(null);
    });
    const { invalidateSpy } = renderSection();
    await screen.findByText('gozi25');
    expect(screen.getByTestId('id-mode-independent')).not.toBeDisabled();
    screen.getByTestId('id-mode-independent').click();
    // 确认弹层展示目标模式与重授权警示
    expect(await screen.findByText('切换身份模式')).toBeInTheDocument();
    expect(screen.getByText(/切换到「独立身份」/)).toBeInTheDocument();
    screen.getByRole('button', { name: '确定' }).click();
    expect(await screen.findByText('已切换为独立身份')).toBeInTheDocument();
    const setCall = invokeMock.mock.calls.find(c => c[0] === 'identity_mode_set');
    expect(setCall?.[1]).toEqual({ mode: 'independent' });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['identity'] });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ['status'] });
  });
});
