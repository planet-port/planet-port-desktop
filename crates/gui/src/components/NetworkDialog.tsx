import { useMemo, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import type { NetworkEntry } from '../api/types';
import ConfirmDialog from './ConfirmDialog';
import CopyableValue from './CopyableValue';
import DrawerShell from './DrawerShell';
import QRIcon from './QRIcon';
import QRShareSheet from './QRShareSheet';
import { useToast } from './Toaster';
import { Switch } from './ui-x/Switch';
import { serversToSlots, slotsToServers, slotsValid, type DnsSlots } from '../dns';

/// daemon mac 若为 12 位裸十六进制则补冒号小写(aa:bb:cc:dd:ee:ff),已带分隔原样
function formatMac(mac: string): string {
  return /^[0-9a-fA-F]{12}$/.test(mac) ? (mac.match(/.{2}/g) ?? []).join(':').toLowerCase() : mac;
}

/// 网络详情(布局同 iOS NetworkDetailView,全部即时生效无保存按钮):
/// 基础信息行(ID 行点击复制+二维码入口/MAC 可复制) + 分配地址逐行可复制 +
/// 托管路由(target↔via 点击切换,直连不可切) + DNS(模式即点即生效,custom 槽
/// 失焦且合法时自动应用,network 模式只读展示下发服务器) + routeVia 开关 +
/// 居中红字删除(=leave)。广播/桥接两行不做(daemon NetworkEntry 未暴露)
export default function NetworkDialog({ row, onClose, onDone }: { row: NetworkEntry; onClose: () => void; onDone: () => void }) {
  const { t } = useTranslation();
  const toast = useToast();
  const qc = useQueryClient();
  /// 瞬时型弹层:内部持有 open,onClose 只翻 state 走滑出动画,动画结束
  /// (DrawerShell onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [routeVia, setRouteVia] = useState(row.routeViaZeroTier);
  const [dnsMode, setDnsMode] = useState(row.dnsMode);
  const [slots, setSlots] = useState<DnsSlots>(() => serversToSlots(row.customDns));
  /// 已应用的 customDns(逗号连接串):custom 槽失焦时对比,有差异才发请求
  const [savedServers, setSavedServers] = useState(() => row.customDns.join(','));
  const [expandedRoutes, setExpandedRoutes] = useState<Set<number>>(new Set());
  const [showQR, setShowQR] = useState(false);
  /// 禁用态:daemon 对禁用网络 set 报 404,编辑控件整体禁用,
  /// footer 提示恢复后可编辑(动态字段缺失靠占位行数据天然为 '—'/空)
  const disabled = row.status === 'DISABLED';
  const [confirmDel, setConfirmDel] = useState(false);
  /// 网络二维码 = 网络 ID 纯文本(16 hex,与移动端扫码填 ID 互逆);
  /// data 身份须稳定——QRShareSheet 内部 useMemo 以 data 为依赖,内联编码会每渲染重置
  const qrData = useMemo(() => new TextEncoder().encode(row.nwid), [row.nwid]);

  /// 即时生效统一通道:单字段 PATCH,成功静默失效缓存(无成功 toast,
  /// 失败 toast + 调用方回滚本地态)
  const apply = useMutation({
    mutationFn: (body: { routeViaZeroTier?: boolean; dnsMode?: string; customDns?: string[] }) =>
      api.setNetwork(row.nwid, body, row.planet),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['networks'] });
      void qc.invalidateQueries({ queryKey: ['status'] });
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });
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
  const setMode = (m: string) => {
    if (disabled || m === dnsMode) return;
    const prev = dnsMode;
    setDnsMode(m);
    const servers = m === 'custom' && slotsValid(slots) ? slotsToServers(slots) : null;
    apply.mutate(
      servers ? { dnsMode: m, customDns: servers } : { dnsMode: m },
      {
        onSuccess: () => {
          if (servers) setSavedServers(servers.join(','));
        },
        onError: () => setDnsMode(prev),
      },
    );
  };
  /// custom 槽失焦:合法且有差异才应用(输入中间态不打请求)
  const applySlots = () => {
    if (disabled || dnsMode !== 'custom' || !slotsValid(slots)) return;
    const servers = slotsToServers(slots);
    if (servers.join(',') === savedServers) return;
    apply.mutate({ dnsMode: 'custom', customDns: servers }, { onSuccess: () => setSavedServers(servers.join(',')) });
  };
  /// 开关即时生效:下一态由 Radix onCheckedChange(boolean) 给出(禁用态下
  /// 原件不触发点击,这里仍保留守卫兜底 routeViaLocked),失败回滚本地态
  const toggleRouteVia = (next: boolean) => {
    if (disabled || row.routeViaLocked) return;
    setRouteVia(next);
    apply.mutate({ routeViaZeroTier: next }, { onError: () => setRouteVia(!next) });
  };
  const del = useMutation({
    mutationFn: () => api.leaveNetwork(row.nwid, row.planet),
    onSuccess: () => {
      toast.ok(t('networks.left'));
      onDone();
      setConfirmDel(false);
      close();
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });

  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose} testId="network-detail">
        <header>
          <h3>{t('networks.detailTitle', { name: row.name ?? row.nwid })}</h3>
        </header>
        <section className="detail-section">
          <div className="kv">
            <span>{t('networks.id')}</span>
            <span className="kv-val">
              <CopyableValue text={row.nwid} />
              <button className="plain" aria-label={t('networks.qr')} title={t('networks.qr')} onClick={() => setShowQR(true)}><QRIcon /></button>
            </span>
          </div>
          <div className="kv"><span>{t('networks.name')}</span><span className="mono">{row.name ?? '-'}</span></div>
          <div className="kv">
            <span>{t('networks.statusLabel')}</span>
            <span className="mono">{t(`networks.statusMap.${row.status}`, { defaultValue: row.status })}</span>
          </div>
          <div className="kv"><span>{t('networks.type')}</span><span className="mono">{row.type}</span></div>
          <div className="kv">
            <span>{t('networks.mac')}</span>
            <CopyableValue text={formatMac(row.mac)} />
          </div>
          <div className="kv"><span>{t('networks.mtu')}</span><span className="mono">{row.mtu || '-'}</span></div>
        </section>
        <h4 className="group-title">{t('networks.addrs')}</h4>
        <section className="detail-section">
          {row.assignedAddrs.length === 0 ? (
            <div className="kv"><span>{t('networks.addrLabel', { index: 1 })}</span><span className="mono">-</span></div>
          ) : (
            row.assignedAddrs.map((a, i) => (
              <div className="kv" key={a}>
                <span>{t('networks.addrLabel', { index: i + 1 })}</span>
                <CopyableValue text={a} />
              </div>
            ))
          )}
        </section>
        <h4 className="group-title">{t('networks.routes')}</h4>
        <section className="detail-section">
          {row.managedRoutes.length === 0 ? (
            <div className="kv"><span>{t('networks.routeLabel', { index: 1 })}</span><span className="mono">-</span></div>
          ) : (
            row.managedRoutes.map((r, i) => (
              <div className="kv" key={`${r.target}/${r.via ?? ''}/${i}`}>
                <span>{t('networks.routeLabel', { index: i + 1 })}</span>
                <button
                  className={`link mono ${r.via ? '' : 'muted'}`}
                  disabled={!r.via}
                  title={r.via ? t('networks.routeToggleHint') : t('networks.directRoute')}
                  onClick={() => toggleRoute(i, r.via)}
                >
                  {expandedRoutes.has(i) && r.via ? `via ${r.via}` : r.target}
                </button>
              </div>
            ))
          )}
        </section>
        <h4 className="group-title">DNS</h4>
        <section className="detail-section">
          <div className="seg">
            {(['none', 'network', 'custom'] as const).map(m => (
              <button key={m} className={dnsMode === m ? 'active' : ''} disabled={disabled} onClick={() => setMode(m)}>{t(`dns.mode.${m}`)}</button>
            ))}
          </div>
          {dnsMode === 'custom' && (
            <div className="dns-slots">
              {(['v4a', 'v4b', 'v6a', 'v6b'] as const).map(k => (
                <input key={k} className="input mono" placeholder={t(`dns.${k}`)} value={slots[k]}
                  disabled={disabled}
                  onChange={e => setSlots({ ...slots, [k]: e.target.value })}
                  onBlur={applySlots}
                  onKeyDown={e => e.key === 'Enter' && (e.target as HTMLInputElement).blur()} />
              ))}
              {!slotsValid(slots) && <p className="field-error">{t('dns.invalid')}</p>}
            </div>
          )}
          {dnsMode === 'network' && (
            row.dnsServers.length > 0 ? (
              row.dnsServers.map(s => <span key={s} className="dns-net-server mono">{s}</span>)
            ) : (
              <span className="small">{t('networks.dnsPending')}</span>
            )
          )}
          <p className="section-foot">{t('networks.dnsFooter')}</p>
        </section>
        <section className="detail-section">
          <div className="kv">
            <span>{t('join.routeVia')}</span>
            <Switch checked={routeVia} onCheckedChange={toggleRouteVia} disabled={row.routeViaLocked || disabled} />
          </div>
          <p className="section-foot">
            {disabled ? t('networks.disabledHint') : (row.routeViaLocked ? t('networks.routeViaLocked') : t('join.routeViaFooter'))}
          </p>
        </section>
        <section className="detail-section">
          <button className="danger-row" onClick={() => setConfirmDel(true)}>{t('networks.deleteNetwork')}</button>
        </section>
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
