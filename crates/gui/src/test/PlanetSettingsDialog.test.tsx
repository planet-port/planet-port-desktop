import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import PlanetSettingsDialog from '../components/PlanetSettingsDialog';
import { ToastProvider } from '../components/Toaster';
import type { PlanetStatus } from '../api/types';
import { initTestI18n } from './i18n';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn(), open: vi.fn() }));

const planet: PlanetStatus = {
  id: '1', name: 'gozi25', icon: '', color: '', type: 'custom', status: 'running',
  enabled: true, online: true, nodeId: '1792e3382e', coreVersion: null, latencyMs: 12,
  networkCount: 2, port: 9993, transport: null, tcpFallbackRelay: null,
};
const detail = {
  id: '1', name: 'gozi25', icon: '', color: '', type: 'custom', worldId: 2929292,
  roots: ['1.2.3.4/9993'], fileSize: 1234567, revisionTimeMs: Date.UTC(2026, 10, 1, 8, 0, 0),
  lastNodeId: '1792e3382e', source: '/path/planet', networkCount: 2, port: 9993,
  status: 'running', enabled: true, online: true, transport: null, tcpFallbackRelay: null,
};

beforeAll(async () => {
  await initTestI18n('zh');
});
beforeEach(() => {
  invokeMock.mockReset();
  // planet_detail 命令透传 daemon 信封 { planet: <detail> },client.ts 解包 .planet
  invokeMock.mockImplementation((cmd: string) =>
    cmd === 'planet_detail' ? Promise.resolve({ planet: detail }) : Promise.resolve({}));
});

const renderDialog = (p: PlanetStatus = planet) => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <PlanetSettingsDialog planet={p} onClose={() => {}} onDone={() => {}} />
      </ToastProvider>
    </QueryClientProvider>,
  );
};

describe('PlanetSettingsDialog', () => {
  it('只读信息:WorldID 16 位十六进制/节点 ID/类型/文件大小(1000 进制)/修订时间', async () => {
    renderDialog();
    await screen.findByText('00000000002cb28c'); // (2929292).toString(16) 补零
    expect(screen.getByText('1792e3382e')).toBeInTheDocument();
    expect(screen.getByText('1.2 MB')).toBeInTheDocument();
    expect(screen.getByText(/2026/)).toBeInTheDocument();
  });

  it('custom:无改动时保存禁用;改名后保存提交 planet_update', async () => {
    renderDialog();
    const save = await screen.findByText('保存');
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByDisplayValue('gozi25'), { target: { value: '新名字' } });
    expect(save).toBeEnabled();
    fireEvent.click(save);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('planet_update', expect.objectContaining({
      id: '1', name: '新名字',
      icon: expect.stringMatching(/^[a-z-]+$/), color: expect.stringMatching(/^#[0-9a-f]{6}$/),
    })));
  });

  it('custom:导出文件/二维码与删除入口存在,删除需确认', async () => {
    renderDialog();
    expect(await screen.findByText('导出文件')).toBeInTheDocument();
    expect(screen.getByText('二维码')).toBeInTheDocument();
    fireEvent.click(screen.getByText('删除'));
    fireEvent.click(await screen.findByText('删除', { selector: '[data-testid="confirm-dialog"] .btn.danger' }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('remove_planet', { id: '1' }));
  });

  it('official:无编辑区/无分享/无删除', async () => {
    renderDialog({ ...planet, type: 'official' });
    await screen.findByText('00000000002cb28c');
    expect(screen.queryByText('导出文件')).not.toBeInTheDocument();
    expect(screen.queryByText('删除')).not.toBeInTheDocument();
    expect(screen.queryByText('保存')).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue('gozi25')).not.toBeInTheDocument();
  });
});
