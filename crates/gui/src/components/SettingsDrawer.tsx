import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';
import { api } from '../api/client';
import BackgroundPickerSheet from './BackgroundPickerSheet';
import IdentitySection from './IdentitySection';
import DrawerShell from './DrawerShell';
import PlanetManager from './PlanetManager';
import ThemeColorSheet from './ThemeColorSheet';
import { backgroundUrl } from '../background';
import { setTheme, getTheme, type ThemeMode } from '../theme';
import { switchLang } from '../i18n';

/// 设置抽屉(布局参照 iOS PrefsView,即时生效无保存按钮):身份/语言/外观
/// (模式+主题色行+背景行)/关于(版本+Core 版本)/高级(行星管理;节点表入口
/// 已移至行星设置「诊断」)。iOS 的 IPv6/多连接数/按需连接为 VPN 层设置,
/// mac daemon 无对应不做。
/// 瞬时型弹层:由 App 条件渲染挂载,内部持有 open,onClose 只翻 state 走
/// 滑出动画,动画结束(DrawerShell onClosed)才回调父层 onClose 卸载
export default function SettingsDrawer({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [layer, setLayer] = useState<'none' | 'planets' | 'accent' | 'background'>('none');
  const [version, setVersion] = useState('');
  /// theme 本地镜像:setTheme 只写 DOM/localStorage 不触发渲染,须同步 state 高亮当前项
  const [theme, setThemeState] = useState<ThemeMode>(getTheme());
  const status = useQuery({ queryKey: ['status'], queryFn: api.status });
  /// Core 版本取自任一有值的行星(同 daemon 单 core)
  const coreVersion = status.data?.planets.find(p => p.coreVersion)?.coreVersion ?? null;
  useEffect(() => {
    if (!version) void getVersion().then(setVersion).catch(() => setVersion('-'));
  }, [version]);
  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose} testId="settings-drawer">
        <header>
          <h3>{t('settings.title')}</h3>
        </header>
        <IdentitySection />
        <section className="drawer-section">
          <h4>{t('settings.language')}</h4>
          <div className="seg">
            <button data-testid="lang-zh" className={i18n.language === 'zh' ? 'active' : ''} onClick={() => switchLang('zh')}>中文</button>
            <button data-testid="lang-en" className={i18n.language === 'en' ? 'active' : ''} onClick={() => switchLang('en')}>English</button>
          </div>
        </section>
        <section className="drawer-section">
          <h4>{t('settings.appearance')}</h4>
          <div className="seg">
            {(['light', 'dark', 'system'] as ThemeMode[]).map(m => (
              <button
                key={m}
                data-testid={`theme-${m}`}
                className={theme === m ? 'active' : ''}
                onClick={() => {
                  setTheme(m);
                  setThemeState(m);
                }}
              >
                {t(`settings.theme.${m}`)}
              </button>
            ))}
          </div>
          {/* 主题色行:当前色点 + chevron 进色板弹层(iOS 外观组首位行) */}
          <button className="kv" onClick={() => setLayer('accent')}>
            <span>{t('settings.accent')}</span>
            <span className="kv-val">
              <span
                aria-hidden="true"
                style={{
                  display: 'inline-block',
                  width: 16,
                  height: 16,
                  borderRadius: '50%',
                  background: 'var(--primary)',
                  border: '1px solid var(--border)',
                }}
              />
              <span className="chev">›</span>
            </span>
          </button>
          {/* 背景行:缩略图/未设置 + chevron 进选图弹层 */}
          <button className="kv" onClick={() => setLayer('background')}>
            <span>{t('settings.background')}</span>
            <span className="kv-val">
              {backgroundUrl() ? (
                <img src={backgroundUrl()!} alt="" style={{ width: 28, height: 28, objectFit: 'cover', borderRadius: 6, border: '1px solid var(--border)' }} />
              ) : (
                <span className="small">{t('common.notSet')}</span>
              )}
              <span className="chev">›</span>
            </span>
          </button>
        </section>
        <section className="drawer-section">
          <h4>{t('settings.about')}</h4>
          <div className="kv"><span>planet-port</span><span className="mono">{version || '-'}</span></div>
          <div className="kv"><span>ZeroTier Core</span><span className="mono">{coreVersion ?? '-'}</span></div>
        </section>
        <section className="drawer-section">
          <h4>{t('settings.advanced')}</h4>
          <button className="btn" onClick={() => setLayer('planets')}>{t('settings.planetManager')}</button>
        </section>
      </DrawerShell>
      {/* 行星管理/主题色/背景弹层与设置抽屉平级(DrawerShell 经 portal 挂 body):后挂者居上 */}
      {layer === 'planets' && <PlanetManager onClose={() => setLayer('none')} />}
      {layer === 'accent' && <ThemeColorSheet onClose={() => setLayer('none')} />}
      {layer === 'background' && <BackgroundPickerSheet onClose={() => setLayer('none')} />}
    </>
  );
}
