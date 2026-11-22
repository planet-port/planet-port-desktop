import { render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import QRShareSheet from '../components/QRShareSheet';
import { initTestI18n } from './i18n';

const enc = (s: string) => new TextEncoder().encode(s);

/// mock 分帧层而非 qrcode:帧数由 mock 直接控制(真实 encodeFrames 的行为
/// 已由 frames.test.ts 覆盖);qrcode 的 toDataURL 是纯 JS PNG 渲染,
/// 在 jsdom 可用且快,保留真实实现让 dataURL 断言有意义。
const { encodeFramesMock, MockTooLarge } = vi.hoisted(() => {
  class MockTooLarge extends Error {}
  return { encodeFramesMock: vi.fn<() => string[]>(), MockTooLarge };
});
vi.mock('../ztp/frames', () => ({
  encodeFrames: encodeFramesMock,
  QRTooLargeError: MockTooLarge,
}));

beforeAll(async () => {
  await initTestI18n('zh');
});

describe('QRShareSheet', () => {
  it('单帧: 渲染二维码图与保存按钮,点击保存回调 dataUrl', async () => {
    encodeFramesMock.mockReturnValue(['ZTQR1|01|01|deadbeef|00045|TESTCHUNK']);
    const onSave = vi.fn();
    render(<QRShareSheet title="分享" data={enc('5cf54f1815588318')} subtitle="办公网" onSave={onSave} onDismiss={() => {}} />);
    const img = await screen.findByAltText('QR');
    expect(img.getAttribute('src')).toMatch(/^data:image\/png;base64,/);
    screen.getByText('办公网');
    screen.getByRole('button', { name: '保存 PNG' }).click();
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0]).toMatch(/^data:image\/png;base64,/);
  });

  it('多帧: 显示帧计数,不渲染保存按钮', async () => {
    encodeFramesMock.mockReturnValue(Array.from({ length: 30 }, (_, i) => `FRAME${i}`));
    render(<QRShareSheet title="分享" data={enc('x')} onDismiss={() => {}} />);
    await screen.findByAltText('QR', {}, { timeout: 4000 });
    expect(await screen.findByText('1/30')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '保存 PNG' })).not.toBeInTheDocument();
  });

  it('超限载荷: 显示过大提示,不出二维码', async () => {
    encodeFramesMock.mockImplementation(() => {
      throw new MockTooLarge('too large');
    });
    render(<QRShareSheet title="分享" data={enc('y')} onDismiss={() => {}} />);
    expect(await screen.findByText(/过大/)).toBeInTheDocument();
    expect(screen.queryByAltText('QR')).not.toBeInTheDocument();
  });
});
