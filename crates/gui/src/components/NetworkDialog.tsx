import { useMemo, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import type { NetworkEntry } from '../api/types';
import QRShareSheet from './QRShareSheet';
import { useToast } from './Toaster';

/// 网络设置对话框：契约仅三字段(spec §2)——routeViaZeroTier / dnsMode / customDns
export default function NetworkDialog({ row, onClose, onDone }: { row: NetworkEntry; onClose: () => void; onDone: () => void }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [routeVia, setRouteVia] = useState(row.routeViaZeroTier);
  const [dnsMode, setDnsMode] = useState(row.dnsMode);
  const [customDns, setCustomDns] = useState(row.customDns.join('\n'));
  const [showQR, setShowQR] = useState(false);
  /// 网络二维码 = 网络 ID 纯文本(16 hex,与移动端扫码填 ID 互逆);
  /// data 身份须稳定——QRShareSheet 内部 useMemo 以 data 为依赖,内联编码会每渲染重置
  const qrData = useMemo(() => new TextEncoder().encode(row.nwid), [row.nwid]);
  const saveSettings = useMutation({
    mutationFn: () =>
      api.setNetwork(
        row.nwid,
        {
          routeViaZeroTier: routeVia,
          dnsMode,
          customDns: customDns.split('\n').map(s => s.trim()).filter(Boolean),
        },
        row.planet,
      ),
    onSuccess: () => {
      toast.ok(t('common.saved'));
      onDone();
      onClose();
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });
  return (
    <>
      <div className="overlay" onClick={onClose}>
        <div className="dialog" onClick={e => e.stopPropagation()}>
          <h3 className="mono">{t('networks.settingsTitle', { id: row.nwid })}</h3>
          <label className="field">
            <input
              type="checkbox"
              checked={routeVia}
              disabled={row.routeViaLocked}
              onChange={e => setRouteVia(e.target.checked)}
            />{' '}
            {t('networks.routeVia')}
            {row.routeViaLocked && <span className="small">（{t('networks.routeViaLocked')}）</span>}
          </label>
          <label className="field">
            {t('networks.dnsMode')}
            <select className="input" value={dnsMode} onChange={e => setDnsMode(e.target.value)}>
              <option value="none">none</option>
              <option value="network">network</option>
              <option value="custom">custom</option>
            </select>
          </label>
          <label className="field">
            {t('networks.customDns')}
            <textarea className="input" rows={3} value={customDns} onChange={e => setCustomDns(e.target.value)} placeholder="10.0.0.1" />
          </label>
          <div className="share-row">
            <span className="mono small">{row.nwid}</span>
            <button
              className="btn"
              onClick={() => {
                void navigator.clipboard.writeText(row.nwid);
                toast.ok(t('guide.copied'));
              }}
            >
              {t('networks.copyId')}
            </button>
            <button className="btn" onClick={() => setShowQR(true)}>{t('networks.qr')}</button>
          </div>
          <div className="dialog-actions">
            <button className="btn" onClick={onClose}>
              {t('common.cancel')}
            </button>
            <button className="btn primary" disabled={saveSettings.isPending} onClick={() => saveSettings.mutate()}>
              {t('common.save')}
            </button>
          </div>
        </div>
      </div>
      {/* QRShareSheet 与设置弹层的 overlay 平级:点二维码遮罩只收二维码,不冒泡关闭设置弹层 */}
      {showQR && (
        <QRShareSheet
          title={t('networks.qrTitle')}
          subtitle={row.name ?? row.nwid}
          data={qrData}
          onDismiss={() => setShowQR(false)}
          onSave={async dataUrl => {
            const path = await save({ defaultPath: `${row.nwid}.png` });
            if (!path) return;
            // dataURL → bytes → write_file_bytes(base64 部分经 atob 解码)
            const b64 = dataUrl.slice(dataUrl.indexOf(',') + 1);
            const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
            await api.writeBytes(path, bytes);
            toast.ok(t('common.saved'));
          }}
        />
      )}
    </>
  );
}
