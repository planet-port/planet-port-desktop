import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';
import IdentitySection from './IdentitySection';
import DrawerShell from './DrawerShell';
import PeersDialog from './PeersDialog';
import PlanetManager from './PlanetManager';
import { setTheme, getTheme, type ThemeMode } from '../theme';
import { switchLang } from '../i18n';

/// 右侧弹开设置抽屉:身份/语言/外观/关于/高级(行星管理+节点表)
export default function SettingsDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const [layer, setLayer] = useState<'none' | 'planets' | 'peers'>('none');
  const [version, setVersion] = useState('');
  /// theme 本地镜像:setTheme 只写 DOM/localStorage 不触发渲染,须同步 state 高亮当前项
  const [theme, setThemeState] = useState<ThemeMode>(getTheme());
  useEffect(() => {
    if (open && !version) void getVersion().then(setVersion).catch(() => setVersion('-'));
  }, [open, version]);
  return (
    <>
      <DrawerShell open={open} onClose={onClose} testId="settings-drawer">
        <header>
          <h3>{t('settings.title')}</h3>
          <button className="btn icon" aria-label={t('common.close')} onClick={onClose}>×</button>
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
        </section>
        <section className="drawer-section">
          <h4>{t('settings.about')}</h4>
          <div className="small">planet-port {version}</div>
        </section>
        <section className="drawer-section">
          <h4>{t('settings.advanced')}</h4>
          <button className="btn" onClick={() => setLayer('planets')}>{t('settings.planetManager')}</button>
          <button className="btn" onClick={() => setLayer('peers')}>{t('settings.peers')}</button>
        </section>
      </DrawerShell>
      {/* 行星管理/节点表与设置抽屉平级(DrawerShell 经 portal 挂 body):后挂者居上 */}
      {layer === 'planets' && <PlanetManager onClose={() => setLayer('none')} />}
      {layer === 'peers' && <PeersDialog onClose={() => setLayer('none')} />}
    </>
  );
}
