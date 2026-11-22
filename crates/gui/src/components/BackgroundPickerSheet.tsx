import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import { backgroundUrl, getBackground, setBackground } from '../background';
import { useToast } from './Toaster';

/// 背景图选择弹层(iOS BackgroundImagePickerSheet 对位):预览当前背景,
/// 选图(复制进 appData 后即刻生效)/清除。遮罩点击关闭
export default function BackgroundPickerSheet({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [url, setUrl] = useState(() => backgroundUrl());
  const [busy, setBusy] = useState(false);
  const choose = async () => {
    const path = await open({
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
    <div className="overlay" onClick={onClose}>
      <div className="dialog" onClick={e => e.stopPropagation()}>
        <h3>{t('settings.background')}</h3>
        {url ? (
          <img className="bg-thumb" src={url} alt={t('settings.background')} />
        ) : (
          <div className="kv"><span>{t('common.notSet')}</span></div>
        )}
        <div className="dialog-actions">
          {getBackground() && (
            <button className="btn danger" disabled={busy} onClick={() => void clear()}>{t('settings.bgClear')}</button>
          )}
          <span className="grow" />
          <button className="btn primary" disabled={busy} onClick={() => void choose()}>{t('settings.bgPick')}</button>
        </div>
      </div>
    </div>
  );
}
