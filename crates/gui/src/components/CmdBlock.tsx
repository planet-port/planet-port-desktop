import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from './ui-x/Button';

/// 命令展示块(带复制按钮):命令/日志指引场景共用的独立展示件
/// (当前暂无挂载方,保留供安装失败日志指引等后续场景复用)。
/// 旧 .cmd/.cmd-head/.cmd-body 规则对译:14px 上下距、头行 12px 灰字、
/// 终端块主题不变暗面(#101216/#262a33/#e7e9ec)+ mono 12px/1.6 + 可选中
export default function CmdBlock({ label, cmd }: { label: string; cmd: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <div className="my-3.5">
      <div className="mb-1 flex items-center justify-between text-xs text-muted-foreground">
        <span>{label}</span>
        <Button
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
          {copied ? t('common.copied') : t('common.copy')}
        </Button>
      </div>
      <pre className="m-0 select-text whitespace-pre-wrap break-all rounded-[6px] border border-[#262a33] bg-[#101216] px-3 py-2.5 font-mono text-xs leading-[1.6] text-[#e7e9ec]">
        {cmd}
      </pre>
    </div>
  );
}
