import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import { emptySlots, slotsToServers, slotsValid, type DnsSlots } from '../dns';
import { useToast } from './Toaster';

const isHex16 = (s: string) => /^[0-9a-fA-F]{16}$/.test(s);

/// 添加网络抽屉(字段同 iOS JoinNetworkSheet):16hex 实时校验+提交小写+
/// 重复客户端预检;行星选择为桌面多行星并发特有(iOS 单隧道无此字段)。
/// daemon join 不收路由/DNS 参数,非默认值经 setNetwork 随后补投
export default function AddNetworkDrawer({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [nwid, setNwid] = useState('');
  const [planet, setPlanet] = useState('');
  const [routeVia, setRouteVia] = useState(false);
  const [dnsMode, setDnsMode] = useState('none');
  const [slots, setSlots] = useState<DnsSlots>(emptySlots());

  const status = useQuery({ queryKey: ['status'], queryFn: api.status });
  const planets = status.data?.planets ?? [];
  const existing = useQuery({ queryKey: ['networks', ''], queryFn: () => api.networks() });

  const idValid = isHex16(nwid.trim());
  const servers = slotsToServers(slots);
  const dnsOk = dnsMode !== 'custom' || slotsValid(slots);

  const join = useMutation({
    mutationFn: async () => {
      const id = nwid.trim().toLowerCase();
      if ((existing.data?.networks ?? []).some(n => n.nwid === id)) {
        throw new Error(t('join.duplicate', { id }));
      }
      await api.joinNetwork(id, planet || undefined);
      if (routeVia || dnsMode !== 'none') {
        await api.setNetwork(id, { routeViaZeroTier: routeVia, dnsMode, customDns: servers }, planet || undefined);
      }
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['networks'] });
      void qc.invalidateQueries({ queryKey: ['status'] });
      toast.ok(t('networks.joined'));
      onClose();
    },
    onError: e => {
      const ge = asError(e);
      toast.error(ge?.message ?? String(e));
    },
  });

  return (
    <>
      <div className="drawer-mask" onClick={onClose} />
      <aside className="drawer" data-testid="add-network-drawer">
        <header>
          <h3>{t('join.title')}</h3>
          <button className="btn icon" onClick={onClose}>×</button>
        </header>
        <section className="drawer-section">
          <h4>{t('networks.id')}</h4>
          <input className="input mono" autoFocus placeholder={t('join.idPlaceholder')} value={nwid}
            onChange={e => setNwid(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && idValid && dnsOk && !join.isPending && join.mutate()} />
          {nwid.trim() !== '' && !idValid && <p className="field-error">{t('join.idInvalid')}</p>}
        </section>
        <section className="drawer-section">
          <h4>{t('join.planet')}</h4>
          <select className="input" value={planet} onChange={e => setPlanet(e.target.value)}>
            <option value="">{t('join.defaultPlanet')}</option>
            {planets.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
        </section>
        <section className="drawer-section">
          <label className="field">
            <input type="checkbox" checked={routeVia} onChange={e => setRouteVia(e.target.checked)} /> {t('join.routeVia')}
          </label>
          <p className="small hint">{t('join.routeViaFooter')}</p>
        </section>
        <section className="drawer-section">
          <h4>DNS</h4>
          <div className="seg">
            {(['none', 'network', 'custom'] as const).map(m => (
              <button key={m} className={dnsMode === m ? 'active' : ''} onClick={() => setDnsMode(m)}>{t(`dns.mode.${m}`)}</button>
            ))}
          </div>
          {dnsMode === 'custom' && (
            <div className="dns-slots">
              {(['v4a', 'v4b', 'v6a', 'v6b'] as const).map(k => (
                <input key={k} className="input mono" placeholder={t(`dns.${k}`)} value={slots[k]}
                  onChange={e => setSlots({ ...slots, [k]: e.target.value })} />
              ))}
              {!dnsOk && <p className="field-error">{t('dns.invalid')}</p>}
            </div>
          )}
        </section>
        <button className="btn primary" disabled={!idValid || !dnsOk || join.isPending} onClick={() => join.mutate()}>
          {t('join.action')}
        </button>
      </aside>
    </>
  );
}
