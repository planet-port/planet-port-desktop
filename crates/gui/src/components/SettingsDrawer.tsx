import { useEffect, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';
import { api } from '../api/client';
import type { EmbeddedManifest } from '../api/types';
import BackgroundPickerSheet from './BackgroundPickerSheet';
import CheckListSheet from './CheckListSheet';
import IdentitySection from './IdentitySection';
import DrawerShell from './DrawerShell';
import ThemeColorSheet from './ThemeColorSheet';
import { useToast } from './Toaster';
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

/// 安装失败 reason → i18n key(其余 reject 串走 failGeneric)
const INSTALL_FAIL_KEY: Record<string, string> = {
  'install-exit-nonzero': 'settings.failExit',
  'daemon-timeout': 'settings.failTimeout',
};

/// daemon 安装行(关于组):bundle 内嵌资源时显示,一键安装/重装;无资源
/// (dev/未组包)不渲染。安装走 install-progress/done/failed 事件流与 Rust
/// in-flight 位,两入口不会并发提权。在线重装会重启 daemon:界面短暂离线,
/// 首页只禁用不换屏,daemon 上线后心跳自动恢复
function DaemonInstallRow({ installed }: { installed: boolean }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [manifest, setManifest] = useState<EmbeddedManifest | null>(null);
  const [phase, setPhase] = useState<'idle' | 'installing' | 'failed'>('idle');

  useEffect(() => {
    void api.embeddedManifest().then(setManifest).catch(() => {});
    const unsubs = [
      listen('install-progress', () => setPhase('installing')),
      // done 时 daemon 端口已通(bootstrap 已 wait_port),心跳 ≤3s 恢复在线
      listen('install-done', () => {
        setPhase('idle');
        toast.ok(t('settings.installDone'));
      }),
      listen<string>('install-failed', e => {
        const key = INSTALL_FAIL_KEY[String(e.payload)];
        setPhase('failed');
        toast.error(key ? t(key) : t('settings.failGeneric'));
      }),
    ].map(p => p.then(f => f));
    // 每个 unlisten 必须真正执行,否则重复挂载后监听泄漏
    return () => {
      for (const u of unsubs) void u.then(f => f());
    };
  }, [toast, t]);

  if (!manifest) return null;
  const startInstall = () => {
    setPhase('installing');
    api
      .startInstall()
      .catch((e: unknown) => {
        const msg = String(e);
        // 后端已有安装在跑:保持 installing,等事件回报结果
        if (msg.includes('already in progress')) return;
        // 提权前失败(缺资源/拷贝 staging)走 invoke rejection,事件不会来
        setPhase('failed');
        toast.error(t('settings.failGeneric'));
      });
  };
  return (
    <ValueRow
      divider
      testId="settings-install-daemon"
      label={installed ? t('settings.reinstallDaemon') : t('settings.installDaemon')}
      value={
        phase === 'installing' ? (
          <span
            className="inline-block size-3.5 animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--primary)] [animation-duration:0.8s]"
            aria-label="installing"
          />
        ) : phase === 'failed' ? (
          <span className={smallText}>{t('settings.retry')}</span>
        ) : (
          <span className={smallText}>{manifest.daemon}</span>
        )
      }
      onClick={phase === 'installing' ? undefined : startInstall}
    />
  );
}

/// 设置抽屉(布局参照 iOS PrefsView,即时生效无保存按钮):身份/语言/外观
/// (模式+主题色行+背景行)/关于(gui/cli/daemon 三组件版本+Core 版本)。选项组件参照 iOS 设置:
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
  const [manifest, setManifest] = useState<EmbeddedManifest | null>(null);
  const [cliVersion, setCliVersion] = useState<string | null>(null);
  const [daemonVersion, setDaemonVersion] = useState<string | null>(null);
  /// theme 本地镜像:setTheme 只写 DOM/localStorage 不触发渲染,须同步 state 高亮当前项
  const [theme, setThemeState] = useState<ThemeMode>(getTheme());
  const status = useQuery({ queryKey: ['status'], queryFn: api.status });
  /// Core 版本取自任一有值的行星(同 daemon 单 core)
  const coreVersion = status.data?.planets.find(p => p.coreVersion)?.coreVersion ?? null;
  /// daemon 服务安装态:决定安装行文案(安装/重装);读取失败按已装处理
  const [daemonInstalled, setDaemonInstalled] = useState(true);
  useEffect(() => {
    void api.daemonState().then(s => {
      setDaemonInstalled(s.installed);
      setDaemonVersion(s.daemonVersion);
    }).catch(() => {});
  }, []);
  // cli/daemon 未装(探测为 null)时回退内嵌 manifest 双版本
  useEffect(() => {
    void api.embeddedManifest().then(setManifest).catch(() => {});
  }, []);
  useEffect(() => {
    if (cliVersion === null) void api.cliVersion().then(setCliVersion).catch(() => setCliVersion(null));
  }, [cliVersion]);
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
          <div className={kvRow}><span className={kvLabel}>planet-port-gui</span><span className={monoValue}>{version || '-'}</span></div>
          <div className={`${kvRow} ${rowDivider}`}><span className={kvLabel}>planet-port</span><span className={monoValue}>{cliVersion || manifest?.cli || '-'}</span></div>
          <div className={`${kvRow} ${rowDivider}`}><span className={kvLabel}>planet-portd</span><span className={monoValue}>{daemonVersion || manifest?.daemon || '-'}</span></div>
          <div className={`${kvRow} ${rowDivider}`}><span className={kvLabel}>ZeroTier Core</span><span className={monoValue}>{coreVersion ?? '-'}</span></div>
          <div className={`${kvRow} ${rowDivider}`}><span className={kvLabel}>{t('settings.buildTime')}</span><span className={monoValue}>{buildTime || '-'}</span></div>
          <DaemonInstallRow installed={daemonInstalled} />
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
