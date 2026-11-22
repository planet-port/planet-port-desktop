import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ANIMALS, ANIMAL_SVGS, PALETTE, randomColorHex } from '../icons/planetIcon';
import DrawerShell from './DrawerShell';
import PlanetIconView from './PlanetIconView';

/// 图标选择器(iOS IconPickerSheet 对齐):图形 92 宫格 + 底色 24 预设/随机。
/// 确认语义为「完成」回传当前选择;取消不回传
export default function IconPicker({ animal, color, onSelect, onClose }: {
  animal: string; color: string; onSelect: (animal: string, color: string) => void; onClose: () => void;
}) {
  const { t } = useTranslation();
  /// 瞬时型弹层:内部持有 open,onClose 只翻 state 走滑出动画,动画结束
  /// (DrawerShell onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [tab, setTab] = useState<'graphic' | 'color'>('graphic');
  const [a, setA] = useState(animal);
  const [c, setC] = useState(color);
  return (
    <DrawerShell open={open} onClose={close} onClosed={onClose}>
      <header>
        <h3>{t('icon.title')}</h3>
      </header>
      <div className="seg">
        <button className={tab === 'graphic' ? 'active' : ''} onClick={() => setTab('graphic')}>{t('icon.graphic')}</button>
        <button className={tab === 'color' ? 'active' : ''} onClick={() => setTab('color')}>{t('icon.color')}</button>
      </div>
      {tab === 'graphic' ? (
        <div className="icon-grid" role="grid">
          {ANIMALS.map(k => (
            <button key={k} className={k === a ? 'active' : ''} aria-label={k} onClick={() => setA(k)}>
              <img src={ANIMAL_SVGS[k]} alt={k} />
            </button>
          ))}
        </div>
      ) : (
        <div className="color-page">
          <PlanetIconView icon={a} color={c} name={a} id={a} />
          <button className="btn" onClick={() => setC(randomColorHex())}>{t('icon.randomColor')}</button>
          <div className="color-grid">
            {PALETTE.map(h => (
              <button key={h} aria-label={h} className={h === c ? 'active' : ''} style={{ background: h }} onClick={() => setC(h)} />
            ))}
          </div>
        </div>
      )}
      <div className="dialog-actions">
        <button className="btn" onClick={close}>{t('common.cancel')}</button>
        <button className="btn primary" onClick={() => { onSelect(a, c); close(); }}>{t('common.done')}</button>
      </div>
    </DrawerShell>
  );
}
