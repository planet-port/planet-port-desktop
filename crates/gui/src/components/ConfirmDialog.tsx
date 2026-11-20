import { useTranslation } from 'react-i18next';

interface Props {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/// 应用内二次确认对话框。wry 在 macOS/Linux 未实现 JS 原生对话框,
/// window.confirm 恒 falsy——所有破坏性操作必须走此组件。
export default function ConfirmDialog({ open, title, message, confirmLabel, danger, onConfirm, onCancel }: Props) {
  const { t } = useTranslation();
  if (!open) return null;
  return (
    <div className="overlay" onClick={onCancel}>
      <div className="dialog confirm" onClick={e => e.stopPropagation()}>
        <h3>{title}</h3>
        <p className="dialog-message">{message}</p>
        <div className="dialog-actions">
          <button className="btn" onClick={onCancel}>
            {t('common.cancel')}
          </button>
          <button className={`btn ${danger ? 'danger' : 'primary'}`} onClick={onConfirm}>
            {confirmLabel ?? t('common.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
