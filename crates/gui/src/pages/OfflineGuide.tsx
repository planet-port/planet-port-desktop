import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import type { EmbeddedManifest } from '../api/types';
import CmdBlock from '../components/CmdBlock';
import { detectOs } from '../platform';
import { needInstall } from './setup';
import SetupWizard from './SetupWizard';

/// 安装脚本走 api.github.com contents 通道(raw.githubusercontent.com 部分网络 TCP 直连被断,api 域可达)
const INSTALL_URL = 'https://api.github.com/repos/planet-port/planet-port-desktop/contents/scripts';

/// 离线引导双态:bundle 内嵌安装资源且需要安装/升级 → 自动安装区置顶(手动命令折叠为后备);
/// 无内嵌资源(dev 或未组包)→ 维持纯手动引导。
/// 旧 .guide 系规则对译:根自承滚动(视口已锁死)+ 720px 上限、h2 15px/500、
/// details 10px 上距、summary 手型、灰注文字、命令双列网格 320px auto-fill
export default function OfflineGuide({ installed }: { installed: boolean }) {
  const { t } = useTranslation();
  const os = useMemo(detectOs, []);
  const win = os === 'windows';
  const sudo = win ? '' : 'sudo ';
  const installCmd = win
    ? `Invoke-WebRequest -UseBasicParsing -Headers @{Accept='application/vnd.github.raw'} "${INSTALL_URL}/install.ps1?ref=main" -OutFile install.ps1\npowershell -ExecutionPolicy Bypass -File .\\install.ps1`
    : `curl -fsSL -H "Accept: application/vnd.github.raw" "${INSTALL_URL}/install.sh?ref=main" | sudo bash`;

  // v1 不预读已装版本号(installedVersion 固定 null):已装场景一律按幂等升级展示
  const [manifest, setManifest] = useState<EmbeddedManifest | null>(null);
  useEffect(() => {
    // 读取失败按"无内嵌资源"处理,退回手动引导
    void api.embeddedManifest().then(setManifest).catch(() => {});
  }, []);
  const auto = needInstall(manifest, installed, null);

  const manual = (
    <>
      <h2 className="mb-1 mt-2 text-[15px] font-medium">
        {installed ? t('guide.titleInstalled') : t('guide.titleNotInstalled')}
      </h2>
      <p className="text-muted-foreground">{win ? t('guide.windowsNote') : t('guide.unixNote')}</p>
      <CmdBlock
        label={installed ? t('guide.start') : t('guide.hintNotInstalled')}
        cmd={installed ? `${sudo}planet-port service start` : installCmd}
      />
      <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-x-[18px] gap-y-1">
        <CmdBlock label={t('guide.status')} cmd={`planet-port service status`} />
        <CmdBlock label={t('guide.logs')} cmd={`planet-port service logs`} />
        <CmdBlock label={t('guide.stop')} cmd={`${sudo}planet-port service stop`} />
      </div>
      <p className="text-muted-foreground">{t('guide.retried')}</p>
    </>
  );

  if (!auto || manifest === null)
    return <div className="max-w-[720px] flex-1 overflow-y-auto">{manual}</div>;
  return (
    <div className="max-w-[720px] flex-1 overflow-y-auto">
      <SetupWizard manifest={manifest} installed={installed} sudo={sudo} />
      <details className="mt-2.5">
        <summary className="cursor-pointer text-muted-foreground">{t('guide.manualTitle')}</summary>
        {manual}
      </details>
    </div>
  );
}
