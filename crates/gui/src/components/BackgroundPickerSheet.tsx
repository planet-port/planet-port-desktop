import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import { backgroundUrl, getBackground, setBackground } from '../background';
import { useToast } from './Toaster';
import { Dialog, DialogContent, DialogTitle } from './ui-x/Dialog';
import { Button } from './ui-x/Button';

/// 背景图选择弹层(iOS BackgroundImagePickerSheet 对位):预览当前背景,
/// 选图(复制进 appData 后即刻生效)/清除。遮罩点击/Esc 关闭,无右上角关闭钮
/// (基线 overlay 形态)。容器迁 shadcn Dialog(ui-x/Dialog):瞬时型时序——
/// 内部持有 open,关闭走退出动画后再回调父层卸载。
/// 缩略图样式对译 styles.css .bg-thumb(本组件独占,迁完规则零引用):
/// 全宽 120px 高 cover 裁切 + 1px 边 6px 圆角
export default function BackgroundPickerSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [url, setUrl] = useState(() => backgroundUrl());
  const [busy, setBusy] = useState(false);
  const choose = async () => {
    const path = await openFileDialog({
      multiple: false,
      filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
    });
    if (!path) return;
    setBusy(true);
    try {
      const dest = await api.backgroundSet(path);
      setBackground(dest);
      setUrl(backgroundUrl());
      toast.ok(t('common.saved'));
    } catch (e) {
      toast.error(asError(e)?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };
  const clear = async () => {
    setBusy(true);
    try {
      await api.backgroundClear();
      setBackground(null);
      setUrl(null);
    } catch (e) {
      toast.error(asError(e)?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={o => { if (!o) close(); }} onClosed={onClose}>
      <DialogContent>
        <DialogTitle>{t('settings.background')}</DialogTitle>
        {url ? (
          <img
            className="block h-[120px] w-full rounded-[6px] border border-border object-cover"
            src={url}
            alt={t('settings.background')}
          />
        ) : (
          <div className="kv"><span>{t('common.notSet')}</span></div>
        )}
        <div className="mt-4 flex items-center justify-end gap-2">
          {getBackground() && (
            <Button variant="danger" disabled={busy} onClick={() => void clear()}>{t('settings.bgClear')}</Button>
          )}
          <span className="flex-1" />
          <Button variant="primary" disabled={busy} onClick={() => void choose()}>{t('settings.bgPick')}</Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
