import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';
import { api } from '../api/client';
import BackgroundPickerSheet from './BackgroundPickerSheet';
import CheckListSheet from './CheckListSheet';
import IdentitySection from './IdentitySection';
import DrawerShell from './DrawerShell';
import ThemeColorSheet from './ThemeColorSheet';
import { backgroundUrl } from '../background';
import { setTheme, getTheme, type ThemeMode } from '../theme';
import { switchLang } from '../i18n';

/// 设置抽屉(布局参照 iOS PrefsView,即时生效无保存按钮):身份/语言/外观
/// (模式+主题色行+背景行)/关于(版本+Core 版本)/高级(行星管理;节点表入口
/// 已移至行星设置「诊断」)。选项组件参照 iOS 设置:值行(右侧当前值+›)
/// 进下级勾选页,不再用行内 seg。iOS 的 IPv6/多连接数/按需连接为 VPN 层
/// 设置,mac daemon 无对应不做。
/// 瞬时型弹层:由 App 条件渲染挂载,内部持有 open,onClose 只翻 state 走
/// 滑出动画,动画结束(DrawerShell onClosed)才回调父层 onClose 卸载
export default function SettingsDrawer({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [layer, setLayer] = useState<'none' | 'accent' | 'background' | 'lang' | 'theme'>('none');
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
        <h4 className="group-title">{t('settings.language')}</h4>
        <section className="drawer-section">
          <button className="kv" onClick={() => setLayer('lang')}>
            <span>{t('settings.language')}</span>
            <span className="kv-val">
              <span className="small">{i18n.language === 'zh' ? '中文' : 'English'}</span>
              <span className="chev" aria-hidden="true">›</span>
            </span>
          </button>
        </section>
        <h4 className="group-title">{t('settings.appearance')}</h4>
        <section className="drawer-section">
          <button className="kv" onClick={() => setLayer('theme')}>
            <span>{t('settings.appearance')}</span>
            <span className="kv-val">
              <span className="small">{t(`settings.theme.${theme}`)}</span>
              <span className="chev" aria-hidden="true">›</span>
            </span>
          </button>
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
              <span className="chev" aria-hidden="true">›</span>
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
              <span className="chev" aria-hidden="true">›</span>
            </span>
          </button>
        </section>
        <h4 className="group-title">{t('settings.about')}</h4>
        <section className="drawer-section">
          <div className="kv"><span>planet-port</span><span className="mono">{version || '-'}</span></div>
          <div className="kv"><span>ZeroTier Core</span><span className="mono">{coreVersion ?? '-'}</span></div>
        </section>
      </DrawerShell>
      {/* 语言/外观/主题色/背景弹层与设置抽屉平级(DrawerShell 经 portal 挂 body):后挂者居上 */}
      {layer === 'lang' && (
        <CheckListSheet
          title={t('settings.language')}
          value={i18n.language}
          options={[
            { value: 'zh', label: '中文' },
            { value: 'en', label: 'English' },
          ]}
          onPick={v => {
            switchLang(v as 'zh' | 'en');
            return true;
          }}
          onClose={() => setLayer('none')}
          testId="lang"
        />
      )}
      {layer === 'theme' && (
        <CheckListSheet
          title={t('settings.appearance')}
          value={theme}
          options={(['light', 'dark', 'system'] as ThemeMode[]).map(m => ({ value: m, label: t(`settings.theme.${m}`) }))}
          onPick={v => {
            setTheme(v as ThemeMode);
            setThemeState(v as ThemeMode);
            return true;
          }}
          onClose={() => setLayer('none')}
          testId="theme"
        />
      )}
      {layer === 'accent' && <ThemeColorSheet onClose={() => setLayer('none')} />}
      {layer === 'background' && <BackgroundPickerSheet onClose={() => setLayer('none')} />}
    </>
  );
}
