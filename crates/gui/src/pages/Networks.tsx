import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import type { NetworkEntry } from '../api/types';
import { useToast } from '../components/Toaster';

export default function Networks({ focusJoinSignal }: { focusJoinSignal: number }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [nwid, setNwid] = useState('');
  const [planetFilter, setPlanetFilter] = useState('');
  const [editing, setEditing] = useState<NetworkEntry | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // 复用概览的 status 查询(同 key 共享缓存)，仅用于 planet 过滤下拉
  const status = useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 3000 });
  const planets = status.data?.planets ?? [];
  const q = useQuery({
    queryKey: ['networks', planetFilter],
    queryFn: () => api.networks(planetFilter || undefined),
    refetchInterval: 5000,
  });
  const rows = q.data?.networks ?? [];

  // 托盘「加入网络…」跳转聚焦
  useEffect(() => {
    if (focusJoinSignal > 0) inputRef.current?.focus();
  }, [focusJoinSignal]);

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['networks'] });
    void qc.invalidateQueries({ queryKey: ['status'] });
  };
  const onErr = (e: unknown) => {
    const ge = asError(e);
    const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
    toast.error(`${ge?.message ?? String(e)}${cand}`);
  };

  const join = useMutation({
    mutationFn: () => api.joinNetwork(nwid.trim(), planetFilter || undefined),
    onSuccess: () => {
      setNwid('');
      invalidate();
      toast.ok(t('networks.joined'));
    },
    onError: onErr,
  });
  const leave = useMutation({
    mutationFn: (row: NetworkEntry) => api.leaveNetwork(row.nwid, row.planet),
    onSuccess: invalidate,
    onError: onErr,
  });

  return (
    <div className="page">
      <div className="toolbar">
        <input
          ref={inputRef}
          className="input grow"
          placeholder={t('networks.nwidPlaceholder')}
          value={nwid}
          onChange={e => setNwid(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && nwid.trim() && join.mutate()}
        />
        <button className="btn primary" disabled={!nwid.trim() || join.isPending} onClick={() => join.mutate()}>
          {t('networks.join')}
        </button>
        {planets.length > 1 && (
          <select className="input" value={planetFilter} onChange={e => setPlanetFilter(e.target.value)}>
            <option value="">{t('networks.allPlanets')}</option>
            {planets.map(p => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        )}
      </div>
      {rows.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              <th>{t('networks.id')}</th>
              <th>{t('networks.name')}</th>
              <th>{t('networks.status')}</th>
              <th>{t('networks.ips')}</th>
              <th>{t('networks.iface')}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map(r => (
              <tr key={`${r.planet}/${r.nwid}`}>
                <td className="mono">{r.nwid}</td>
                <td>
                  {r.name ?? '-'}
                  {planets.length > 1 && <div className="small">{r.planet}</div>}
                </td>
                <td>
                  <span className={`badge ${r.status === 'OK' ? 'ok' : 'muted'}`}>
                    {t(`networks.status.${r.status}`, { defaultValue: r.status })}
                  </span>
                  {r.conflict && (
                    <span className="badge warn" title={r.conflict}>
                      {t('networks.conflict')}
                    </span>
                  )}
                </td>
                <td className="mono small">{r.assignedAddrs.join(', ') || '-'}</td>
                <td className="mono">{r.interface ?? '-'}</td>
                <td>
                  <div className="row-actions">
                    <button className="btn" onClick={() => setEditing(r)}>
                      {t('networks.settings')}
                    </button>
                    <button
                      className="btn danger"
                      disabled={leave.isPending}
                      onClick={() => window.confirm(t('networks.leaveConfirm')) && leave.mutate(r)}
                    >
                      {t('networks.leave')}
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {rows.length === 0 && <div className="empty">{t('networks.empty')}</div>}
      {editing && <NetworkDialog row={editing} onClose={() => setEditing(null)} onDone={invalidate} />}
    </div>
  );
}

/// 设置对话框：契约仅三字段(spec §2)——routeViaZeroTier / dnsMode / customDns
function NetworkDialog({ row, onClose, onDone }: { row: NetworkEntry; onClose: () => void; onDone: () => void }) {
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
