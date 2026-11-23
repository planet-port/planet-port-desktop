import { useEffect, useState, type ReactNode } from 'react';
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

/// .kv 语义类 Tailwind 对译(与报告速查表同口径)
const sectionCard = 'flex flex-col rounded-md border border-[color:var(--border)] bg-[var(--panel)] dark:bg-[#22252d] dark:border-[#2c3038] px-2 py-0.5';
const rowDivider = 'relative before:absolute before:top-0 before:left-4 before:right-0 before:border-t before:border-[color:var(--border-soft)] before:content-[""]';
const kvRow = 'flex items-center justify-between gap-3 -mx-2 px-2 py-1.5 text-[13px]';
const kvButtonRow = `${kvRow} w-full cursor-pointer border-0 bg-transparent text-left rounded-[6px] transition-colors duration-[120ms] ease-[ease] enabled:hover:bg-[var(--panel)]`;
const kvLabel = 'shrink-0 text-[var(--text)]';
const kvVal = 'inline-flex min-w-0 items-center justify-end gap-1.5';
const chev = 'shrink-0 text-sm leading-none text-[var(--faint)]';
const smallText = 'text-xs text-[var(--muted)]';
const groupTitle = 'mt-0.5 -mb-1.5 mr-2 text-xs font-medium text-[var(--muted)]';
const monoValue = 'font-mono break-all text-right';

/// 值行(label + 当前值 + ›)通用骨架:进下级勾选页的行与展示行共用
function ValueRow({
  label, value, onClick, testId, divider,
}: {
  label: string;
  value: ReactNode;
  onClick?: () => void;
  testId?: string;
  divider?: boolean;
}) {
  return (
    <button className={divider ? `${kvButtonRow} ${rowDivider}` : kvButtonRow} data-testid={testId} onClick={onClick}>
      <span className={kvLabel}>{label}</span>
      <span className={kvVal}>
        {value}
        <span className={chev} aria-hidden="true">›</span>
      </span>
    </button>
  );
}

/// 设置抽屉(布局参照 iOS PrefsView,即时生效无保存按钮):身份/语言/外观
/// (模式+主题色行+背景行)/关于(版本+Core 版本)。选项组件参照 iOS 设置:
/// 值行(右侧当前值+›)进下级勾选页,不再用行内 seg。iOS 的 IPv6/多连接数/
/// 按需连接为 VPN 层设置,mac daemon 无对应不做。
/// 瞬时型弹层:由 App 条件渲染挂载,内部持有 open,onClose 只翻 state 走
/// 滑出动画,动画结束(DrawerShell onClosed)才回调父层 onClose 卸载
export default function SettingsDrawer({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [layer, setLayer] = useState<'none' | 'accent' | 'background' | 'lang' | 'theme'>('none');
  const [version, setVersion] = useState('');
  const [buildTime, setBuildTime] = useState('');
  /// theme 本地镜像:setTheme 只写 DOM/localStorage 不触发渲染,须同步 state 高亮当前项
  const [theme, setThemeState] = useState<ThemeMode>(getTheme());
  const status = useQuery({ queryKey: ['status'], queryFn: api.status });
  /// Core 版本取自任一有值的行星(同 daemon 单 core)
  const coreVersion = status.data?.planets.find(p => p.coreVersion)?.coreVersion ?? null;
  useEffect(() => {
    if (!version) void getVersion().then(setVersion).catch(() => setVersion('-'));
  }, [version]);
  useEffect(() => {
    if (!buildTime) void api.buildTime().then(setBuildTime).catch(() => setBuildTime('-'));
  }, [buildTime]);
  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose} testId="settings-drawer">
        <header className="flex items-center justify-between pr-[30px]">
          <h3 className="m-0 text-sm font-medium">{t('settings.title')}</h3>
        </header>
        <IdentitySection />
        <h4 className={groupTitle}>{t('settings.language')}</h4>
        <section className={sectionCard}>
          <ValueRow label={t('settings.language')} value={<span className={smallText}>{i18n.language === 'zh' ? '中文' : 'English'}</span>} onClick={() => setLayer('lang')} testId="settings-lang-row" />
        </section>
        <h4 className={groupTitle}>{t('settings.appearance')}</h4>
        <section className={sectionCard}>
          <ValueRow label={t('settings.appearance')} value={<span className={smallText}>{t(`settings.theme.${theme}`)}</span>} onClick={() => setLayer('theme')} testId="settings-appearance-row" />
          {/* 主题色行:当前色点 + chevron 进色板弹层(iOS 外观组首位行) */}
          <ValueRow
            divider
            label={t('settings.accent')}
            value={
              <span
                aria-hidden="true"
                className="inline-block size-4 rounded-full border border-[color:var(--border)] bg-[var(--primary)]"
              />
            }
            onClick={() => setLayer('accent')}
          />
          {/* 背景行:缩略图/未设置 + chevron 进选图弹层 */}
          <ValueRow
            divider
            label={t('settings.background')}
            value={
              backgroundUrl() ? (
                <img src={backgroundUrl()!} alt="" className="size-7 rounded-[6px] border border-[color:var(--border)] object-cover" />
              ) : (
                <span className={smallText}>{t('common.notSet')}</span>
              )
            }
            onClick={() => setLayer('background')}
          />
        </section>
        <h4 className={groupTitle}>{t('settings.about')}</h4>
        <section className={sectionCard}>
          <div className={kvRow}><span className={kvLabel}>planet-port</span><span className={monoValue}>{version || '-'}</span></div>
          <div className={`${kvRow} ${rowDivider}`}><span className={kvLabel}>ZeroTier Core</span><span className={monoValue}>{coreVersion ?? '-'}</span></div>
          <div className={`${kvRow} ${rowDivider}`}><span className={kvLabel}>{t('settings.buildTime')}</span><span className={monoValue}>{buildTime || '-'}</span></div>
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
