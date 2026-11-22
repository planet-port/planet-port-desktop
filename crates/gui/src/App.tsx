import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from './api/client';
import logo from './assets/icon-1024.png';
import AddNetworkDrawer from './components/AddNetworkDrawer';
import AddPlanetDrawer from './components/AddPlanetDrawer';
import SettingsDrawer from './components/SettingsDrawer';
import { useToast } from './components/Toaster';
import Home from './pages/Home';
import OfflineGuide from './pages/OfflineGuide';

/// 顶栏抽屉状态机:三入口(添加网络/添加行星/设置)同一时刻仅开一个
type DrawerKind = 'none' | 'addNet' | 'addPlanet' | 'settings';

export default function App() {
  const { t } = useTranslation();
  const toast = useToast();
  const [online, setOnline] = useState(false);
  const [installed, setInstalled] = useState(true);
  const [drawer, setDrawer] = useState<DrawerKind>('none');

  useEffect(() => {
    void api.daemonState().then(s => {
      setOnline(s.online);
      setInstalled(s.installed);
    });
    const unsubs = [
      listen('daemon-online', () => setOnline(true)),
      listen('daemon-offline', () => setOnline(false)),
      // 托盘「加入网络…」:打开添加网络抽屉(与顶栏按钮同一入口)
      listen<{ focusJoin?: boolean }>('navigate', e => {
        if (e.payload.focusJoin) setDrawer('addNet');
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
          <header className="topbar">
            <img src={logo} alt="logo" className="app-logo" />
            <span className="app-title">Planet Port</span>
            <span className={`dot ${online ? 'on' : 'off'}`} />
            <span className="small">{online ? t('daemon.online') : t('daemon.offline')}</span>
            <span className="grow" />
            <button className="btn" data-testid="topbar-add-network" onClick={() => setDrawer('addNet')}>{t('home.addNetwork')}</button>
            <button className="btn" data-testid="topbar-add-planet" onClick={() => setDrawer('addPlanet')}>{t('home.addPlanet')}</button>
            <button className="btn icon" data-testid="topbar-settings" aria-label={t('settings.title')} onClick={() => setDrawer('settings')}>⚙</button>
          </header>
          <Home />
        </main>
      )}
      {/* 三个抽屉均为瞬时型:内部自持 open 走滑出动画,关动画结束才回调
          onClose 清 drawer 状态,故这里条件渲染保持不变 */}
      {online && drawer === 'addNet' && <AddNetworkDrawer onClose={() => setDrawer('none')} />}
      {online && drawer === 'addPlanet' && <AddPlanetDrawer onClose={() => setDrawer('none')} />}
      {online && drawer === 'settings' && <SettingsDrawer onClose={() => setDrawer('none')} />}
    </div>
  );
}
