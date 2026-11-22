import { useMemo, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import type { NetworkEntry } from '../api/types';
import ConfirmDialog from './ConfirmDialog';
import DrawerShell from './DrawerShell';
import QRShareSheet from './QRShareSheet';
import { useToast } from './Toaster';
import { serversToSlots, slotsToServers, slotsValid, type DnsSlots } from '../dns';

/// daemon mac 若为 12 位裸十六进制则补冒号小写(aa:bb:cc:dd:ee:ff),已带分隔原样
function formatMac(mac: string): string {
  return /^[0-9a-fA-F]{12}$/.test(mac) ? (mac.match(/.{2}/g) ?? []).join(':').toLowerCase() : mac;
}

/// 网络详情(参数面同 iOS NetworkDetailView):只读行+分配地址(逐条可复制)+
/// 托管路由(target↔via 点击切换,直连不可切)+DNS 三段四槽+routeVia+删除(=leave)。
/// 广播/桥接两行不做(daemon NetworkEntry 未暴露)
export default function NetworkDialog({ row, onClose, onDone }: { row: NetworkEntry; onClose: () => void; onDone: () => void }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [routeVia, setRouteVia] = useState(row.routeViaZeroTier);
  const [dnsMode, setDnsMode] = useState(row.dnsMode);
  const [slots, setSlots] = useState<DnsSlots>(() => serversToSlots(row.customDns));
  const [expandedRoutes, setExpandedRoutes] = useState<Set<number>>(new Set());
  const [showQR, setShowQR] = useState(false);
  const [confirmDel, setConfirmDel] = useState(false);
  /// 网络二维码 = 网络 ID 纯文本(16 hex,与移动端扫码填 ID 互逆);
  /// data 身份须稳定——QRShareSheet 内部 useMemo 以 data 为依赖,内联编码会每渲染重置
  const qrData = useMemo(() => new TextEncoder().encode(row.nwid), [row.nwid]);
  const dnsOk = dnsMode !== 'custom' || slotsValid(slots);

  const copy = (s: string) => {
    void navigator.clipboard.writeText(s);
    toast.ok(t('guide.copied'));
  };
  /// 直连路由(via=null)无第二形态,不进切换集合
  const toggleRoute = (i: number, via: string | null) => {
    if (!via) return;
    setExpandedRoutes(prev => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  const saveSettings = useMutation({
    mutationFn: () =>
      api.setNetwork(row.nwid, { routeViaZeroTier: routeVia, dnsMode, customDns: slotsToServers(slots) }, row.planet),
    onSuccess: () => {
      toast.ok(t('common.saved'));
      onDone();
      onClose();
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });
  const del = useMutation({
    mutationFn: () => api.leaveNetwork(row.nwid, row.planet),
    onSuccess: () => {
      toast.ok(t('networks.left'));
      onDone();
      onClose();
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });

  return (
    <>
      <DrawerShell open onClose={onClose} wide testId="network-detail">
        <header>
          <h3>{t('networks.detailTitle', { name: row.name ?? row.nwid })}</h3>
          <button className="btn icon" aria-label={t('common.close')} onClick={onClose}>×</button>
        </header>
        <section className="detail-section">
          <div className="kv"><span>{t('networks.id')}</span><span className="mono">{row.nwid}</span></div>
          <div className="kv"><span>{t('networks.name')}</span><span>{row.name ?? '-'}</span></div>
          <div className="kv">
            <span>{t('networks.statusLabel')}</span>
            <span className={`badge ${row.status === 'OK' ? 'ok' : 'muted'}`}>{t(`networks.statusMap.${row.status}`, { defaultValue: row.status })}</span>
          </div>
          <div className="kv"><span>{t('networks.type')}</span><span>{row.type}</span></div>
          <div className="kv"><span>{t('networks.mac')}</span><span className="mono">{formatMac(row.mac)}</span></div>
          <div className="kv"><span>{t('networks.mtu')}</span><span>{row.mtu}</span></div>
        </section>
        <section className="detail-section">
          <h4>{t('networks.addrs')}</h4>
          {row.assignedAddrs.length === 0 ? (
            <span className="small">-</span>
          ) : (
            row.assignedAddrs.map(a => (
              <button key={a} className="link mono" onClick={() => copy(a)}>{a}</button>
            ))
          )}
        </section>
        <section className="detail-section">
          <h4>{t('networks.routes')}</h4>
          {row.managedRoutes.map((r, i) => (
            <button
              key={`${r.target}/${r.via ?? ''}`}
              className={`link mono ${r.via ? '' : 'muted'}`}
              disabled={!r.via}
              title={r.via ? t('networks.routeToggleHint') : t('networks.directRoute')}
              onClick={() => toggleRoute(i, r.via)}
            >
              {expandedRoutes.has(i) && r.via ? `via ${r.via}` : r.target}
            </button>
          ))}
        </section>
        <section className="detail-section">
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
        <section className="detail-section">
          <label className="field">
            <input type="checkbox" checked={routeVia} disabled={row.routeViaLocked}
              onChange={e => setRouteVia(e.target.checked)} /> {t('join.routeVia')}
          </label>
          <p className="small hint">{row.routeViaLocked ? t('networks.routeViaLocked') : t('join.routeViaFooter')}</p>
        </section>
        <div className="share-row">
          <span className="mono small">{row.nwid}</span>
          <button className="btn" onClick={() => copy(row.nwid)}>{t('networks.copyId')}</button>
          <button className="btn" onClick={() => setShowQR(true)}>{t('networks.qr')}</button>
        </div>
        <div className="dialog-actions">
          <button className="btn danger" onClick={() => setConfirmDel(true)}>{t('networks.deleteNetwork')}</button>
          <span className="grow" />
          <button className="btn" onClick={onClose}>{t('common.cancel')}</button>
          <button className="btn primary" disabled={!dnsOk || saveSettings.isPending} onClick={() => saveSettings.mutate()}>
            {t('common.save')}
          </button>
        </div>
      </DrawerShell>
      {/* QRShareSheet/确认框与详情抽屉平级(DrawerShell 经 portal 挂 body):
          点二维码遮罩只收二维码,不冒泡关闭详情 */}
      {showQR && (
        <QRShareSheet
          title={t('networks.qrTitle')}
          subtitle={row.name ?? row.nwid}
          data={qrData}
          onDismiss={() => setShowQR(false)}
          onSave={async dataUrl => {
            const path = await save({ defaultPath: `${row.nwid}.png` });
            if (!path) return;
            // dataURL → bytes → write_file_bytes(base64 部分经 atob 解码)
            const b64 = dataUrl.slice(dataUrl.indexOf(',') + 1);
            const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
            await api.writeBytes(path, bytes);
            toast.ok(t('common.saved'));
          }}
        />
      )}
      {confirmDel && (
        <ConfirmDialog
          open
          title={t('networks.deleteNetwork')}
          message={t('networks.deleteConfirm', { id: row.nwid })}
          confirmLabel={t('networks.deleteNetwork')}
          danger
          onConfirm={() => del.mutate()}
          onCancel={() => setConfirmDel(false)}
        />
      )}
    </>
  );
}
