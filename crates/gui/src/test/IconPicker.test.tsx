import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import IconPicker from '../components/IconPicker';
import { initTestI18n } from './i18n';

beforeAll(async () => {
  await initTestI18n('zh');
});

describe('IconPicker', () => {
  it('图形页渲染 92 动物宫格,选中高亮', () => {
    render(<IconPicker animal="seal" color="#dc2626" onSelect={() => {}} onClose={() => {}} />);
    // testing-library 只在 within()/screen/render 上挂查询,元素本身无 getAllByRole
    const grid = within(screen.getByRole('grid'));
    expect(grid.getAllByRole('button').length).toBe(92);
    // 选中态迁 aria-pressed(原 .active 类随结构类一并 Tailwind 化)
    expect(grid.getAllByRole('button')[72]).toHaveAttribute('aria-pressed', 'true');
  });

  it('底色页渲染 24 预设 + 随机底色按钮,完成后回传选择', () => {
    const onSelect = vi.fn();
    render(<IconPicker animal="seal" color="#dc2626" onSelect={onSelect} onClose={() => {}} />);
    fireEvent.click(screen.getByText('底色'));
    const colors = screen.getAllByRole('button').filter(b => b.getAttribute('aria-label')?.startsWith('#'));
    expect(colors.length).toBe(24);
    fireEvent.click(colors[5]!); // 任选一档
    fireEvent.click(screen.getByText('随机底色'));
    fireEvent.click(screen.getByText('完成'));
    expect(onSelect).toHaveBeenCalledWith('seal', expect.stringMatching(/^#[0-9a-f]{6}$/));
  });
});
