import { useTranslation } from 'react-i18next';
import type { NetworkEntry } from '../api/types';
import CopyableValue from './CopyableValue';
import { Switch } from './ui-x/Switch';
import { pickDisplayAddr } from '../lib/addr';

/// 网络卡片(方案C卡片网格,弱透明卡面+泊位灯):信息区点击进设置弹层,卡头开关
/// 切换禁用/启用(OK↔DISABLED,占位行=join)。泊位灯=网络级状态(在线青光/
/// 连接中青光呼吸/断开灰/冲突黄),连接中(REQUESTING_CONFIGURATION,enable
/// 后未获配置阶段)整卡不降透明、开关位 spinner(2026-11-24 口径:连接中
/// 不得再呈断开灰态)。pending 期开关换 spinner 并整卡禁点(防抖动)。
/// IP 行为可复制值(小号图标);状态文字不展示,灯已承载。
/// IP 只显示一个(IPv4 优先),复制与显示一致(2026-11-24 口径)。
/// 旧 .netcard 系规则对译:veil 卡面 + hairline 边 + --radius 圆角,hover 抬
/// --panel-2 底亮边;泊位灯 8px 方灯(ok=青光带 4px glow);值行 mono 12px 省略;
/// IP 占位降透明;冲突徽章 34% 状态色边;spinner 0.8s 转(keyframes 由
/// animate-spin 触发 Tailwind 自带 spin,时长对齐基线 0.8s)
export default function NetworkCard({
  row,
  pending,
  onOpen,
  onToggle,
}: {
  row: NetworkEntry;
  pending: boolean;
  onOpen: (row: NetworkEntry) => void;
  onToggle: (row: NetworkEntry) => void;
}) {
  const { t } = useTranslation();
  const on = row.status === 'OK';
  /// 连接努力中(enable 后未拿到配置的中间态):青光呼吸 + 开关位 spinner
  const connecting = row.status === 'REQUESTING_CONFIGURATION';
  return (
    <section
      data-testid="network-card"
      className={`flex flex-col gap-2 rounded-md border border-[var(--border)] bg-[var(--veil)] px-3.5 py-3 transition-[border-color,background-color] duration-[120ms] ease-[ease] hover:border-[var(--border-h)] hover:bg-secondary ${
        pending ? 'cursor-default opacity-70' : 'cursor-pointer'
      } ${on || connecting ? '' : 'opacity-55'}`}
      onClick={() => !pending && onOpen(row)}
    >
      <header className="flex items-center gap-2.5">
        <span
          className={`h-2 w-2 flex-none rounded-[2px] ${
            row.conflict
              ? 'bg-[var(--warn)]'
              : on
                ? 'bg-[var(--lamp)] shadow-[0_0_4px_var(--lamp)]'
                : connecting
                  ? 'animate-pulse bg-[var(--lamp)]'
                  : 'bg-[var(--off)]'
          }`}
          aria-hidden="true"
        />
        <span
          className={`min-w-0 flex-1 truncate text-[13px] ${
            on ? 'font-medium text-foreground' : 'font-normal text-muted-foreground'
          }`}
        >
          {row.name ?? row.nwid}
        </span>
        {pending || connecting ? (
          <span
            className="inline-block h-4 w-4 flex-none animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--primary)] [animation-duration:0.8s]"
            aria-label="pending"
          />
        ) : (
          // 卡头开关:stopPropagation 挡住卡面 onClick 进详情;Radix 点击仍会
          // 内部翻转并回调 onCheckedChange,业务回调保持传 row 的原签名
          <Switch checked={on} onCheckedChange={() => onToggle(row)} onClick={e => e.stopPropagation()} />
        )}
      </header>
      <div className="flex flex-col items-start gap-[3px]">
        <span className="max-w-full truncate font-mono text-xs leading-none tracking-[0.1px] text-muted-foreground">
          {row.nwid}
        </span>
        <CopyableValue
          small
          className={`max-w-full font-mono text-xs leading-none tracking-[0.1px] [&>span]:truncate ${
            row.assignedAddrs.length === 0 ? 'cursor-default opacity-45' : ''
          }`}
          text={pickDisplayAddr(row.assignedAddrs) || '-'}
        />
        {row.conflict && (
          <span
            className="mt-0.5 inline-flex items-center rounded-full border border-[color:color-mix(in_srgb,var(--warn)_34%,transparent)] px-[9px] py-[3px] font-mono text-xs leading-none text-[var(--warn)]"
            title={row.conflict}
          >
            {t('networks.conflict')}
          </span>
        )}
      </div>
    </section>
  );
}
