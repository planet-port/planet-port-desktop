import { useTranslation } from 'react-i18next';
import DrawerShell from './DrawerShell';

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
  return (
    <DrawerShell open={open} onClose={onCancel} width={320} testId="confirm-dialog">
      <header>
        <h3>{title}</h3>
        <button className="btn icon" aria-label={t('common.close')} onClick={onCancel}>×</button>
      </header>
      <p className="dialog-message">{message}</p>
      <div className="dialog-actions">
        <button className="btn" onClick={onCancel}>
          {t('common.cancel')}
        </button>
        <button className={`btn ${danger ? 'danger' : 'primary'}`} onClick={onConfirm}>
          {confirmLabel ?? t('common.confirm')}
        </button>
      </div>
    </DrawerShell>
  );
}
