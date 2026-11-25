import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { Settings } from 'lucide-react';
import { api } from './api/client';
import logo from './assets/icon-1024.png';
import AddNetworkDrawer from './components/AddNetworkDrawer';
import AddPlanetDrawer from './components/AddPlanetDrawer';
import SettingsDrawer from './components/SettingsDrawer';
import Statusbar from './components/Statusbar';
import { useToast } from './components/Toaster';
import Home from './pages/Home';
import OfflineGuide from './pages/OfflineGuide';
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
  const [installed, setInstalled] = useState(true);
  const [drawer, setDrawer] = useState<DrawerState>({ kind: 'none' });
  /// macOS 红绿灯悬浮于 webview(Overlay 标题栏),顶栏需留出左侧灯位;
  /// Windows/Linux 保持原生标题栏,不加留位
  const mac = useMemo(() => detectOs() === 'macos', []);

  useEffect(() => {
    void api.daemonState().then(s => {
      setOnline(s.online);
      setInstalled(s.installed);
    });
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
    ].map(p => p.then(f => f));
    // 每个 unlisten 必须真正执行,否则重复挂载后监听泄漏
    return () => {
      for (const p of unsubs) void p.then(f => f());
    };
  }, [toast]);

  return (
    <div className="flex h-screen flex-col">
      {/* 标题栏(方案 C 布局):[灯位留白] logo · 应用名 │ daemon 状态 …… 动作组。
          在线/离线引导两分支共用——引导页此前整页裸奔:无品牌层、无拖拽区、
          mac 红绿灯悬浮叠在内容上。非交互子元素同样标注 drag-region ——
          Tauri 只认 mousedown 目标元素。动作组仅在在线分支显示(设置/添加
          行星都依赖 daemon);旧 .topbar 系规则对译:52px 恒定高与 --bg 同底
          + hairline 下边、mac 96px 灯位、动作钮透明底 hover 显面 */}
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
        {online && (
          <>
            <button
              className="inline-flex min-h-7 cursor-pointer items-center gap-1.5 rounded-[6px] border-none bg-transparent px-2.5 text-[13px] text-muted-foreground transition-[background-color,color] duration-[120ms] ease-[ease] enabled:hover:bg-card enabled:hover:text-foreground focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1"
              data-testid="topbar-add-planet"
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
          </>
        )}
      </header>
      {!online ? (
        <OfflineGuide installed={installed} />
      ) : (
        <main className="flex min-w-0 flex-1 flex-col">
          <Home onAddNetwork={planet => setDrawer({ kind: 'addNet', planet })} />
          <Statusbar />
        </main>
      )}
      {/* 三个抽屉均为瞬时型:内部自持 open 走滑出动画,关动画结束才回调
          onClose 清 drawer 状态,故这里条件渲染保持不变 */}
      {online && drawer.kind === 'addNet' && (
        <AddNetworkDrawer defaultPlanet={drawer.planet} onClose={() => setDrawer({ kind: 'none' })} />
      )}
      {online && drawer.kind === 'addPlanet' && <AddPlanetDrawer onClose={() => setDrawer({ kind: 'none' })} />}
      {online && drawer.kind === 'settings' && <SettingsDrawer onClose={() => setDrawer({ kind: 'none' })} />}
    </div>
  );
}
