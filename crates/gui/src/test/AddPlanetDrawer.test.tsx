import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import AddPlanetDrawer from '../components/AddPlanetDrawer';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock, openMock } = vi.hoisted(() => ({
  invokeMock: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  openMock: vi.fn<() => Promise<string | null>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openMock, save: vi.fn() }));

beforeAll(async () => {
  await initTestI18n('zh');
});
beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue({});
});

const renderDrawer = () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <AddPlanetDrawer onClose={() => {}} />
      </ToastProvider>
    </QueryClientProvider>,
  );
};

describe('AddPlanetDrawer', () => {
  it('名称或源为空时禁用提交', async () => {
    renderDrawer();
    const btn = await screen.findByRole('button', { name: '添加行星服务器' });
    expect(btn).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText('行星名称'), { target: { value: '新行星' } });
    expect(btn).toBeDisabled();
  });

  it('文件选择填源;提交透传 name/icon/color', async () => {
    openMock.mockResolvedValue('/tmp/new.planet');
    renderDrawer();
    fireEvent.click(await screen.findByText('选择行星文件'));
    // 文件选择回调异步落 source state
    await waitFor(() => expect(screen.getByText('/tmp/new.planet')).toBeInTheDocument());
    fireEvent.change(screen.getByPlaceholderText('行星名称'), { target: { value: '新行星' } });
    // 标题与提交按钮同文案,经 role 消歧锁定提交按钮
    fireEvent.click(screen.getByRole('button', { name: '添加行星服务器' }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('add_planet', expect.objectContaining({
      source: '/tmp/new.planet',
      name: '新行星',
      icon: expect.stringMatching(/^[a-z-]+$/),
      color: expect.stringMatching(/^#[0-9a-f]{6}$/),
    })));
  });

  it('URL 输入 + 导入后作为源提交', async () => {
    renderDrawer();
    fireEvent.change(screen.getByPlaceholderText('https://example.com/planet.planet'), { target: { value: 'https://example.com/planet.planet' } });
    fireEvent.click(screen.getByText('导入'));
    fireEvent.change(screen.getByPlaceholderText('行星名称'), { target: { value: 'URL 行星' } });
    fireEvent.click(screen.getByRole('button', { name: '添加行星服务器' }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('add_planet', expect.objectContaining({ source: 'https://example.com/planet.planet', name: 'URL 行星' })));
  });
});
