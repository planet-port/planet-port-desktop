import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import OfflineGuide from '../pages/OfflineGuide';
import { initTestI18n } from './i18n';

// listen mock 把回调收集进 listeners,由测试侧手动触发安装事件
const { invokeMock, listeners } = vi.hoisted(() => ({
  invokeMock: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
  listeners: new Map<string, Array<(e: { payload: unknown }) => void>>(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, cb: (e: { payload: unknown }) => void) => {
    const arr = listeners.get(event) ?? [];
    arr.push(cb);
    listeners.set(event, arr);
    return Promise.resolve(() => {
      const list = listeners.get(event);
      if (!list) return;
      const i = list.indexOf(cb);
      if (i >= 0) list.splice(i, 1);
    });
  }),
}));

/// embedded_manifest 返回值由用例按需指定;start_install 可注入 reject 串(在 invoke 时才构造,
/// 避免 rejected promise 悬空触发 unhandled rejection)
let manifest: { version: string; target: string } | null;
let startInstallError: string | null;

function emit(event: string, payload: unknown) {
  act(() => {
    for (const cb of listeners.get(event) ?? []) cb({ payload });
  });
}

const callsOf = (cmd: string) => invokeMock.mock.calls.filter(c => c[0] === cmd).length;

beforeAll(async () => {
  await initTestI18n('zh');
});

beforeEach(() => {
  listeners.clear();
  manifest = null;
  startInstallError = null;
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'embedded_manifest') return Promise.resolve(manifest);
    if (cmd === 'start_install') {
      return startInstallError !== null ? Promise.reject(startInstallError) : Promise.resolve(null);
    }
    return Promise.resolve(null);
  });
});

/// 双态引导:有内嵌资源且需要安装/升级 → 自动安装区;否则维持纯手动引导
describe('OfflineGuide 双态', () => {
  it('manifest=null(无内嵌资源)不出现安装按钮,维持手动引导原样', async () => {
    render(<OfflineGuide installed={false} />);
    await act(async () => {}); // 等 embedded_manifest resolve
    expect(screen.queryByTestId('install-btn')).not.toBeInTheDocument();
    expect(screen.getByText('未检测到 planet-port daemon')).toBeInTheDocument();
    expect(screen.getByText(/curl -fsSL/)).toBeInTheDocument();
  });

  it('manifest 有值且未安装:展示自动安装区,按钮文案「立即安装」,手动命令折叠', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    const { container } = render(<OfflineGuide installed={false} />);
    const btn = await screen.findByTestId('install-btn');
    expect(btn).toHaveTextContent('立即安装');
    expect(screen.getByText(/0\.5\.0/)).toBeInTheDocument(); // 版本说明插值
    // 手动命令退到折叠区(details/summary),不与自动安装区并排裸露
    expect(screen.getByText('手动安装命令')).toBeInTheDocument();
    const details = container.querySelector('details');
    expect(details?.querySelector('.cmd-body')).not.toBeNull();
  });

  it('已装场景(无已装版本号):按钮文案「升级」', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    render(<OfflineGuide installed={true} />);
    const btn = await screen.findByTestId('install-btn');
    expect(btn).toHaveTextContent('升级');
    expect(btn).not.toHaveTextContent('立即安装');
  });

  it('点击安装:乐观置安装态(禁用+spinner 文案)并 invoke start_install', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    render(<OfflineGuide installed={false} />);
    const btn = await screen.findByTestId('install-btn');
    fireEvent.click(btn);
    expect(callsOf('start_install')).toBe(1);
    expect(screen.getByTestId('install-btn')).toBeDisabled();
    expect(screen.getByTestId('install-btn')).toHaveTextContent('正在安装');
    // install-progress 事件维持安装态
    emit('install-progress', 'elevating');
    expect(screen.getByTestId('install-btn')).toBeDisabled();
  });

  it('start_install 被后端拒绝(embedded resources missing)转 failed 态并展示通用失败文案', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    startInstallError = 'embedded resources missing';
    render(<OfflineGuide installed={false} />);
    fireEvent.click(await screen.findByTestId('install-btn'));
    const fail = await screen.findByTestId('install-fail');
    expect(fail).toHaveTextContent('安装启动失败');
    expect(fail).toHaveTextContent('embedded resources missing'); // 非枚举 reason 原样展示供排查
    expect(screen.queryByTestId('install-btn')).not.toBeInTheDocument();
  });

  it('install-failed(daemon-timeout):展示超时文案与「重试」按钮,附日志指引', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    render(<OfflineGuide installed={false} />);
    fireEvent.click(await screen.findByTestId('install-btn'));
    emit('install-failed', 'daemon-timeout');
    const fail = await screen.findByTestId('install-fail');
    expect(fail).toHaveTextContent('服务未在预期时间内上线');
    expect(screen.getByTestId('install-retry')).toBeInTheDocument();
    expect(screen.getAllByText('查看日志').length).toBeGreaterThan(0);
  });

  it('install-failed(install-exit-nonzero):展示退出码失败文案', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    render(<OfflineGuide installed={false} />);
    fireEvent.click(await screen.findByTestId('install-btn'));
    emit('install-failed', 'install-exit-nonzero');
    expect(await screen.findByTestId('install-fail')).toHaveTextContent('退出码非零');
  });

  it('重试回 idle:安装按钮复现并可再次触发安装', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    render(<OfflineGuide installed={false} />);
    fireEvent.click(await screen.findByTestId('install-btn'));
    emit('install-failed', 'install-exit-nonzero');
    fireEvent.click(await screen.findByTestId('install-retry'));
    const btn = await screen.findByTestId('install-btn');
    expect(screen.queryByTestId('install-fail')).not.toBeInTheDocument();
    expect(btn).toBeEnabled();
    fireEvent.click(btn);
    expect(callsOf('start_install')).toBe(2);
  });

  it('install-done 维持安装态,等待 daemon-online 事件切页(不闪失败)', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    render(<OfflineGuide installed={false} />);
    fireEvent.click(await screen.findByTestId('install-btn'));
    emit('install-done', undefined);
    expect(screen.getByTestId('install-btn')).toBeDisabled();
    expect(screen.queryByTestId('install-fail')).not.toBeInTheDocument();
  });

  it('卸载后解除安装事件监听(unlisten 真正执行,不泄漏)', async () => {
    manifest = { version: '0.5.0', target: 'aarch64-apple-darwin' };
    const { unmount } = render(<OfflineGuide installed={false} />);
    await screen.findByTestId('install-btn');
    expect([...listeners.keys()].sort()).toEqual(['install-done', 'install-failed', 'install-progress']);
    unmount();
    await act(async () => {});
    for (const event of ['install-done', 'install-failed', 'install-progress']) {
      expect(listeners.get(event), event).toHaveLength(0);
    }
  });
});
