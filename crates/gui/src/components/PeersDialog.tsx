import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import QueryError from './QueryError';

/// 节点表弹层(原 Peers 页收编):设置抽屉「高级」打开
export default function PeersDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const q = useQuery({ queryKey: ['peers'], queryFn: () => api.peers(), refetchInterval: 5000 });
  const rows = q.data?.peers ?? [];
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog dialog-wide" onClick={e => e.stopPropagation()}>
        <h3>{t('peers.title')}</h3>
        {rows.length > 0 && (
          <table className="table">
            <thead>
              <tr>
                <th>{t('peers.planet')}</th>
                <th>{t('peers.address')}</th>
                <th>{t('peers.role')}</th>
                <th>{t('peers.version')}</th>
                <th>{t('peers.latency')}</th>
                <th>{t('peers.path')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(p => (
                <tr key={`${p.planet}/${p.address}`}>
                  <td>{p.planet}</td>
                  <td className="mono">{p.address}</td>
                  <td>
                    <span className={`badge ${p.role === 'PLANET' ? 'ok' : 'muted'}`}>{p.role}</span>
                  </td>
                  <td>{p.version ?? '-'}</td>
                  <td>{p.latencyMs != null ? `${p.latencyMs}ms` : '-'}</td>
                  <td className="mono small">{p.path ?? t('peers.relay')}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {q.isError ? <QueryError error={q.error} /> : rows.length === 0 && <div className="empty">{t('peers.empty')}</div>}
        <div className="dialog-actions">
          <button className="btn" onClick={onClose}>{t('common.close')}</button>
        </div>
      </div>
    </div>
  );
}
