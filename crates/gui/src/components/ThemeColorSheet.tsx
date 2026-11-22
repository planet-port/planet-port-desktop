import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getAccent, setAccent } from '../theme';

/// 预设主题色板(8 色,首项为内置默认):覆盖 --primary 后 hover 亮/暗由
/// CSS color-mix 派生,亮暗两主题共用一个注入点
const PRESETS = ['#5661c9', '#3a6ea5', '#0d9488', '#19935a', '#a97a0a', '#d4494f', '#b04a8f', '#7c5cbf'];

/// 主题色选择弹层(iOS ThemeColorPickerSheet 对位,即时生效):预设色板
/// (选中描环)+ 原生色盘自定 + 恢复默认。遮罩点击关闭,无确认按钮
export default function ThemeColorSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [accent, setAccentState] = useState(() => getAccent());
  const pick = (hex: string | null) => {
    setAccent(hex);
    setAccentState(hex);
  };
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog" onClick={e => e.stopPropagation()}>
        <h3>{t('settings.accent')}</h3>
        <div className="accent-swatches">
          {PRESETS.map(hex => (
            <button
              key={hex}
              style={{ background: hex }}
              className={accent === hex ? 'active' : ''}
              aria-label={hex}
              onClick={() => pick(hex)}
            />
          ))}
        </div>
        <div className="kv" style={{ marginTop: 14 }}>
          <span>{t('settings.accentCustom')}</span>
          <input
            type="color"
            aria-label={t('settings.accentCustom')}
            value={accent ?? PRESETS[0]}
            onChange={e => pick(e.target.value)}
          />
        </div>
        <div className="dialog-actions">
          {accent && <button className="btn" onClick={() => pick(null)}>{t('settings.accentDefault')}</button>}
        </div>
      </div>
    </div>
  );
}
