import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { api } from '../api/client';
import type { EmbeddedManifest } from '../api/types';
import BackgroundPickerSheet from './BackgroundPickerSheet';
import CheckListSheet from './CheckListSheet';
import IdentitySection from './IdentitySection';
import DrawerShell from './DrawerShell';
import ReinstallDialog from './ReinstallDialog';
import ThemeColorSheet from './ThemeColorSheet';
import { useToast } from './Toaster';
import { Button } from './ui-x/Button';
import { backgroundUrl } from '../background';
import { compareVersion, overallState } from '../lib/version';
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
/// (模式+主题色行+背景行)/关于(gui/cli/daemon 三组件版本+Core 版本)/底部
/// 安装区(已装 vs 内嵌两行+整体状态语+全宽安装按钮)。选项组件参照 iOS 设置:
/// 值行(右侧当前值+›)进下级勾选页,不再用行内 seg。iOS 的 IPv6/多连接数/
/// 按需连接为 VPN 层设置,mac daemon 无对应不做。
/// 瞬时型弹层:由 App 条件渲染挂载,内部持有 open,onClose 只翻 state 走
/// 滑出动画,动画结束(DrawerShell onClosed)才回调父层 onClose 卸载
export default function SettingsDrawer({ onClose }: { onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const toast = useToast();
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
  /// daemon 服务安装态:决定安装按钮文案(安装/重装);读取失败按已装处理
  const [daemonInstalled, setDaemonInstalled] = useState(true);
  /// 已装 daemon 磁盘版本(spawn 探测):版本差异检测用,与 daemonVersion(运行中)口径不同
  const [installedDaemon, setInstalledDaemon] = useState<string | null>(null);
  /// 重装弹窗开关;安装结果弹窗开着时由弹窗自显,兜底 toast 只在其已关时补
  const [reinstallOpen, setReinstallOpen] = useState(false);
  const reinstallOpenRef = useRef(reinstallOpen);
  useEffect(() => {
    reinstallOpenRef.current = reinstallOpen;
  }, [reinstallOpen]);
  /// 已装版本/安装态三探针重拉:挂载首查 + 安装完成后刷新常驻文案
  const refreshVersions = useCallback(() => {
    void api.daemonState().then(s => {
      setDaemonInstalled(s.installed);
      setDaemonVersion(s.daemonVersion);
    }).catch(() => {});
    void api.installedDaemonVersion().then(setInstalledDaemon).catch(() => setInstalledDaemon(null));
    void api.cliVersion().then(setCliVersion).catch(() => setCliVersion(null));
  }, []);
  useEffect(() => {
    refreshVersions();
  }, [refreshVersions]);
  // cli/daemon 未装(探测为 null)时回退内嵌 manifest 双版本
  useEffect(() => {
    void api.embeddedManifest().then(setManifest).catch(() => {});
  }, []);
  useEffect(() => {
    if (!version) void getVersion().then(setVersion).catch(() => setVersion('-'));
  }, [version]);
  useEffect(() => {
    if (!buildTime) void api.buildTime().then(setBuildTime).catch(() => setBuildTime('-'));
  }, [buildTime]);
  useEffect(() => {
    const failText = (r: string) =>
      r === 'install-exit-nonzero' ? t('settings.failExit')
        : r === 'daemon-timeout' ? t('settings.failTimeout')
        : t('settings.failGeneric');
    // 安装结果兜底监听(原 DaemonInstallRow 逻辑):取消等待后弹窗已关,后台
    // 安装仍会落地结果,此处刷新版本并以 toast 告知
    const unsubs = [
      listen('install-done', () => {
        refreshVersions();
        if (!reinstallOpenRef.current) toast.ok(t('settings.installDone'));
      }),
      listen<string>('install-failed', e => {
        if (!reinstallOpenRef.current) toast.error(failText(String(e.payload)));
      }),
    ].map(p => p.then(f => f));
    return () => {
      for (const u of unsubs) void u.then(f => f());
    };
  }, [refreshVersions, t, toast]);
  /// 行文案:已装 vX → 内嵌 vY;探测不到显示未探测到
  const verLine = (installed: string | null, embedded: string) =>
    `${installed ? t('settings.verInstalled', { v: installed }) : t('settings.verNotDetected')} → ${t('settings.verEmbedded', { v: embedded })}`;
  const verOverall = overallState(
    compareVersion(installedDaemon, manifest?.daemon ?? null),
    compareVersion(cliVersion, manifest?.cli ?? null),
  );
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
        </section>
        {/* 安装区(抽屉底部独立块):版本差异常驻两行 + 整体状态语 + 全宽主题色
            按钮;无内嵌资源(dev/未组包)整块不渲染(原 DaemonInstallRow 口径) */}
        {manifest && (
          <section className="mt-auto pt-4" data-testid="settings-install-block">
            <div className="mb-1.5 flex flex-col gap-0.5 px-1 text-xs font-mono text-[var(--muted)]">
              <span data-testid="settings-ver-daemon">
                planet-portd {verLine(installedDaemon, manifest.daemon)}
              </span>
              <span data-testid="settings-ver-cli">
                planet-port {verLine(cliVersion, manifest.cli)}
              </span>
              {verOverall !== 'silent' && (
                <span className="font-sans" data-testid="settings-version-state">
                  {t(verOverall === 'upgrade' ? 'settings.verStateUpgrade' : verOverall === 'compat' ? 'settings.verStateCompat' : 'settings.verStateEven')}
                </span>
              )}
            </div>
            <Button
              variant="primary"
              block
              data-testid="settings-install-daemon"
              onClick={() => setReinstallOpen(true)}
            >
              {daemonInstalled ? t('settings.reinstallDaemon') : t('settings.installDaemon')}
            </Button>
          </section>
        )}
      </DrawerShell>
      {/* 重装确认弹窗:manifest 存在才有安装入口,弹窗随之挂载(open 常关不占 DOM) */}
      {manifest && (
        <ReinstallDialog
          open={reinstallOpen}
          embeddedDaemon={manifest.daemon}
          installed={daemonInstalled}
          onClose={() => setReinstallOpen(false)}
          onDone={refreshVersions}
        />
      )}
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
