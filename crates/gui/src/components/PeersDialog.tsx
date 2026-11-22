import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import DrawerShell from './DrawerShell';
import QueryError from './QueryError';

/// 节点表弹层:设置抽屉「高级」开全局表;行星设置「诊断」按行星过滤打开
/// (隐藏 planet 列)。纯展示弹层无操作按钮,遮罩点击关闭。
/// 瞬时型弹层:内部持有 open,onClose 只翻 state 走滑出动画,动画结束
/// (DrawerShell onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
export default function PeersDialog({ planet, onClose }: { planet?: string; onClose: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const q = useQuery({
    queryKey: ['peers', planet ?? ''],
    queryFn: () => api.peers(planet),
    refetchInterval: 5000,
  });
  const rows = q.data?.peers ?? [];
  return (
    <DrawerShell open={open} onClose={close} onClosed={onClose} testId="peers-dialog">
      <header>
        <h3>{t('peers.title')}</h3>
      </header>
      {rows.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              {!planet && <th>{t('peers.planet')}</th>}
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
                {!planet && <td>{p.planet}</td>}
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
    </DrawerShell>
  );
}
