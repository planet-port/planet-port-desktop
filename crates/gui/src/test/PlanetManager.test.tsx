import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import PlanetManager from '../components/PlanetManager';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

// PlanetManager(自 Planets.tsx 收编)走 ['status'] 查询 → get_status 命令
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

const planet = (over: Record<string, unknown>) => ({
  id: '1', name: 'gozi25', icon: '', color: '', type: 'custom', status: 'running', enabled: true,
  online: true, nodeId: '1792e3382e', coreVersion: '1.16.2', latencyMs: 60, networkCount: 5,
  port: 9993, transport: null, tcpFallbackRelay: null, ...over,
});

beforeAll(async () => {
  await initTestI18n('zh');
});

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_status')
      return Promise.resolve({
        planets: [planet({}), planet({ id: '2', name: 'official-root', type: 'official', nodeId: null })],
      });
    return Promise.resolve(null);
  });
});

function renderManager() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <PlanetManager onClose={() => {}} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('PlanetManager 分享', () => {
  it('custom 行星展示导出文件/二维码按钮,official 不展示', async () => {
    renderManager();
    expect(await screen.findAllByText('gozi25')).not.toHaveLength(0);
    expect(await screen.findAllByRole('button', { name: '导出文件' })).toHaveLength(1);
    expect(await screen.findAllByRole('button', { name: '二维码' })).toHaveLength(1);
  });
});
