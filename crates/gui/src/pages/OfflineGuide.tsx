import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

/// 安装脚本走 api.github.com contents 通道(raw.githubusercontent.com 部分网络 TCP 直连被断,api 域可达)
const INSTALL_URL = 'https://api.github.com/repos/planet-port/planet-port-desktop/contents/scripts';

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
            void navigator.clipboard
              .writeText(cmd)
              .then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              })
              .catch(() => {}); // 剪贴板权限被拒时静默,不打断引导流程
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
    ? `Invoke-WebRequest -UseBasicParsing -Headers @{Accept='application/vnd.github.raw'} "${INSTALL_URL}/install.ps1?ref=main" -OutFile install.ps1\npowershell -ExecutionPolicy Bypass -File .\\install.ps1`
    : `curl -fsSL -H "Accept: application/vnd.github.raw" "${INSTALL_URL}/install.sh?ref=main" | sudo bash`;
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
