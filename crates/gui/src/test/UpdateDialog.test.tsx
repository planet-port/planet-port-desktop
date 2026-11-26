import { fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import UpdateDialog from '../components/UpdateDialog';
import { initTestI18n } from './i18n';

// wry 弹窗内点击 GitHub 链接须经 opener 拉起系统浏览器:mock 隔离真实 invoke
const { openUrlMock } = vi.hoisted(() => ({ openUrlMock: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: openUrlMock }));

beforeAll(async () => {
  await initTestI18n('zh');
});

beforeEach(() => {
  openUrlMock.mockReset();
  openUrlMock.mockResolvedValue(undefined);
});

describe('UpdateDialog 四态', () => {
  const base = { open: true, current: '0.6.1', onClose: vi.fn(), onRetry: vi.fn() };

  it('checking:spinner 与「正在检查…」,不渲染任何按钮', () => {
    render(<UpdateDialog {...base} phase="checking" />);
    expect(screen.getByTestId('update-checking')).toBeInTheDocument();
    // 标题与正文同文案,两处均须出现
    expect(screen.getAllByText('正在检查…')).toHaveLength(2);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('found:标题与正文含当前/最新版本;点「前往 GitHub 下载」以 releases 页调用 openUrl 一次并回调 onClose', () => {
    const onClose = vi.fn();
    render(<UpdateDialog {...base} onClose={onClose} phase="found" latest="0.7.0" />);
    expect(screen.getByText('发现新版本')).toBeInTheDocument();
    expect(screen.getByText('当前 v0.6.1，最新 v0.7.0。')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('update-go'));
    expect(openUrlMock).toHaveBeenCalledTimes(1);
    expect(openUrlMock).toHaveBeenCalledWith(
      'https://github.com/planet-port/planet-port-desktop/releases/tag/planet-port-gui/v0.7.0',
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('latest:「已是最新」标题 + 当前版本正文 + 确定钮回调 onClose', () => {
    const onClose = vi.fn();
    render(<UpdateDialog {...base} onClose={onClose} phase="latest" />);
    expect(screen.getByText('已是最新')).toBeInTheDocument();
    expect(screen.getByText('当前版本 v0.6.1')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '确定' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('failed:失败标题 + 错误原文;重试回调 onRetry,关闭回调 onClose', () => {
    const onClose = vi.fn();
    const onRetry = vi.fn();
    render(
      <UpdateDialog {...base} onClose={onClose} onRetry={onRetry} phase="failed" error="网络不可达" />,
    );
    expect(screen.getByText('检查更新失败')).toBeInTheDocument();
    expect(screen.getByText('网络不可达')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '重试' }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
