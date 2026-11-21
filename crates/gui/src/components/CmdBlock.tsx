import { useState } from 'react';
import { useTranslation } from 'react-i18next';

/// 命令展示块(带复制按钮):离线引导与安装失败日志指引共用
export default function CmdBlock({ label, cmd }: { label: string; cmd: string }) {
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
