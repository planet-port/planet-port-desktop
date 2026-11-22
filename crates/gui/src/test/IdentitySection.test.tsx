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

function renderSection() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <IdentitySection />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('IdentitySection', () => {
  it('展示全局 nodeId 与各行星身份', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'identity_get' ? Promise.resolve(identity) : Promise.resolve(null));
    renderSection();
    expect(await screen.findAllByText('1792e3382e')).not.toHaveLength(0);
    expect(await screen.findByText('gozi25')).toBeInTheDocument();
  });

  it('预检→覆盖确认→import 带 force:true(固定 mode shared 在 Rust 侧)', async () => {
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
});
