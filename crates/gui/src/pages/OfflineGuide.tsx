import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

const INSTALL_URL = 'https://raw.githubusercontent.com/planet-port/planet-port-desktop/main';

function detectOs(): 'macos' | 'windows' | 'linux' {
  const ua = navigator.userAgent;
  if (ua.includes('Windows')) return 'windows';
  if (ua.includes('Mac')) return 'macos';
  return 'linux';
}

function CmdBlock({ label, cmd }: { label: string; cmd: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <div className="cmd">
      <div className="cmd-head">
        <span>{label}</span>
        <button
          className="btn"
          onClick={() => {
            void navigator.clipboard.writeText(cmd).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            });
          }}
        >
          {copied ? t('guide.copied') : t('guide.copy')}
        </button>
      </div>
      <pre className="cmd-body">{cmd}</pre>
    </div>
  );
}

export default function OfflineGuide({ installed }: { installed: boolean }) {
  const { t } = useTranslation();
  const os = useMemo(detectOs, []);
  const win = os === 'windows';
  const sudo = win ? '' : 'sudo ';
  const installCmd = win
    ? `Invoke-WebRequest -UseBasicParsing ${INSTALL_URL}/scripts/install.ps1 -OutFile install.ps1\npowershell -ExecutionPolicy Bypass -File .\\install.ps1`
    : `curl -fsSL ${INSTALL_URL}/scripts/install.sh | sudo bash`;
  return (
    <div className="guide">
      <h2>{installed ? t('guide.titleInstalled') : t('guide.titleNotInstalled')}</h2>
      <p className="muted">{win ? t('guide.windowsNote') : t('guide.unixNote')}</p>
      <CmdBlock label={installed ? t('guide.start') : t('guide.hintNotInstalled')} cmd={installed ? `${sudo}planet-port service start` : installCmd} />
      <div className="cmd-grid">
        <CmdBlock label={t('guide.status')} cmd={`planet-port service status`} />
        <CmdBlock label={t('guide.logs')} cmd={`planet-port service logs`} />
        <CmdBlock label={t('guide.stop')} cmd={`${sudo}planet-port service stop`} />
      </div>
      <p className="muted">{t('guide.retried')}</p>
    </div>
  );
}
