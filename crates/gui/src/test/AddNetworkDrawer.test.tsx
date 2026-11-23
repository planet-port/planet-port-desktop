import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import AddNetworkDrawer from '../components/AddNetworkDrawer';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

beforeAll(async () => {
  await initTestI18n('zh');
});
beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_status') return Promise.resolve({ planets: [{ id: '1', name: 'gozi25', icon: '', color: '', type: 'custom', status: 'running', enabled: true, online: true, nodeId: '1792e3382e', coreVersion: null, latencyMs: 12, networkCount: 2, port: 9993, transport: null, tcpFallbackRelay: null }] });
    if (cmd === 'get_networks') return Promise.resolve({ networks: [{ planet: '1', nwid: '9536600adcf2b350', name: '办公网', status: 'OK', conflict: null, type: 'PRIVATE', mac: 'aabbccddeeff', interface: 'zt0', mtu: 2800, assignedAddrs: [], managedRoutes: [], dnsServers: [], routeViaZeroTier: false, routeViaLocked: false, dnsMode: 'none', customDns: [] }] });
    return Promise.resolve({});
  });
});

const renderDrawer = () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <AddNetworkDrawer onClose={() => {}} />
      </ToastProvider>
    </QueryClientProvider>,
  );
};

describe('AddNetworkDrawer', () => {
  it('ID 非 16 位十六进制时禁用提交并提示', async () => {
    renderDrawer();
    const btn = await screen.findByText('加入');
    expect(btn).toBeDisabled();
    const input = screen.getByPlaceholderText('16 位十六进制');
    fireEvent.change(input, { target: { value: 'xyz' } });
    expect(screen.getByText('网络 ID 必须为 16 位十六进制字符')).toBeInTheDocument();
    expect(btn).toBeDisabled();
  });

  it('合法 ID + 默认参数提交 join_network(小写),不带 setNetwork', async () => {
    renderDrawer();
    fireEvent.change(await screen.findByPlaceholderText('16 位十六进制'), { target: { value: 'ABCDEF0123456789' } });
    fireEvent.click(screen.getByText('加入'));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('join_network', { nwid: 'abcdef0123456789', planet: null }));
    expect(invokeMock).not.toHaveBeenCalledWith('set_network', expect.anything());
  });

  it('重复网络预检:已在列表的 ID 提交被拦,不触发 join', async () => {
    renderDrawer();
    fireEvent.change(await screen.findByPlaceholderText('16 位十六进制'), { target: { value: '9536600adcf2b350' } });
    fireEvent.click(screen.getByText('加入'));
    await waitFor(() => expect(screen.getByText(/已在列表中/)).toBeInTheDocument());
    expect(invokeMock).not.toHaveBeenCalledWith('join_network', expect.anything());
  });

  it('custom DNS 空槽禁用提交;填入合法值后 join+setNetwork', async () => {
    renderDrawer();
    fireEvent.change(await screen.findByPlaceholderText('16 位十六进制'), { target: { value: 'abcdef0123456789' } });
    // Radix TabsTrigger 走 mousedown 激活(button=0 且非 ctrl),click 不换段
    fireEvent.mouseDown(screen.getByText('自定义'));
    const join = screen.getByText('加入');
    expect(join).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText('IPv4 DNS 1'), { target: { value: '10.0.0.1' } });
    expect(join).toBeEnabled();
    fireEvent.click(join);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('join_network', expect.anything()));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_network', expect.objectContaining({
      body: expect.objectContaining({ dnsMode: 'custom', customDns: ['10.0.0.1'] }),
    })));
  });
});
