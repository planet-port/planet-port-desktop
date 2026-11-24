import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Plus, Settings } from 'lucide-react';
import { api, asError } from '../api/client';
import type { NetworkEntry, PlanetStatus } from '../api/types';
import LocalBand from '../components/LocalBand';
import NetworkCard from '../components/NetworkCard';
import NetworkDialog from '../components/NetworkDialog';
import PlanetIconView from '../components/PlanetIconView';
import PlanetSettingsDialog from '../components/PlanetSettingsDialog';
import QueryError from '../components/QueryError';
import { Button } from '../components/ui-x/Button';
import { useToast } from '../components/Toaster';

/// 单页首页:行星分组头(状态/传输/计数) + 网络卡片网格(方案C卡片,泊位灯)。
/// 加入入口 = 卡片网格尾的空卡(点击开「添加网络」抽屉并预选本行星,顶栏按钮已移除);
/// 卡片开关 = 禁用/启用(脱离保留配置/恢复),彻底删除走详情抽屉。
/// 旧 .page/.pgroup 系/.netcards/.netcard.add 规则对译:页面区 16/18px 留白自滚、
/// 分组头 32px、网格 240px auto-fill、空卡虚线边 + 主色加号 + hover 主色描边
/// 与 80% 透明 veil 底
export default function Home({ onAddNetwork }: { onAddNetwork?: (planet?: string) => void }) {
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
    <div className="flex-1 overflow-y-auto px-[18px] py-4">
      {status.isError ? (
        <QueryError error={status.error} />
      ) : planets.length === 0 ? (
        <div className="py-12 text-center text-muted-foreground">{t('home.emptyNoPlanet')}</div>
      ) : (
        planets.map(p => {
          /// 卡片序:纯按名称(name 缺失回退 nwid,与卡片显示口径一致),
          /// 断开/连接状态不参与排序(2026-11-24 口径)
          const mine = rows
            .filter(r => r.planet === p.id)
            .sort((a, b) => (a.name ?? a.nwid).localeCompare(b.name ?? b.nwid));
          return (
            <section key={p.id} className={`mb-[18px] ${p.enabled ? '' : 'opacity-55'}`}>
              <header className="mb-2.5 flex h-8 items-center gap-2.5 px-1.5">
                <PlanetIconView icon={p.icon} color={p.color} name={p.name} id={p.id} size={22} className="rounded-[5px]" />
                <span
                  className={`inline-block h-1.5 w-1.5 flex-none rounded-[2px] ${
                    p.status === 'running' ? 'bg-[var(--ok)]' : 'bg-[var(--off)]'
                  }`}
                />
                <span className="text-[13px] font-medium text-foreground" data-testid="home-planet-name">{p.name}</span>
                <span className="whitespace-nowrap font-mono text-xs leading-none tracking-[0.1px] text-muted-foreground" data-testid="home-planet-status">
                  {t(`planet.status.${p.status}`)}
                  {p.transport ? ` · ${t(`transport.tcp.${p.transport.tcp}`)}` : ''}
                </span>
                <span className="flex-[1_1_auto]" />
                <span className="whitespace-nowrap font-mono text-xs leading-none tracking-[0.1px] text-muted-foreground">
                  {t('home.netCount', { ok: mine.filter(r => r.status === 'OK').length, total: mine.length })}
                </span>
                <Button
                  variant="icon"
                  aria-label={t('planet.settingsTitle', { name: p.name })}
                  onClick={() => setEditPlanet(p)}
                >
                  <Settings size={14} strokeWidth={1.6} aria-hidden="true" />
                </Button>
              </header>
              {/* 空分组文字提示默认不显示:无网络时添加卡即唯一内容,入口即引导 */}
              {false && mine.length === 0 && (
                <div className="py-12 text-center text-xs text-muted-foreground">{t('home.noNetworks')}</div>
              )}
              <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-2.5">
                {mine.map(r => (
                  <NetworkCard key={`${r.planet}/${r.nwid}`} row={r} pending={toggling === `${r.planet}/${r.nwid}`} onOpen={setEditing} onToggle={onToggle} />
                ))}
                {/* 空卡 = 添加网络入口(禁用分组不显示:行星已停,加入无意义) */}
                {p.enabled && (
                  <button
                    className="flex min-h-24 cursor-pointer flex-col items-center justify-center gap-1.5 rounded-md border border-dashed border-[var(--border-h)] bg-transparent px-3.5 py-3 font-medium text-muted-foreground transition-[border-color,background-color,color] duration-[120ms] ease-[ease] hover:border-primary hover:bg-[color:color-mix(in_srgb,var(--veil)_80%,transparent)] hover:text-foreground"
                    data-testid="home-add-network"
                    onClick={() => onAddNetwork?.(p.id)}
                  >
                    <Plus size={18} strokeWidth={2} aria-hidden="true" className="text-primary" />
                    <span>{t('home.addCard')}</span>
                  </button>
                )}
              </div>
            </section>
          );
        })
      )}

      {/* 底部工具带(方案 B):本机概览 + 命令速查 —— 默认隐藏(暂不加显示开关),
          启用时把条件改回 true 即可,组件与文案保持可用 */}
      {false && <LocalBand />}

      {editing && <NetworkDialog row={editing} onClose={() => setEditing(null)} onDone={invalidate} />}
      {editPlanet && (
        <PlanetSettingsDialog planet={editPlanet} onClose={() => setEditPlanet(null)} onDone={invalidate} />
      )}
    </div>
  );
}
