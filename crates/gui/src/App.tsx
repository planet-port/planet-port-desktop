import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from './api/client';
import logo from './assets/icon-1024.png';
import AddNetworkDrawer from './components/AddNetworkDrawer';
import AddPlanetDrawer from './components/AddPlanetDrawer';
import SettingsDrawer from './components/SettingsDrawer';
import GearIcon from './components/GearIcon';
import Statusbar from './components/Statusbar';
import { useToast } from './components/Toaster';
import Home from './pages/Home';
import OfflineGuide from './pages/OfflineGuide';
import { detectOs } from './platform';

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
      // 托盘「加入网络…」:打开添加网络抽屉(与首页空卡同一入口)
      listen<{ focusJoin?: boolean }>('navigate', e => {
        if (e.payload.focusJoin) setDrawer({ kind: 'addNet' });
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
    <div className="app app-single">
      {!online ? (
        <OfflineGuide installed={installed} />
      ) : (
        <main className="main">
          {/* 标题栏(方案 C 布局):[灯位留白] logo · 应用名 │ 在线 …… 动作组。
              非交互子元素同样标注 drag-region —— Tauri 只认 mousedown 目标元素 */}
          <header className={`topbar ${mac ? 'mac' : ''}`} data-tauri-drag-region>
            <img src={logo} alt="logo" className="app-logo" data-tauri-drag-region />
            <span className="app-title" data-tauri-drag-region>Planet Port</span>
            <span className="tb-sep" data-tauri-drag-region aria-hidden="true" />
            <span className="tb-online">
              <span className={`dot ${online ? 'on' : 'off'}`} />
              {online ? t('daemon.online') : t('daemon.offline')}
            </span>
            <span className="grow" data-tauri-drag-region />
            <button className="btn" data-testid="topbar-add-planet" onClick={() => setDrawer({ kind: 'addPlanet' })}>{t('home.addPlanet')}</button>
            <button className="btn icon" data-testid="topbar-settings" aria-label={t('settings.title')} onClick={() => setDrawer({ kind: 'settings' })}><GearIcon /></button>
          </header>
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
