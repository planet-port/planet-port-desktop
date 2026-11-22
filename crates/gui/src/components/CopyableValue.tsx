import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

/// 可复制值(语义同 iOS CopyableValue,图标为 lucide 形态):mono 文本 + 常驻
/// 复制图标;点击复制后图标位置短暂变对勾(0.8s)作为反馈(不发 toast)。
/// 占位值(空/'-'/'—')不可复制,图标随之隐藏,避免"看着能点、点了没反应"。
/// iconSize 分两档:详情行 12px、卡片紧凑行 10px(约为文字字高的 2/3)。
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
      className={`copyable ${className ?? ''}`}
      onClick={copy}
      aria-label={t('guide.copy')}
      title={title ?? (isPlaceholder ? undefined : t('guide.copy'))}
    >
      <span className="mono">{text}</span>
      {(!isPlaceholder || copied) && copied ? (
        <svg width={s} height={s} viewBox="0 0 24 24" fill="none" stroke="var(--ok)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="copyable-icon">
          <path d="M20 6 9 17l-5-5" />
        </svg>
      ) : (
        <svg width={s} height={s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="copyable-icon">
          <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
          <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
        </svg>
      )}
    </button>
  );
}
