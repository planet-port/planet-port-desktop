import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { QrCode } from 'lucide-react';
import { api, asError } from '../api/client';
import { emptySlots, slotsToServers, slotsValid, type DnsSlots } from '../dns';
import DrawerShell from './DrawerShell';
import QRScannerDialog, { extractNetworkId } from './QRScannerDialog';
import { useToast } from './Toaster';
import { Button } from './ui-x/Button';
import { Input } from './ui-x/Input';
import { Select, SelectContent, SelectItem, SelectTrigger } from './ui-x/Select';
import { Switch } from './ui-x/Switch';
import { Tabs, TabsList, TabsTrigger } from './ui-x/Tabs';

const isHex16 = (s: string) => /^[0-9a-fA-F]{16}$/.test(s);

/// .kv 语义类 Tailwind 对译(与报告速查表同口径):
/// 卡片组 = --panel 底 + 1px --border 边 + 8px 圆角 + 2px 8px 内衬(暗色 #22252d/#2c3038);
/// 行盒 = 负 margin + padding 补偿撑满卡内宽(文字起点卡内 16px,分隔线 ::before 缩进对齐);
/// 可点行 hover 抬 --panel(可点信号);iOS 形态分隔线挂在后续行的 before 上。
const sectionCard = 'flex flex-col rounded-md border border-[color:var(--border)] bg-[var(--panel)] dark:bg-[#22252d] dark:border-[#2c3038] px-2 py-0.5';
/// 后续行(行盒同款几何 + 顶部 16px 缩进 hairline 分隔线)
const rowDivider = 'relative before:absolute before:top-0 before:left-4 before:right-0 before:border-t before:border-[color:var(--border-soft)] before:content-[""]';
/// 静态行(div)
const kvRow = 'flex items-center justify-between gap-3 -mx-2 px-2 py-1.5 text-[13px]';
/// 行内 label / 值区 / 弱化值(本文件值行由 ui-x/Select Trigger 承载可点形态)
const kvLabel = 'shrink-0 text-[var(--text)]';
const kvVal = 'inline-flex min-w-0 items-center justify-end gap-1.5';
const smallText = 'text-xs text-[var(--muted)]';
/// 卡外组标题 / 组尾说明(iOS Form 形态:负 margin 贴近卡片,净距 6px)
const groupTitle = 'mt-0.5 -mb-1.5 mr-2 text-xs font-medium text-[var(--muted)]';
const sectionFoot = 'mt-0.5 text-xs text-[var(--faint)]';
/// 校验错误行
const fieldError = 'm-0 text-xs text-[var(--danger)]';
/// Radix Select 空 value 非法:「默认行星」选项走哨兵值映射回 ''
const DEFAULT_PLANET = '__default__';

/// 添加网络抽屉(布局同 iOS JoinNetworkSheet):行星选择(桌面多行星并发
/// 特有,iOS 单隧道无此字段,置首)→ 网络 ID 组(输入框右端内嵌扫码钮,
/// 识别二维码中的 16hex 回填)+ routeVia 开关 + DNS 三段四槽。daemon join
/// 不收路由/DNS 参数,非默认值经 setNetwork 随后补投
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
        <header className="flex items-center justify-between pr-[30px]">
          <h3 className="m-0 text-sm font-medium">{t('join.title')}</h3>
        </header>
        <section className={sectionCard}>
          {/* 行星选择(iOS picker 值行):label + 当前值 + ›,点开浮层勾选列表 */}
          <Select
            value={planet || DEFAULT_PLANET}
            onValueChange={v => setPlanet(v === DEFAULT_PLANET ? '' : v)}
          >
            <SelectTrigger data-testid="planet-picker">
              <span className={kvLabel}>{t('join.planet')}</span>
              <span className={`ml-auto truncate ${smallText}`}>
                {planets.find(p => p.id === planet)?.name ?? t('join.defaultPlanet')}
              </span>
            </SelectTrigger>
            <SelectContent position="popper" sideOffset={4}>
              <SelectItem value={DEFAULT_PLANET} data-testid="planet-opt-default">
                {t('join.defaultPlanet')}
              </SelectItem>
              {planets.map(p => (
                <SelectItem key={p.id} value={p.id} data-testid={`planet-opt-${p.id}`}>
                  {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </section>
        <section className={sectionCard}>
          <div className={kvRow}>
            <span className={kvLabel}>{t('networks.id')}</span>
            {/* 扫码钮叠加输入框右端:pr 让位图标,绝对定位右对齐内嵌 */}
            <span className={`${kvVal} relative`}>
              <Input
                className="w-[200px] pr-7 text-right font-mono"
                autoFocus
                placeholder={t('join.idPlaceholder')}
                value={nwid}
                onChange={e => setNwid(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && idValid && dnsOk && !join.isPending && join.mutate()}
              />
              <button
                className="plain absolute right-1.5 top-1/2 -translate-y-1/2"
                aria-label={t('join.scan')}
                title={t('join.scan')}
                onClick={() => setShowScanner(true)}
              >
                <QrCode size={14} aria-hidden="true" />
              </button>
            </span>
          </div>
          {nwid.trim() !== '' && !idValid && <p className={`${fieldError} ${rowDivider}`}>{t('join.idInvalid')}</p>}
        </section>
        <section className={sectionCard}>
          <div className={kvRow}>
            <span className={kvLabel}>{t('join.routeVia')}</span>
            {/* Radix 受控开关:点击即回调 onCheckedChange(boolean),无自管 onClick */}
            <Switch checked={routeVia} onCheckedChange={setRouteVia} />
          </div>
          <p className={`${sectionFoot} ${rowDivider}`}>{t('join.routeViaFooter')}</p>
        </section>
        <h4 className={groupTitle}>DNS</h4>
        <section className={sectionCard}>
          <Tabs value={dnsMode} onValueChange={setDnsMode}>
            <TabsList>
              {(['none', 'network', 'custom'] as const).map(m => (
                <TabsTrigger key={m} value={m}>{t(`dns.mode.${m}`)}</TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          {dnsMode === 'custom' && (
            <div className={`flex flex-col gap-1.5 ${rowDivider}`}>
              {(['v4a', 'v4b', 'v6a', 'v6b'] as const).map(k => (
                <Input key={k} className="font-mono" placeholder={t(`dns.${k}`)} value={slots[k]}
                  onChange={e => setSlots({ ...slots, [k]: e.target.value })} />
              ))}
              {!dnsOk && <p className={fieldError}>{t('dns.invalid')}</p>}
            </div>
          )}
        </section>
        <Button variant="primary" block className="mt-2.5" disabled={!idValid || !dnsOk || join.isPending} onClick={() => join.mutate()}>
          {t('join.action')}
        </Button>
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
