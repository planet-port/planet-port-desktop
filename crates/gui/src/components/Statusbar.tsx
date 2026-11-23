import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';

/// 底部状态条(方案 B):窗口级全局计数概览。与 Home 共用 status/networks
/// 查询键,缓存由 Home 的轮询保持新鲜,这里只读不设独立轮询。
/// 旧 .statusbar/.s-div 规则对译:33px 恒定高、hairline 上边、mono 12px 灰字、
/// 24px 水平留白;分隔竖线 1×12px
export default function Statusbar() {
  const { t } = useTranslation();
  const status = useQuery({ queryKey: ['status'], queryFn: api.status });
  const q = useQuery({ queryKey: ['networks', ''], queryFn: () => api.networks() });
  const planets = status.data?.planets ?? [];
  const nets = q.data?.networks ?? [];
  const online = nets.filter(n => n.status === 'OK').length;
  return (
    <footer
      className="flex h-[33px] shrink-0 items-center gap-3 border-t border-[var(--border-soft)] bg-background px-6 font-mono text-xs leading-none text-muted-foreground"
      data-tauri-drag-region
    >
      <span>{t('home.sbPlanets', { count: planets.length })}</span>
      <i className="h-3 w-px flex-none bg-[var(--border)]" aria-hidden="true" />
      <span>{t('home.sbNetworks', { count: nets.length })}</span>
      <i className="h-3 w-px flex-none bg-[var(--border)]" aria-hidden="true" />
      <span>{t('home.sbOnline', { count: online })}</span>
    </footer>
  );
}
