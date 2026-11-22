import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import type { NetworkEntry } from '../api/types';
import { useToast } from './Toaster';

/// 网络设置对话框：契约仅三字段(spec §2)——routeViaZeroTier / dnsMode / customDns
export default function NetworkDialog({ row, onClose, onDone }: { row: NetworkEntry; onClose: () => void; onDone: () => void }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [routeVia, setRouteVia] = useState(row.routeViaZeroTier);
  const [dnsMode, setDnsMode] = useState(row.dnsMode);
  const [customDns, setCustomDns] = useState(row.customDns.join('\n'));
  const save = useMutation({
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
        <div className="dialog-actions">
          <button className="btn" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button className="btn primary" disabled={save.isPending} onClick={() => save.mutate()}>
            {t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}
