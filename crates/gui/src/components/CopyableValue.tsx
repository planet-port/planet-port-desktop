import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy } from 'lucide-react';
import { cn } from '@/lib/utils';

/// 可复制值(语义同 iOS CopyableValue):mono 文本 + 常驻复制图标;点击复制后
/// 图标位置短暂变对勾(0.8s)作为反馈(不发 toast)。占位值(空/'-'/'—')不可
/// 复制且 hover 不亮图标,避免"看着能点、点了没反应"。
/// iconSize 分两档:详情行 12px、卡片紧凑行 10px(约为文字字高的 2/3)。
/// 旧 .copyable 系规则对译(根右对齐 28px 最高、gap 5px;值 span 跨行 break-all;
/// 图标 tertiary 色 hover 抬主色,占位值不挂 hover 抬亮——对应原 .netcard
/// .addr.is-blank 对图标的压制,由组件按 isPlaceholder 自持,调用方免传标记类)
export default function CopyableValue({
  text,
  small,
  className,
  title,
}: {
  text: string;
  small?: boolean;
  className?: string;
  title?: string;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  /// 连点重置计时:上一次 0.8s 定时未到期又复制时,以最近一次为准
  const timer = useRef<number | undefined>(undefined);
  const isPlaceholder = !text || text === '—' || text === '-';
  const copy = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isPlaceholder) return;
    void navigator.clipboard.writeText(text);
    setCopied(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 800);
  };
  const s = small ? 10 : 12;
  return (
    <button
      className={cn(
        'group inline-flex max-w-full min-h-7 items-center justify-end gap-[5px] border-0 bg-transparent px-0 py-0.5 text-right text-foreground cursor-pointer',
        className,
      )}
      onClick={copy}
      aria-label={t('common.copy')}
      title={title ?? (isPlaceholder ? undefined : t('common.copy'))}
    >
      <span className="min-w-0 break-all text-right font-mono">{text}</span>
      {(!isPlaceholder || copied) && copied ? (
        <Check
          size={s}
          strokeWidth={2.5}
          aria-hidden="true"
          className="shrink-0 text-[var(--ok)] transition-colors duration-[120ms] ease-[ease]"
        />
      ) : (
        <Copy
          size={s}
          strokeWidth={2}
          aria-hidden="true"
          className={cn(
            'shrink-0 text-[var(--faint)] transition-colors duration-[120ms] ease-[ease]',
            !isPlaceholder && 'group-hover:text-[var(--primary)]',
          )}
        />
      )}
    </button>
  );
}
