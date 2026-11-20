import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from './api/client';
import { useToast } from './components/Toaster';
import Networks from './pages/Networks';
import OfflineGuide from './pages/OfflineGuide';
import Overview from './pages/Overview';
import Peers from './pages/Peers';
import Planets from './pages/Planets';
import { switchLang } from './i18n';

export type Page = 'overview' | 'networks' | 'planets' | 'peers';

export default function App() {
  const { t, i18n } = useTranslation();
  const toast = useToast();
  const [page, setPage] = useState<Page>('overview');
  const [online, setOnline] = useState(false);
  const [installed, setInstalled] = useState(true);
  const [focusJoin, setFocusJoin] = useState(0);

  useEffect(() => {
    void api.daemonState().then(s => {
      setOnline(s.online);
      setInstalled(s.installed);
    });
    const unsubs = [
      listen('daemon-online', () => setOnline(true)),
      listen('daemon-offline', () => setOnline(false)),
      listen<{ to: Page; focusJoin?: boolean }>('navigate', e => {
        if (e.payload.focusJoin) setFocusJoin(n => n + 1);
        setPage(e.payload.to);
      }),
      // 托盘动作失败提示(Rust 侧在后台线程执行,失败经此回传)
      listen<string>('tray-error', e => toast.error(e.payload)),
    ].map(p => p.then(f => f));
    // 每个 unlisten 必须真正执行,否则重复挂载后监听泄漏
    return () => {
      for (const p of unsubs) void p.then(f => f());
    };
  }, [toast]);

  const body = !online ? (
    <OfflineGuide installed={installed} />
  ) : page === 'overview' ? (
    <Overview />
  ) : page === 'networks' ? (
    <Networks focusJoinSignal={focusJoin} />
  ) : page === 'planets' ? (
    <Planets />
  ) : (
    <Peers />
  );

  return (
    <div className="app">
      <aside className="side">
        <div className="brand">planet-port</div>
        <nav>
          {(['overview', 'networks', 'planets', 'peers'] as Page[]).map(p => (
            <button key={p} className={page === p ? 'active' : ''} onClick={() => setPage(p)}>
              {t(`nav.${p}`)}
            </button>
          ))}
        </nav>
        <div className="lang">
          <button className={i18n.language === 'zh' ? 'active' : ''} onClick={() => switchLang('zh')}>
            中
          </button>
          <button className={i18n.language === 'en' ? 'active' : ''} onClick={() => switchLang('en')}>
            EN
          </button>
        </div>
      </aside>
      <main className="main">
        <header className="topbar">
          <span className={`dot ${online ? 'on' : 'off'}`} />
          {online ? t('daemon.online') : t('daemon.offline')}
        </header>
        {body}
      </main>
    </div>
  );
}
