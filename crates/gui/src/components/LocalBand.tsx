import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getVersion } from '@tauri-apps/api/app';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import { detectOs } from '../platform';

function CopyBtn({ cmd }: { cmd: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="btn sm copy"
      onClick={() => {
        void navigator.clipboard
          .writeText(cmd)
          .then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          })
          .catch(() => {}); // 剪贴板权限被拒时静默,不打断
      }}
    >
      {copied ? t('guide.copied') : t('guide.copy')}
    </button>
  );
}

/// 首页底部工具带(方案 B):左「本机」概览(daemon/身份/版本/端口)+
/// 右「命令」速查(状态/日志/启停,行内复制;Windows 无 sudo 前缀)。
/// 查询键与 Home/IdentitySection 共用,只读缓存
export default function LocalBand() {
  const { t } = useTranslation();
  const os = useMemo(detectOs, []);
  const win = os === 'windows';
  const sudo = win ? '' : 'sudo ';
  const status = useQuery({ queryKey: ['status'], queryFn: api.status });
  const identity = useQuery({ queryKey: ['identity'], queryFn: api.identityGet });
  const [version, setVersion] = useState('');

  useEffect(() => {
    if (!version) void getVersion().then(setVersion).catch(() => setVersion('-'));
  }, [version]);

  const planets = status.data?.planets ?? [];
  const port = planets.find(p => p.enabled && p.status === 'running')?.port ?? planets[0]?.port;
  const cmds = [
    { label: t('guide.status'), cmd: 'planet-port service status' },
    { label: t('guide.logs'), cmd: 'planet-port service logs' },
    { label: t('guide.start'), cmd: `${sudo}planet-port service start` },
    { label: t('guide.stop'), cmd: `${sudo}planet-port service stop` },
  ];
  const mode = identity.data?.mode;

  return (
    <div className="band">
      <section>
        <div className="shead">
          <span className="sname">{t('home.thisMachine')}</span>
        </div>
        <div className="card band-card">
          <div className="kv">
            <span>daemon</span>
            <span className={`v-ok ${status.isError ? 'is-off' : ''}`}>
              <i className={`sdot ${status.isError ? 'off' : 'ok'}`} aria-hidden="true" />
              {status.isError ? t('daemon.offline') : t('daemon.online')}
            </span>
          </div>
          <div className="kv">
            <span>{t('home.identity')}</span>
            <span>{mode ? t(`identity.mode.${mode}`) : '—'}</span>
          </div>
          <div className="kv">
            <span>{t('home.version')}</span>
            <span className="mono">{version || '—'}</span>
          </div>
          <div className="kv">
            <span>{t('home.port')}</span>
            <span className="mono">{port ?? '—'}</span>
          </div>
        </div>
      </section>
      <section>
        <div className="shead">
          <span className="sname">{t('home.commands')}</span>
          <span className="grow" />
          <span className="snote">{t('home.cmdNote')}</span>
        </div>
        <div className="card band-card">
          {cmds.map(c => (
            <div className="cmd-row" key={c.cmd}>
              <span className="cmd-label">{c.label}</span>
              <span className="cmd-text">{c.cmd}</span>
              <CopyBtn cmd={c.cmd} />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
