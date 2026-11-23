import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ANIMALS, ANIMAL_SVGS, PALETTE, randomColorHex } from '../icons/planetIcon';
import PlanetIconView from './PlanetIconView';
import { Dialog, DialogContent, DialogTitle } from './ui-x/Dialog';
import { Button } from './ui-x/Button';

/// 基线 .icon-grid 系对译(styles.css 已删,本组件独占):92 宫格 8 列,
/// 32px 方钮 --panel 底 1px 边,选中主色边;亮色主题图形压黑、暗色还原
const gridBtnCls =
  'flex h-8 cursor-pointer items-center justify-center rounded-[6px] border bg-[var(--panel)] transition-colors duration-[120ms] ease-[ease] focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1';
const gridImgCls = 'size-5 opacity-75 brightness-0 dark:opacity-100 dark:brightness-100';
/// .color-grid 对译:28px 色块 2px 边,hover 放大 6%,选中 --text 描边
const colorBtnCls =
  'h-7 w-full cursor-pointer rounded-[6px] border-2 transition-[border-color,transform] duration-[120ms] ease-[ease] focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1';

/// 图标选择器(iOS IconPickerSheet 对齐):图形 92 宫格 + 底色 24 预设/随机。
/// 确认语义为「完成」回传当前选择;取消不回传。
/// 容器迁 shadcn Dialog(ui-x/Dialog):瞬时型时序——内部持有 open,取消先走退出
/// 动画,动画结束(onClosed)才回调父层卸载;「完成」先回传选择再关。
/// 选中态用 aria-pressed 表达(原 .active 类随结构类一并 Tailwind 化)。
export default function IconPicker({ animal, color, onSelect, onClose }: {
  animal: string; color: string; onSelect: (animal: string, color: string) => void; onClose: () => void;
}) {
  const { t } = useTranslation();
  /// 瞬时型弹层:内部持有 open,onClose 只翻 state 走退出动画,动画结束
  /// (ui-x/Dialog onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [tab, setTab] = useState<'graphic' | 'color'>('graphic');
  const [a, setA] = useState(animal);
  const [c, setC] = useState(color);
  return (
    <Dialog open={open} onOpenChange={o => { if (!o) close(); }} onClosed={onClose}>
      <DialogContent closeLabel={t('common.close')}>
        <DialogTitle>{t('icon.title')}</DialogTitle>
        <div className="seg">
          <button className={tab === 'graphic' ? 'active' : ''} onClick={() => setTab('graphic')}>{t('icon.graphic')}</button>
          <button className={tab === 'color' ? 'active' : ''} onClick={() => setTab('color')}>{t('icon.color')}</button>
        </div>
        {tab === 'graphic' ? (
          <div className="my-2 grid max-h-60 grid-cols-8 gap-1 overflow-y-auto" role="grid">
            {ANIMALS.map(k => (
              <button
                key={k}
                aria-pressed={k === a}
                aria-label={k}
                className={`${gridBtnCls} ${k === a ? 'border-primary' : 'border-border hover:border-input'}`}
                onClick={() => setA(k)}
              >
                <img src={ANIMAL_SVGS[k]} alt={k} className={gridImgCls} />
              </button>
            ))}
          </div>
        ) : (
          <div className="my-2 flex flex-wrap items-center gap-3">
            <PlanetIconView icon={a} color={c} name={a} id={a} />
            <Button onClick={() => setC(randomColorHex())}>{t('icon.randomColor')}</Button>
            <div className="grid flex-1 grid-cols-8 gap-1.5">
              {PALETTE.map(h => (
                <button
                  key={h}
                  aria-label={h}
                  aria-pressed={h === c}
                  style={{ background: h }}
                  className={`${colorBtnCls} ${h === c ? 'border-[var(--text)]' : 'border-border hover:scale-[1.06] hover:border-input'}`}
                  onClick={() => setC(h)}
                />
              ))}
            </div>
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Button onClick={close}>{t('common.cancel')}</Button>
          <Button variant="primary" onClick={() => { onSelect(a, c); close(); }}>{t('common.done')}</Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
