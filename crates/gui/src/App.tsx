import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { Settings } from 'lucide-react';
import { api } from './api/client';
import logo from './assets/icon-1024.png';
import AddNetworkDrawer from './components/AddNetworkDrawer';
import AddPlanetDrawer from './components/AddPlanetDrawer';
import { FAIL_REASON_KEYS } from './components/ReinstallDialog';
import SettingsDrawer from './components/SettingsDrawer';
import Statusbar from './components/Statusbar';
import { useToast } from './components/Toaster';
import UpdateDialog, { type UpdatePhase } from './components/UpdateDialog';
import Home from './pages/Home';
import { compareVersion, overallState, versionLt } from './lib/version';
import { detectOs } from './platform';
import { cn } from '@/lib/utils';

/// 顶栏抽屉状态机:三入口(添加网络/添加行星/设置)同一时刻仅开一个。
/// 添加网络入口已从顶栏按钮移至首页卡片尾的空卡,planet 为空 = 默认行星
type DrawerState =
  | { kind: 'none' }
  | { kind: 'addNet'; planet?: string }
  | { kind: 'addPlanet' }
  | { kind: 'settings' };

export default function App() {
  const { t } = useTranslation();
  const toast = useToast();
  const [online, setOnline] = useState(false);
  const [drawer, setDrawer] = useState<DrawerState>({ kind: 'none' });
  /// macOS 红绿灯悬浮于 webview(Overlay 标题栏),顶栏需留出左侧灯位;
  /// Windows/Linux 保持原生标题栏,不加留位
  const mac = useMemo(() => detectOs() === 'macos', []);

  /// 启动一次性版本差异探测:内嵌 manifest vs 磁盘已装(spawn 探测)。
  /// 被动化口径:只点亮状态栏右下角「可升级」,不弹 toast(用户定稿);
  /// compat(已装新于内嵌)无动作,详细信息在设置抽屉版本行/图标呈现
  const versionChecked = useRef(false);
  const [upgradable, setUpgradable] = useState(false);
  useEffect(() => {
    if (versionChecked.current) return;
    void Promise.all([api.embeddedManifest(), api.installedDaemonVersion(), api.cliVersion()])
      .then(([manifest, instDaemon, instCli]) => {
        versionChecked.current = true;
        if (!manifest) return; // dev/未组包无内嵌资源,无从比对
        setUpgradable(
          overallState(compareVersion(instDaemon, manifest.daemon), compareVersion(instCli, manifest.cli)) === 'upgrade',
        );
      })
      // 单项探测 reject 视为整体失败静默跳过:状态栏保持不提示
      .catch(() => {});
  }, []);

  /// GUI 更新检查(手动触发,无静默轮询):phase 与结果都在 App,弹窗受控;
  /// 查得新版同时点亮关于区 gui 行升级图标(guiUpdate 会话内状态,不持久化)
  const [updateDlg, setUpdateDlg] = useState<
    { phase: UpdatePhase; current: string; latest?: string; error?: string } | null
  >(null);
  const [guiUpdate, setGuiUpdate] = useState<string | null>(null);
  const runUpdateCheck = useCallback(() => {
    void getVersion().then(current => {
      setUpdateDlg({ phase: 'checking', current });
      api.checkGuiUpdate()
        .then(latest => {
          if (versionLt(current, latest)) {
            setGuiUpdate(latest);
            setUpdateDlg({ phase: 'found', current, latest });
          } else {
            setUpdateDlg({ phase: 'latest', current });
          }
        })
        .catch((e: unknown) => setUpdateDlg({ phase: 'failed', current, error: String(e) }));
    });
  }, []);

  useEffect(() => {
    void api.daemonState().then(s => setOnline(s.online));
    const unsubs = [
      listen('daemon-online', () => setOnline(true)),
      listen('daemon-offline', () => setOnline(false)),
      // 托盘导航:「+ 加入网络…」带行星维度预选;「加入行星」开添加行星抽屉
      listen<{ focusJoin?: boolean; planet?: string; addPlanet?: boolean }>('navigate', e => {
        if (e.payload.addPlanet) setDrawer({ kind: 'addPlanet' });
        else if (e.payload.focusJoin) setDrawer({ kind: 'addNet', planet: e.payload.planet });
      }),
      // 托盘动作失败提示(Rust 侧在后台线程执行,失败经此回传)
      listen<string>('tray-error', e => toast.error(e.payload)),
      // 安装结果兜底 toast 常驻 App 层:设置抽屉开着时弹窗自显成败,双提示并存
      // 是设计预期;抽屉已关(取消等待)后安装落地仍有回报。失败原因映射同
      // ReinstallDialog,表外原因(含提权前 rejection)兜底 failGeneric
      // 装完即置 false:磁盘此刻必等于内嵌,否则抽屉箭头已灭而状态栏滞留可升级同屏矛盾
      listen('install-done', () => {
        setUpgradable(false);
        toast.ok(t('settings.installDone'));
      }),
      listen<string>('install-failed', e =>
        toast.error(t(FAIL_REASON_KEYS[String(e.payload)] ?? 'settings.failGeneric'))),
      // 托盘「检查更新」:Rust 侧已先 show_main,这里只触发检查流程(结果弹窗)
      listen('check-update', () => runUpdateCheck()),
    ].map(p => p.then(f => f));
    // 每个 unlisten 必须真正执行,否则重复挂载后监听泄漏
    return () => {
      for (const p of unsubs) void p.then(f => f());
    };
  }, [toast, t, runUpdateCheck]);

  return (
    <div className="flex h-screen flex-col">
      {/* 标题栏(方案 C 布局):[灯位留白] logo · 应用名 │ daemon 状态 …… 动作组。
          在线/离线两态共用——首页离线只禁用不换屏。非交互子元素同样标注
          drag-region —— Tauri 只认 mousedown 目标元素。添加行星离线禁用;
          设置常显(安装入口在其中);旧 .topbar 系规则对译:52px 恒定高与
          --bg 同底 + hairline 下边、mac 96px 灯位、动作钮透明底 hover 显面 */}
      <header
        className={cn(
          'flex h-[52px] shrink-0 items-center justify-start gap-2.5 border-b border-[var(--border-soft)] bg-background pr-3.5 pl-5 text-muted-foreground',
          mac && 'pl-24',
        )}
        data-tauri-drag-region
      >
        <img src={logo} alt="logo" className="h-[22px] w-[22px] flex-none rounded-[5px]" data-tauri-drag-region />
        <span className="whitespace-nowrap text-[15px] font-semibold tracking-[0.2px] text-foreground" data-tauri-drag-region>
          Planet Port
        </span>
        <span className="ml-4 h-4 w-px flex-none bg-[var(--border-h)]" data-tauri-drag-region aria-hidden="true" />
        <span className="inline-flex items-center gap-[7px] text-xs text-muted-foreground" data-tauri-drag-region>
          <span
            className={`inline-block h-1.5 w-1.5 flex-none rounded-[2px] ${
              online ? 'bg-[var(--ok)]' : 'bg-[var(--off)]'
            }`}
          />
          {online ? t('daemon.online') : t('daemon.offline')}
        </span>
        <span className="flex-[1_1_auto]" data-tauri-drag-region />
        <button
          className="inline-flex min-h-7 cursor-pointer items-center gap-1.5 rounded-[6px] border-none bg-transparent px-2.5 text-[13px] text-muted-foreground transition-[background-color,color] duration-[120ms] ease-[ease] disabled:cursor-default disabled:opacity-70 enabled:hover:bg-card enabled:hover:text-foreground focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1"
          data-testid="topbar-add-planet"
          disabled={!online}
          onClick={() => setDrawer({ kind: 'addPlanet' })}
        >
          {t('home.addPlanet')}
        </button>
        <button
          className="inline-flex min-h-7 min-w-7 cursor-pointer items-center justify-center rounded-[6px] border-none bg-transparent p-0 text-muted-foreground transition-[background-color,color] duration-[120ms] ease-[ease] enabled:hover:bg-card enabled:hover:text-foreground focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1"
          data-testid="topbar-settings"
          aria-label={t('settings.title')}
          onClick={() => setDrawer({ kind: 'settings' })}
        >
          <Settings size={14} strokeWidth={1.6} aria-hidden="true" />
        </button>
      </header>
      <main className="flex min-w-0 flex-1 flex-col">
        <Home online={online} onAddNetwork={planet => setDrawer({ kind: 'addNet', planet })} />
        <Statusbar upgradable={upgradable} />
      </main>
      {/* 三个抽屉均为瞬时型:内部自持 open 走滑出动画,关动画结束才回调
          onClose 清 drawer 状态,故这里条件渲染保持不变;settings 离线也可开
          (安装能力在内),addNet/addPlanet 依赖 daemon 在线 */}
      {online && drawer.kind === 'addNet' && (
        <AddNetworkDrawer defaultPlanet={drawer.planet} onClose={() => setDrawer({ kind: 'none' })} />
      )}
      {online && drawer.kind === 'addPlanet' && <AddPlanetDrawer onClose={() => setDrawer({ kind: 'none' })} />}
      {drawer.kind === 'settings' && (
        <SettingsDrawer
          onClose={() => setDrawer({ kind: 'none' })}
          guiUpdate={guiUpdate}
          checking={updateDlg?.phase === 'checking'}
          onCheckUpdate={runUpdateCheck}
        />
      )}
      {/* 检查更新弹窗:updateDlg 非空即开(检查中就先开,checking 态弹窗内自显) */}
      {updateDlg && (
        <UpdateDialog
          open
          phase={updateDlg.phase}
          current={updateDlg.current}
          latest={updateDlg.latest}
          error={updateDlg.error}
          onClose={() => setUpdateDlg(null)}
          onRetry={runUpdateCheck}
        />
      )}
    </div>
  );
}
