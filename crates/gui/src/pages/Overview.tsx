import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import QueryError from '../components/QueryError';

export default function Overview() {
  const { t } = useTranslation();
  const { data, dataUpdatedAt, isError, error } = useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 3000 });
  const planets = data?.planets ?? [];
  return (
    <div className="page">
      {isError ? (
        <QueryError error={error} />
      ) : planets.length === 0 ? (
        <div className="empty">{t('overview.empty')}</div>
      ) : (
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
                <dt>{t('planet.online')}</dt>
                <dd>
                  {p.online ? t('common.yes') : t('common.no')}
                  {p.latencyMs != null ? ` · ${p.latencyMs}ms` : ''}
                </dd>
                <dt>{t('planet.networks')}</dt>
                <dd>{p.networkCount}</dd>
                <dt>{t('planet.node')}</dt>
                <dd className="mono">{p.nodeId ?? '-'}</dd>
                {p.transport && (
                  <>
                    <dt>{t('planet.transport')}</dt>
                    <dd className="small">
                      UDP {p.transport.primaryPort}
                      {p.transport.secondaryPort != null ? ` / ${p.transport.secondaryPort}` : ''} · {t(`transport.tcp.${p.transport.tcp}`)}
                    </dd>
                  </>
                )}
              </dl>
            </section>
          ))}
        </div>
      )}
      {dataUpdatedAt > 0 && <footer className="hint">{t('overview.updatedAt', { time: new Date(dataUpdatedAt).toLocaleTimeString() })}</footer>}
    </div>
  );
}
