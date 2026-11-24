import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getAccent, setAccent } from '../theme';
import DrawerShell from './DrawerShell';
import { Button } from './ui-x/Button';

/// 预设主题色板(8 色,首项为内置默认):覆盖 --primary 后 hover 亮/暗由
/// CSS color-mix 派生,亮暗两主题共用一个注入点
const PRESETS = ['#5661c9', '#3a6ea5', '#0d9488', '#19935a', '#a97a0a', '#d4494f', '#b04a8f', '#7c5cbf'];

/// 基线 .accent-swatches 系对译(styles.css 已删,本组件独占):8 列圆形
/// 色板,2px 透明边占位,选中换 --text 描边;hover 未选中才放大 6% + 描环
const swatchCls =
  'aspect-square cursor-pointer rounded-full border-2 border-transparent p-0 transition-[transform,box-shadow] duration-[120ms] ease-[ease] focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1';

/// 主题色选择弹层(iOS ThemeColorPickerSheet 对位,即时生效):预设色板
/// (选中描环)+ 原生色盘自定 + 恢复默认。遮罩点击/Esc 关闭,无确认按钮。
/// 容器统一右侧抽屉(DrawerShell,2026-11-24 口径):瞬时型时序——内部持有
/// open,关闭走退出动画后再回调父层卸载。
/// 选中态用 aria-pressed 表达(原 .active 类随结构类一并 Tailwind 化)
export default function ThemeColorSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [accent, setAccentState] = useState(() => getAccent());
  const pick = (hex: string | null) => {
    setAccent(hex);
    setAccentState(hex);
  };
  return (
    <DrawerShell open={open} onClose={close} onClosed={onClose}>
      <header className="flex items-center justify-between pr-[30px]">
        <h3 className="m-0 text-sm font-medium">{t('settings.accent')}</h3>
      </header>
        <div className="grid grid-cols-8 gap-1.5">
          {PRESETS.map(hex => (
            <button
              key={hex}
              style={{ background: hex }}
              aria-pressed={accent === hex}
              aria-label={hex}
              className={`${swatchCls} ${accent === hex ? 'border-[var(--text)]' : 'hover:scale-110 hover:[box-shadow:0_0_0_2px_var(--border-h)]'}`}
              onClick={() => pick(hex)}
            />
          ))}
        </div>
        <div className="kv mt-3.5">
          <span>{t('settings.accentCustom')}</span>
          <input
            type="color"
            aria-label={t('settings.accentCustom')}
            value={accent ?? PRESETS[0]}
            onChange={e => pick(e.target.value)}
          />
        </div>
        <div className="mt-4 flex justify-end gap-2">
          {accent && <Button onClick={() => pick(null)}>{t('settings.accentDefault')}</Button>}
        </div>
    </DrawerShell>
  );
}
