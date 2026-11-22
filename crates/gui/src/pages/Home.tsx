import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import type { NetworkEntry, PlanetStatus } from '../api/types';
import NetworkCard from '../components/NetworkCard';
import NetworkDialog from '../components/NetworkDialog';
import PlanetSettingsDialog from '../components/PlanetSettingsDialog';
import QueryError from '../components/QueryError';
import { useToast } from '../components/Toaster';

/// 单页首页:行星分组头(状态/传输/计数) + 网络卡片流(卡片⇄列表,localStorage 记忆)。
/// 加入入口移至顶栏「添加网络」抽屉;离开由卡片开关直切(同移动端,无确认弹层)
export default function Home() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [view, setView] = useState<'cards' | 'list'>(() => (localStorage.getItem('netview') === 'list' ? 'list' : 'cards'));
  const [editing, setEditing] = useState<NetworkEntry | null>(null);
  /// 行星设置弹层(分组头 ⚙);删除成功后 onDone 走全局失效
  const [editPlanet, setEditPlanet] = useState<PlanetStatus | null>(null);
  /// 开关 in-flight 网络(`${planet}/${nwid}`);pending 期对应卡片 spinner
  const [toggling, setToggling] = useState<string | null>(null);

  const status = useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 3000 });
  const planets: PlanetStatus[] = status.data?.planets ?? [];
  const q = useQuery({ queryKey: ['networks', ''], queryFn: () => api.networks(), refetchInterval: 5000 });
  const rows = q.data?.networks ?? [];

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['networks'] });
    void qc.invalidateQueries({ queryKey: ['status'] });
  };
  const onErr = (e: unknown) => {
    const ge = asError(e);
    const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
    toast.error(`${ge?.message ?? String(e)}${cand}`);
  };

  const leave = useMutation({
    mutationFn: (row: NetworkEntry) => api.leaveNetwork(row.nwid, row.planet),
    onSettled: () => {
      setToggling(null);
      invalidate();
    },
    onError: onErr,
  });
  const rejoin = useMutation({
    mutationFn: (row: NetworkEntry) => api.joinNetwork(row.nwid, row.planet),
    onSettled: () => {
      setToggling(null);
      invalidate();
    },
    onError: onErr,
  });

  const onToggle = (row: NetworkEntry) => {
    setToggling(`${row.planet}/${row.nwid}`);
    if (row.status === 'OK') leave.mutate(row);
    else rejoin.mutate(row);
  };
  const switchView = (v: 'cards' | 'list') => {
    setView(v);
    localStorage.setItem('netview', v);
  };

  return (
    <div className="page home">
      <div className="toolbar">
        <span className="grow" />
        <div className="viewtoggle">
          <button className={`btn ${view === 'cards' ? 'active' : ''}`} onClick={() => switchView('cards')} aria-label="cards">▤</button>
          <button className={`btn ${view === 'list' ? 'active' : ''}`} onClick={() => switchView('list')} aria-label="list">☰</button>
        </div>
      </div>

      {status.isError ? (
        <QueryError error={status.error} />
      ) : planets.length === 0 ? (
        <div className="empty">{t('home.emptyNoPlanet')}</div>
      ) : (
        planets.map(p => {
          const mine = rows.filter(r => r.planet === p.id);
          return (
            <section key={p.id} className={`pgroup ${p.enabled ? '' : 'disabled'}`}>
              <header className="pgroup-head">
                <span className={`dot ${p.status === 'running' ? 'on' : 'off'}`} />
                <span className="pgroup-name">{p.name}</span>
                <span className="small">
                  {t(`planet.status.${p.status}`)}
                  {p.transport ? ` · ${t(`transport.tcp.${p.transport.tcp}`)}` : ''}
                </span>
                <span className="grow" />
                <span className="small">{t('home.netCount', { ok: mine.filter(r => r.status === 'OK').length, total: mine.length })}</span>
                <button
                  className="btn icon"
                  aria-label={t('planet.settingsTitle', { name: p.name })}
                  onClick={() => setEditPlanet(p)}
                >
                  ⚙
                </button>
              </header>
              {mine.length === 0 ? (
                <div className="empty small">{t('home.noNetworks')}</div>
              ) : view === 'cards' ? (
                <div className="netcards">
                  {mine.map(r => (
                    <NetworkCard key={`${r.planet}/${r.nwid}`} row={r} pending={toggling === `${r.planet}/${r.nwid}`} onOpen={setEditing} onToggle={onToggle} />
                  ))}
                </div>
              ) : (
                <table className="table">
                  <thead>
                    <tr>
                      <th>{t('networks.id')}</th><th>{t('networks.name')}</th><th>{t('networks.statusLabel')}</th>
                      <th>{t('networks.ips')}</th><th />
                    </tr>
                  </thead>
                  <tbody>
                    {mine.map(r => (
                      <tr key={`${r.planet}/${r.nwid}`} onClick={() => setEditing(r)}>
                        <td className="mono">{r.nwid}</td>
                        <td>{r.name ?? '-'}</td>
                        <td><span className={`badge ${r.status === 'OK' ? 'ok' : 'muted'}`}>{t(`networks.statusMap.${r.status}`, { defaultValue: r.status })}</span></td>
                        <td className="mono small">{r.assignedAddrs.join(', ') || '-'}</td>
                        <td>
                          {toggling === `${r.planet}/${r.nwid}` ? (
                            <span className="spinner" />
                          ) : (
                            <button className={`switch sm ${r.status === 'OK' ? 'on' : ''}`} role="switch" aria-checked={r.status === 'OK'}
                              onClick={e => { e.stopPropagation(); onToggle(r); }} />
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </section>
          );
        })
      )}

      {editing && <NetworkDialog row={editing} onClose={() => setEditing(null)} onDone={invalidate} />}
      {editPlanet && (
        <PlanetSettingsDialog planet={editPlanet} onClose={() => setEditPlanet(null)} onDone={invalidate} />
      )}
    </div>
  );
}
