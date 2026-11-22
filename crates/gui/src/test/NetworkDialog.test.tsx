import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import NetworkDialog from '../components/NetworkDialog';
import type { NetworkEntry } from '../api/types';
import { ToastProvider } from '../components/Toaster';
import { initTestI18n } from './i18n';

const row = {
  planet: '3530253632', nwid: '5cf54f1815588318', name: '办公网', status: 'OK',
  conflict: null, type: 'PRIVATE', mac: 'aa:bb', interface: 'feth4', mtu: 2800,
  assignedAddrs: ['10.121.17.128/24'], managedRoutes: [], dnsServers: [],
  routeViaZeroTier: true, routeViaLocked: false, dnsMode: 'none', customDns: [],
} as NetworkEntry;

const clipboardWrite = vi.fn();
vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: clipboardWrite } });

beforeAll(async () => {
  await initTestI18n('zh');
});

function renderDialog() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <NetworkDialog row={row} onClose={() => {}} onDone={() => {}} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe('NetworkDialog 分享区', () => {
  it('展示网络 ID 与复制/二维码按钮;点二维码弹出 QRShareSheet', async () => {
    renderDialog();
    expect(screen.getByText('5cf54f1815588318')).toBeInTheDocument();
    screen.getByRole('button', { name: '二维码' }).click();
    // 网络二维码 = 16 hex 纯文本,真实分帧管线必落单帧,直接渲染出图
    expect(await screen.findByAltText('QR')).toBeInTheDocument();
  });

  it('点复制 ID 写入剪贴板并弹成功 toast', async () => {
    renderDialog();
    screen.getByRole('button', { name: '复制 ID' }).click();
    expect(clipboardWrite).toHaveBeenCalledWith('5cf54f1815588318');
    expect(await screen.findByText('已复制')).toBeInTheDocument();
  });
});
