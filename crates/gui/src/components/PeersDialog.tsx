import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import QueryError from './QueryError';
import { Dialog, DialogContent, DialogTitle } from './ui-x/Dialog';

/// 基线 .table 系对译(styles.css 已删,本组件独占):卡式表格,th 灰字
/// --bg 底、行间 hairline 分隔、末行无分隔、--radius 圆角裁切
const thCls =
  'border-b border-[var(--border-soft)] bg-[var(--bg)] px-3 py-2.5 text-left text-xs font-medium text-[var(--muted)]';
const tdCls = 'border-b border-[var(--border-soft)] px-3 py-2.5 text-left';
/// .badge 对译:边框式徽标(透明底 + 1px 状态色边 + 状态色字,45% 混色用
/// srgb color-mix arbitrary 复刻,不用 oklab 透明度修饰符)
const badgeCls =
  'inline-flex items-center rounded-full border px-[9px] py-[3px] font-mono text-xs leading-none';

/// 节点表弹层:行星设置「诊断」按行星过滤打开(隐藏 planet 列)。纯展示弹层
/// 无操作按钮,遮罩点击关闭。
/// 瞬时型弹层:内部持有 open,onClose 只翻 state 走退出动画,动画结束
/// (ui-x/Dialog onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
export default function PeersDialog({ planet, onClose }: { planet?: string; onClose: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const q = useQuery({
    queryKey: ['peers', planet ?? ''],
    queryFn: () => api.peers(planet),
    refetchInterval: 5000,
  });
  const rows = q.data?.peers ?? [];
  return (
    <Dialog open={open} onOpenChange={o => { if (!o) close(); }} onClosed={onClose}>
      {/* 六列表格弹层走宽档(.dialog-wide = min(760px, 100vw-40px)) */}
      <DialogContent testId="peers-dialog" closeLabel={t('common.close')} wide>
        <DialogTitle>{t('peers.title')}</DialogTitle>
        {rows.length > 0 && (
          <table className="w-full overflow-hidden rounded-[var(--radius)] border border-border border-collapse bg-[var(--panel)]">
            <thead>
              <tr>
                {!planet && <th className={thCls}>{t('peers.planet')}</th>}
                <th className={thCls}>{t('peers.address')}</th>
                <th className={thCls}>{t('peers.role')}</th>
                <th className={thCls}>{t('peers.version')}</th>
                <th className={thCls}>{t('peers.latency')}</th>
                <th className={thCls}>{t('peers.path')}</th>
              </tr>
            </thead>
            <tbody className="[&>tr:last-child>td]:border-b-0">
              {rows.map(p => (
                <tr key={`${p.planet}/${p.address}`}>
                  {!planet && <td className={tdCls}>{p.planet}</td>}
                  <td className={`${tdCls} mono`}>{p.address}</td>
                  <td className={tdCls}>
                    <span
                      className={
                        p.role === 'PLANET'
                          ? `${badgeCls} border-[color:color-mix(in_srgb,var(--ok)_34%,transparent)] text-[var(--ok)]`
                          : `${badgeCls} border-input text-[var(--muted)]`
                      }
                    >
                      {p.role}
                    </span>
                  </td>
                  <td className={tdCls}>{p.version ?? '-'}</td>
                  <td className={tdCls}>{p.latencyMs != null ? `${p.latencyMs}ms` : '-'}</td>
                  <td className={`${tdCls} mono small`}>{p.path ?? t('peers.relay')}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {q.isError ? (
          <QueryError error={q.error} />
        ) : (
          rows.length === 0 && <div className="py-12 text-center text-[var(--muted)]">{t('peers.empty')}</div>
        )}
      </DialogContent>
    </Dialog>
  );
}
