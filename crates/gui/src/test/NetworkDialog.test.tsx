import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import NetworkDialog from '../components/NetworkDialog';
import type { NetworkEntry } from '../api/types';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const { invokeMock, saveDialogMock } = vi.hoisted(() => ({
  invokeMock: vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  saveDialogMock: vi.fn<(opts?: Record<string, unknown>) => Promise<string | null>>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: saveDialogMock }));

/// 详情参数面 fixture:MAC 裸 12 hex、地址 1 条、路由含直连(via=null)、DNS v4+v6 混合
const row = {
  planet: '3530253632', nwid: '5cf54f1815588318', name: '办公网', status: 'OK',
  conflict: null, type: 'PRIVATE', mac: 'aabbccddeeff', interface: 'feth4', mtu: 2800,
  assignedAddrs: ['10.121.17.100'],
  managedRoutes: [{ target: '10.121.0.0/16', via: '10.121.17.1' }, { target: '192.168.0.0/24', via: null }],
  dnsServers: [], routeViaZeroTier: true, routeViaLocked: false, dnsMode: 'none',
  customDns: ['10.0.0.1', 'fd00::1'],
} as NetworkEntry;

const clipboardWrite = vi.fn();
vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: clipboardWrite } });

beforeAll(async () => {
  await initTestI18n('zh');
});
beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue({});
  saveDialogMock.mockReset();
  clipboardWrite.mockClear();
});

function renderDialog(overrides: Partial<NetworkEntry> = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <NetworkDialog row={{ ...row, ...overrides }} onClose={() => {}} onDone={() => {}} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('NetworkDialog 网络详情', () => {
  it('只读行:名称/状态/类型/MAC 补冒号/MTU 来自 row', async () => {
    renderDialog();
    expect(await screen.findByText('办公网')).toBeInTheDocument();
    expect(screen.getByText('aa:bb:cc:dd:ee:ff')).toBeInTheDocument();
    expect(screen.getByText('2800')).toBeInTheDocument();
    expect(screen.getByText('PRIVATE')).toBeInTheDocument();
  });

  it('分配地址逐条渲染且可复制;托管路由 target↔via 点击切换,直连不可切', async () => {
    renderDialog();
    expect(await screen.findByText('10.121.17.100')).toBeInTheDocument();
    expect(screen.getByText('10.121.0.0/16')).toBeInTheDocument();
    expect(screen.queryByText(/via 10\.121\.17\.1/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('10.121.17.100'));
    expect(clipboardWrite).toHaveBeenCalledWith('10.121.17.100');
    fireEvent.click(screen.getByText('10.121.0.0/16'));
    expect(screen.getByText(/via 10\.121\.17\.1/)).toBeInTheDocument();
    // 直连路由按钮禁用
    expect(screen.getByText('192.168.0.0/24')).toBeDisabled();
    fireEvent.click(screen.getByText('192.168.0.0/24'));
    expect(clipboardWrite).not.toHaveBeenCalledWith('192.168.0.0/24');
  });

  it('DNS 切到 custom 即时生效:四槽初值以 v4 在前的 string[] 提交 dnsMode+customDns', async () => {
    renderDialog();
    await screen.findByText('办公网');
    fireEvent.click(screen.getByText('自定义'));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_network', {
      nwid: '5cf54f1815588318',
      body: { dnsMode: 'custom', customDns: ['10.0.0.1', 'fd00::1'] },
      planet: '3530253632',
    }));
  });

  it('custom 段四槽回填自 customDns(v4 前两槽/v6 后两槽),失焦且合法且有差异才提交', async () => {
    renderDialog();
    await screen.findByText('办公网');
    fireEvent.click(screen.getByText('自定义'));
    expect(screen.getByPlaceholderText('IPv4 DNS 1')).toHaveValue('10.0.0.1');
    expect(screen.getByPlaceholderText('IPv4 DNS 2（可选）')).toHaveValue('');
    expect(screen.getByPlaceholderText('IPv6 DNS 1')).toHaveValue('fd00::1');
    invokeMock.mockClear();
    // 清空必填槽 → 报错,失焦不发请求
    fireEvent.change(screen.getByPlaceholderText('IPv4 DNS 1'), { target: { value: '' } });
    fireEvent.change(screen.getByPlaceholderText('IPv6 DNS 1'), { target: { value: '' } });
    expect(screen.getByText('DNS 地址格式不正确')).toBeInTheDocument();
    fireEvent.blur(screen.getByPlaceholderText('IPv4 DNS 1'));
    expect(invokeMock).not.toHaveBeenCalled();
    // 修正后失焦 → 提交(v6 清空后仅剩 v4)
    fireEvent.change(screen.getByPlaceholderText('IPv4 DNS 1'), { target: { value: '10.0.0.2' } });
    fireEvent.blur(screen.getByPlaceholderText('IPv4 DNS 1'));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_network', {
      nwid: '5cf54f1815588318',
      body: { dnsMode: 'custom', customDns: ['10.0.0.2'] },
      planet: '3530253632',
    }));
  });

  it('routeVia 锁定:开关禁用且显示锁定文案', async () => {
    renderDialog({ routeViaLocked: true });
    expect(await screen.findByText(/多网络同时连接时不可用/)).toBeInTheDocument();
    expect(screen.getByRole('switch')).toBeDisabled();
  });

  it('routeVia 未锁定:开关可用且已开启,文案为通用说明', async () => {
    renderDialog();
    expect(await screen.findByText(/开启后此网络的默认路由经 ZeroTier 隧道转发/)).toBeInTheDocument();
    expect(screen.getByRole('switch')).toBeEnabled();
    expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'true');
  });

  it('删除网络:确认后 leave_network', async () => {
    renderDialog();
    fireEvent.click(await screen.findByText('删除网络'));
    fireEvent.click(await screen.findByText('删除网络', { selector: '[data-testid="confirm-dialog"] [data-variant="danger"]' }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('leave_network', { nwid: '5cf54f1815588318', planet: '3530253632' }));
    expect(await screen.findByText('已离开网络')).toBeInTheDocument();
  });

  it('ID 行 CopyableValue:点击复制写剪贴板(图标反馈,不发 toast)', async () => {
    renderDialog();
    await screen.findByText('办公网');
    // ID 行与 MAC 行各有一个复制按钮(aria=复制),第一个是 ID 行
    fireEvent.click(screen.getAllByRole('button', { name: '复制' })[0]);
    expect(clipboardWrite).toHaveBeenCalledWith('5cf54f1815588318');
  });

  it('分享区:点二维码弹 QRShareSheet,保存 PNG 走 save 对话框与 write_file_bytes', async () => {
    saveDialogMock.mockResolvedValue('/tmp/out.png');
    renderDialog();
    fireEvent.click(await screen.findByRole('button', { name: '二维码' }));
    expect(await screen.findByAltText('QR')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: '保存 PNG' }));
    await waitFor(() => expect(saveDialogMock).toHaveBeenCalledWith({ defaultPath: '5cf54f1815588318.png' }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('write_file_bytes', expect.objectContaining({ path: '/tmp/out.png' })),
    );
    expect(await screen.findByText('已保存')).toBeInTheDocument();
  });
});
