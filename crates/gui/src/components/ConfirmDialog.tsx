import { useTranslation } from 'react-i18next';
import { AlertDialog, AlertDialogContent, AlertDialogTitle } from './ui-x/AlertDialog';
import { Button } from './ui-x/Button';

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
/// 容器迁 shadcn AlertDialog(ui-x/AlertDialog,role=alertdialog + 焦点圈定):
/// 遮罩点击与 Esc → onCancel;danger/primary 语义经 ui-x/Button variant 保留;
/// 宽 380px 对译 .dialog.confirm;正文/按钮区间距对译 .dialog-message(行高 1.6)
/// 与 .dialog-actions(右对齐 gap 8px 上距 16px)。确认按钮不接 Radix Close——
/// 语义是「等异步完成后才由调用方翻 open」,点击即关会绕过失败回滚路径。
export default function ConfirmDialog({ open, title, message, confirmLabel, danger, onConfirm, onCancel }: Props) {
  const { t } = useTranslation();
  return (
    <AlertDialog open={open} onOpenChange={o => { if (!o) onCancel(); }}>
      <AlertDialogContent testId="confirm-dialog" className="w-[380px]">
        <div className="flex items-center justify-between gap-2">
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <Button variant="icon" aria-label={t('common.close')} onClick={onCancel}>×</Button>
        </div>
        <p className="m-0 leading-[1.6] break-words">{message}</p>
        <div className="mt-4 flex justify-end gap-2">
          <Button onClick={onCancel}>
            {t('common.cancel')}
          </Button>
          <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm}>
            {confirmLabel ?? t('common.confirm')}
          </Button>
        </div>
      </AlertDialogContent>
    </AlertDialog>
  );
}
