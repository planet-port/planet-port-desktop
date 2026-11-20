import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import type { PlanetStatus } from '../api/types';
import { useToast } from '../components/Toaster';

export default function Planets() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [source, setSource] = useState('');

  // 与概览共享 status 缓存
  const q = useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 3000 });
  const planets = q.data?.planets ?? [];

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['status'] });
    void qc.invalidateQueries({ queryKey: ['networks'] });
  };
  const onErr = (e: unknown) => {
    const ge = asError(e);
    const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
    toast.error(`${ge?.message ?? String(e)}${cand}`);
  };

  const importP = useMutation({
    mutationFn: () => api.addPlanet(source.trim()),
    onSuccess: () => {
      setSource('');
      invalidate();
      toast.ok(t('planets.imported'));
    },
    onError: onErr,
  });
  const enable = useMutation({ mutationFn: (id: string) => api.enablePlanet(id), onSuccess: invalidate, onError: onErr });
  const disable = useMutation({ mutationFn: (id: string) => api.disablePlanet(id), onSuccess: invalidate, onError: onErr });
  const remove = useMutation({
    mutationFn: (id: string) => api.removePlanet(id),
    onSuccess: invalidate,
    onError: onErr,
  });

  /// 删除运行中 planet 需二次确认并说明影响(spec §6)
  const confirmRemove = (p: PlanetStatus) => {
    const msg = p.status === 'running' ? t('planets.removeRunningConfirm', { name: p.name }) : t('planets.removeConfirm', { name: p.name });
    if (window.confirm(msg)) remove.mutate(p.id);
  };

  return (
    <div className="page">
      <div className="toolbar">
        <input className="input grow" placeholder={t('planets.sourcePlaceholder')} value={source} onChange={e => setSource(e.target.value)} />
        <button className="btn primary" disabled={!source.trim() || importP.isPending} onClick={() => importP.mutate()}>
          {t('planets.import')}
        </button>
      </div>
      {planets.length > 0 && (
        <div className="cards">
          {planets.map(p => (
            <section key={p.id} className="card">
              <header>
                <span className="name">{p.name}</span>
                <span className={`badge ${p.status === 'running' ? 'ok' : 'muted'}`}>{t(`planet.status.${p.status}`)}</span>
                {!p.enabled && <span className="badge warn">{t('planet.disabled')}</span>}
              </header>
              <dl>
                <dt>{t('planet.kind')}</dt>
                <dd>{t(`planet.kind.${p.type}`)}</dd>
                <dt>{t('planet.networks')}</dt>
                <dd>{p.networkCount}</dd>
                <dt>{t('planet.node')}</dt>
                <dd className="mono">{p.nodeId ?? '-'}</dd>
              </dl>
              <footer className="card-actions">
                {p.status === 'running' ? (
                  <button className="btn" disabled={disable.isPending} onClick={() => disable.mutate(p.id)}>
                    {t('planets.disable')}
                  </button>
                ) : (
                  <button className="btn primary" disabled={enable.isPending} onClick={() => enable.mutate(p.id)}>
                    {t('planets.enable')}
                  </button>
                )}
                <button className="btn danger" onClick={() => confirmRemove(p)}>
                  {t('common.delete')}
                </button>
              </footer>
            </section>
          ))}
        </div>
      )}
      {planets.length === 0 && <div className="empty">{t('planets.empty')}</div>}
    </div>
  );
}
