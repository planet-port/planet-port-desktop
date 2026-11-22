import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import DrawerShell from './DrawerShell';
import QueryError from './QueryError';

/// 节点表弹层(原 Peers 页收编):设置抽屉「高级」打开。
/// 瞬时型弹层:内部持有 open,onClose 只翻 state 走滑出动画,动画结束
/// (DrawerShell onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
export default function PeersDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const q = useQuery({ queryKey: ['peers'], queryFn: () => api.peers(), refetchInterval: 5000 });
  const rows = q.data?.peers ?? [];
  return (
    <DrawerShell open={open} onClose={close} onClosed={onClose} wide>
      <header>
        <h3>{t('peers.title')}</h3>
        <button className="btn icon" aria-label={t('common.close')} onClick={close}>×</button>
      </header>
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
        <button className="btn" onClick={close}>{t('common.close')}</button>
      </div>
    </DrawerShell>
  );
}
