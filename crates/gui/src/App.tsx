import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from './api/client';
import logo from './assets/icon-1024.png';
import Home from './pages/Home';
import OfflineGuide from './pages/OfflineGuide';
import SettingsDrawer from './components/SettingsDrawer';
import { useToast } from './components/Toaster';

export default function App() {
  const { t } = useTranslation();
  const toast = useToast();
  const [online, setOnline] = useState(false);
  const [installed, setInstalled] = useState(true);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [focusJoin, setFocusJoin] = useState(0);

  useEffect(() => {
    void api.daemonState().then(s => {
      setOnline(s.online);
      setInstalled(s.installed);
    });
    const unsubs = [
      listen('daemon-online', () => setOnline(true)),
      listen('daemon-offline', () => setOnline(false)),
      // 托盘「加入网络…」:单页布局无切页,聚焦信号交 Home 聚焦加入框
      listen<{ focusJoin?: boolean }>('navigate', e => {
        if (e.payload.focusJoin) setFocusJoin(n => n + 1);
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
            <button className="btn icon" aria-label={t('settings.title')} onClick={() => setDrawerOpen(true)}>⚙</button>
          </header>
          <Home focusJoinSignal={focusJoin} />
        </main>
      )}
      {online && <SettingsDrawer open={drawerOpen} onClose={() => setDrawerOpen(false)} />}
    </div>
  );
}
