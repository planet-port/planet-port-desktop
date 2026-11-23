import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { getVersion } from '@tauri-apps/api/app';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import { detectOs } from '../platform';
import { Button } from './ui-x/Button';

/// 首页底部工具带(方案 B):左「本机」概览(daemon/身份/版本/端口)+
/// 右「命令」速查(状态/日志/启停,行内复制;Windows 无 sudo 前缀)。
/// 查询键与 Home/IdentitySection 共用,只读缓存。
/// 旧 .band/.shead/.card/.band-card/.kv/.v-ok/.cmd-* 规则对译:双列 16px 间距、
/// 组头 32px、卡面 --panel + hairline、行高 34px、命令行 mono 12px 省略号
function CopyBtn({ cmd }: { cmd: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <Button
      size="sm"
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
    </Button>
  );
}

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
    <div className="mt-[26px] grid grid-cols-2 gap-4">
      <section>
        <div className="mb-2.5 flex h-8 items-center gap-2.5 px-1.5">
          <span className="text-[13px] font-medium text-foreground">{t('home.thisMachine')}</span>
        </div>
        <div className="flex flex-col rounded-md border border-border bg-card px-4 py-1.5">
          <div className="flex min-h-[34px] items-center justify-between gap-3 py-1.5 text-[13px]">
            <span className="flex-none">daemon</span>
            <span className={`inline-flex items-center gap-1.5 ${status.isError ? 'text-muted-foreground' : ''}`}>
              <i
                className={`h-2 w-2 flex-none rounded-[2px] ${
                  status.isError ? 'bg-[var(--off)]' : 'bg-[var(--lamp)] shadow-[0_0_4px_var(--lamp)]'
                }`}
                aria-hidden="true"
              />
              {status.isError ? t('daemon.offline') : t('daemon.online')}
            </span>
          </div>
          <div className="flex min-h-[34px] items-center justify-between gap-3 py-1.5 text-[13px]">
            <span className="flex-none">{t('home.identity')}</span>
            <span>{mode ? t(`identity.mode.${mode}`) : '—'}</span>
          </div>
          <div className="flex min-h-[34px] items-center justify-between gap-3 py-1.5 text-[13px]">
            <span className="flex-none">{t('home.version')}</span>
            <span className="break-all text-right font-mono">{version || '—'}</span>
          </div>
          <div className="flex min-h-[34px] items-center justify-between gap-3 py-1.5 text-[13px]">
            <span className="flex-none">{t('home.port')}</span>
            <span className="break-all text-right font-mono">{port ?? '—'}</span>
          </div>
        </div>
      </section>
      <section>
        <div className="mb-2.5 flex h-8 items-center gap-2.5 px-1.5">
          <span className="text-[13px] font-medium text-foreground">{t('home.commands')}</span>
          <span className="flex-[1_1_auto]" />
          <span className="text-xs text-muted-foreground">{t('home.cmdNote')}</span>
        </div>
        <div className="flex flex-col rounded-md border border-border bg-card px-4 py-1.5">
          {cmds.map(c => (
            <div className="flex min-h-[34px] items-center gap-3" key={c.cmd}>
              <span className="w-16 flex-none text-xs text-muted-foreground">{c.label}</span>
              <span className="min-w-0 flex-1 select-text overflow-hidden font-mono text-xs leading-none tracking-[0.1px] text-ellipsis whitespace-nowrap text-foreground">
                {c.cmd}
              </span>
              <CopyBtn cmd={c.cmd} />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
