import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import { emptySlots, slotsToServers, slotsValid, type DnsSlots } from '../dns';
import DrawerShell from './DrawerShell';
import QRScannerDialog, { extractNetworkId } from './QRScannerDialog';
import QRIcon from './QRIcon';
import { useToast } from './Toaster';

const isHex16 = (s: string) => /^[0-9a-fA-F]{16}$/.test(s);

/// 添加网络抽屉(布局同 iOS JoinNetworkSheet):网络 ID 组(输入框尾部扫码
/// 按钮,识别二维码中的 16hex 回填)+ 行星选择(桌面多行星并发特有,iOS 单
/// 隧道无此字段) + routeVia 开关 + DNS 三段四槽。daemon join 不收路由/DNS
/// 参数,非默认值经 setNetwork 随后补投
export default function AddNetworkDrawer({ defaultPlanet, onClose }: { defaultPlanet?: string; onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  /// 瞬时型弹层:由 App 条件渲染挂载,内部持有 open,onClose 只翻 state 走
  /// 滑出动画,动画结束(DrawerShell onClosed)才回调父层 onClose 卸载
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [nwid, setNwid] = useState('');
  /// 首页空卡入口按分组预选行星(托盘/无来源时仍为默认行星)
  const [planet, setPlanet] = useState(defaultPlanet ?? '');
  const [routeVia, setRouteVia] = useState(false);
  const [dnsMode, setDnsMode] = useState('none');
  const [slots, setSlots] = useState<DnsSlots>(emptySlots());
  const [showScanner, setShowScanner] = useState(false);
  const [planetOpen, setPlanetOpen] = useState(false);

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
      close();
    },
    onError: e => {
      const ge = asError(e);
      toast.error(ge?.message ?? String(e));
    },
  });

  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose} testId="add-network-drawer">
        <header>
          <h3>{t('join.title')}</h3>
        </header>
        <section className="detail-section">
          <div className="kv">
            <span>{t('networks.id')}</span>
            <span className="kv-val">
              <input
                className="input mono id-input"
                autoFocus
                placeholder={t('join.idPlaceholder')}
                value={nwid}
                onChange={e => setNwid(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && idValid && dnsOk && !join.isPending && join.mutate()}
              />
              <button className="plain" aria-label={t('join.scan')} title={t('join.scan')} onClick={() => setShowScanner(true)}>
                <QRIcon />
              </button>
            </span>
          </div>
          {nwid.trim() !== '' && !idValid && <p className="field-error">{t('join.idInvalid')}</p>}
        </section>
        <section className="detail-section">
          {/* 行星选择(iOS picker 值行):label + 当前值 + ›,点开下方选项列表 */}
          <button className="kv" data-testid="planet-picker" onClick={() => setPlanetOpen(v => !v)}>
            <span>{t('join.planet')}</span>
            <span className="kv-val">
              <span className="small">{planets.find(p => p.id === planet)?.name ?? t('join.defaultPlanet')}</span>
              <span className="chev" aria-hidden="true">›</span>
            </span>
          </button>
          {planetOpen && (
            <div className="picker-pop" data-testid="planet-pop">
              <button
                className="kv"
                data-testid="planet-opt-default"
                onClick={() => { setPlanet(''); setPlanetOpen(false); }}
              >
                <span>{t('join.defaultPlanet')}</span>
                {planet === '' && <span className="check" aria-hidden="true">✓</span>}
              </button>
              {planets.map(p => (
                <button
                  className="kv"
                  key={p.id}
                  data-testid={`planet-opt-${p.id}`}
                  onClick={() => { setPlanet(p.id); setPlanetOpen(false); }}
                >
                  <span>{p.name}</span>
                  {planet === p.id && <span className="check" aria-hidden="true">✓</span>}
                </button>
              ))}
            </div>
          )}
        </section>
        <section className="detail-section">
          <div className="kv">
            <span>{t('join.routeVia')}</span>
            <button
              className={`switch ${routeVia ? 'on' : ''}`}
              role="switch"
              aria-checked={routeVia}
              onClick={() => setRouteVia(!routeVia)}
            />
          </div>
          <p className="section-foot">{t('join.routeViaFooter')}</p>
        </section>
        <h4 className="group-title">DNS</h4>
        <section className="detail-section">
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
        <button className="btn primary btn-block" disabled={!idValid || !dnsOk || join.isPending} onClick={() => join.mutate()}>
          {t('join.action')}
        </button>
      </DrawerShell>
      {/* 扫码弹层与抽屉平级:识别成功回填 ID 并停留在加入表单,只关扫码层 */}
      {showScanner && (
        <QRScannerDialog
          extract={extractNetworkId}
          onScanned={id => {
            setNwid(id);
            setShowScanner(false);
          }}
          onClose={() => setShowScanner(false)}
        />
      )}
    </>
  );
}
