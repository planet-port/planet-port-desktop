import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import type { NetworkEntry, PlanetStatus } from '../api/types';
import GearIcon from '../components/GearIcon';
import NetworkCard from '../components/NetworkCard';
import NetworkDialog from '../components/NetworkDialog';
import PlanetIconView from '../components/PlanetIconView';
import PlanetSettingsDialog from '../components/PlanetSettingsDialog';
import QueryError from '../components/QueryError';
import { useToast } from '../components/Toaster';

/// 单页首页:行星分组头(状态/传输/计数) + 网络卡片网格(方案C卡片,泊位灯)。
/// 加入入口移至顶栏「添加网络」抽屉;卡片开关 = 禁用/启用(脱离保留配置/恢复),彻底删除走详情抽屉
export default function Home() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState<NetworkEntry | null>(null);
  /// 行星设置弹层(分组头 ⚙);删除成功后 onDone 走全局失效
  const [editPlanet, setEditPlanet] = useState<PlanetStatus | null>(null);
  /// 开关 in-flight 网络(`${planet}/${nwid}`);pending 期对应卡片 spinner。
  /// 单槽:仅当槽位仍是自己的 key 时才清(并发点另一张卡不误清新请求的 spinner)
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

  const rejoin = useMutation({
    mutationFn: (row: NetworkEntry) => api.joinNetwork(row.nwid, row.planet),
    onSettled: (_d, _e, row) => {
      setToggling(cur => (cur === `${row.planet}/${row.nwid}` ? null : cur));
      invalidate();
    },
    onError: onErr,
  });
  const disable = useMutation({
    mutationFn: (row: NetworkEntry) => api.disableNetwork(row.nwid, row.planet),
    onSettled: (_d, _e, row) => {
      setToggling(cur => (cur === `${row.planet}/${row.nwid}` ? null : cur));
      invalidate();
    },
    onError: onErr,
  });
  const enable = useMutation({
    mutationFn: (row: NetworkEntry) => api.enableNetwork(row.nwid, row.planet),
    onSettled: (_d, _e, row) => {
      setToggling(cur => (cur === `${row.planet}/${row.nwid}` ? null : cur));
      invalidate();
    },
    onError: onErr,
  });

  const onToggle = (row: NetworkEntry) => {
    setToggling(`${row.planet}/${row.nwid}`);
    // 开关语义 = 网络启停:disable 脱离但保留配置,enable 恢复;彻底删除
    // (leave)走详情抽屉「删除网络」;STOPPED 等占位行的开 = 重新 join
    if (row.status === 'OK') disable.mutate(row);
    else if (row.status === 'DISABLED') enable.mutate(row);
    else rejoin.mutate(row);
  };

  return (
    <div className="page home">
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
                <PlanetIconView icon={p.icon} color={p.color} name={p.name} id={p.id} size={16} />
                <span className={`dot ${p.status === 'running' ? 'on' : 'off'}`} />
                <span className="pgroup-name">{p.name}</span>
                <span className="pgroup-meta">
                  {t(`planet.status.${p.status}`)}
                  {p.transport ? ` · ${t(`transport.tcp.${p.transport.tcp}`)}` : ''}
                </span>
                <span className="grow" />
                <span className="pgroup-meta">{t('home.netCount', { ok: mine.filter(r => r.status === 'OK').length, total: mine.length })}</span>
                <button
                  className="btn icon"
                  aria-label={t('planet.settingsTitle', { name: p.name })}
                  onClick={() => setEditPlanet(p)}
                >
                  <GearIcon />
                </button>
              </header>
              {mine.length === 0 ? (
                <div className="empty small">{t('home.noNetworks')}</div>
              ) : (
                <div className="netcards">
                  {mine.map(r => (
                    <NetworkCard key={`${r.planet}/${r.nwid}`} row={r} pending={toggling === `${r.planet}/${r.nwid}`} onOpen={setEditing} onToggle={onToggle} />
                  ))}
                </div>
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
