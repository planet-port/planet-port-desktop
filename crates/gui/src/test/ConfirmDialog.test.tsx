import { render, screen } from '@testing-library/react';
import { fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import ConfirmDialog from '../components/ConfirmDialog';
import { initTestI18n } from './i18n';

/// wry 不支持 window.confirm，所有破坏性操作走此组件——确认/取消回调路径必须可靠
describe('ConfirmDialog', () => {
  it('open=false 早退渲染不产出任何 DOM（早退前的 useTranslation 等 hooks 正常执行）', async () => {
    await initTestI18n();
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { container } = render(
      <ConfirmDialog open={false} title="删除确认" message="确定删除？" onConfirm={onConfirm} onCancel={onCancel} />,
    );
    expect(container).toBeEmptyDOMElement();
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it('open=true 渲染标题/正文/默认按钮文案（取自 i18n）', async () => {
    await initTestI18n();
    render(<ConfirmDialog open title="删除确认" message="确定删除？" onConfirm={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByText('删除确认')).toBeInTheDocument();
    expect(screen.getByText('确定删除？')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '确定' })).toBeInTheDocument();
  });

  it('confirmLabel 覆盖默认文案，点击分别触发对应回调', async () => {
    await initTestI18n();
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDialog open title="离开网络" message="确定离开该网络？" confirmLabel="离开" onConfirm={onConfirm} onCancel={onCancel} />,
    );
    expect(screen.queryByRole('button', { name: '确定' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '离开' }));
    fireEvent.click(screen.getByRole('button', { name: '取消' }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
