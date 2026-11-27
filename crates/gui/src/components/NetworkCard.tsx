import { useTranslation } from 'react-i18next';
import type { NetworkEntry } from '../api/types';
import CopyableValue from './CopyableValue';
import { Switch } from './ui-x/Switch';
import { pickDisplayAddr } from '../lib/addr';

/// 网络卡片(方案C卡片网格,弱透明卡面+泊位灯):信息区点击进设置弹层,卡头开关
/// 切换连接/断开(在网态↔DISCONNECTED,占位行=join)。泊位灯=网络级状态(在线青光/
/// 等待授权黄光呼吸/加入被拒红光/断开灰/冲突与设备异常黄)。开关 on 位 =
/// 核心 in-network(除 DISCONNECTED/STOPPED 外的全部状态),点击=脱离保留配置;
/// off 位 = 断开/占位,点击=join/恢复。断开只是不连接、非禁用,卡面与名称一律
/// 不降透明不弱化,状态语义全由灯与开关承载(2026-11-27 口径)。
/// pending/离线 disabled 期整卡禁点(pending 防抖动;disabled 降透明属真禁用,
/// 与「断开态不置灰」口径不冲突——断开仍在运行只是没连),hover 抬亮同期关闭:
/// 不可点还抬亮会给出「能点」的暗示。
/// IP 行为可复制值(小号图标);IP 只显示一个(IPv4 优先),复制与显示一致
/// (2026-11-24 口径)。连接中间态/错误态无 IP 可显,状态文字占用 IP 位:
/// 纯文字仅颜色区分(等待/冲突=黄/失败=红),不加边框底色等修饰(2026-11-27
/// 口径);冲突同为 IP 位纯文字(冲突可伴随 IP 在网,信息让位状态);设备
/// 异常保留黄 badge 承载。
/// 旧 .netcard 系规则对译:veil 卡面 + hairline 边 + --radius 圆角,hover 抬
/// --panel-2 底亮边;泊位灯 8px 方灯(ok=青光带 4px glow);值行 mono 12px 省略;
/// IP 占位降透明;设备异常徽章 34% 状态色边;spinner 0.8s 转(keyframes 由
/// animate-spin 触发 Tailwind 自带 spin,时长对齐基线 0.8s)
export default function NetworkCard({
  row,
  pending,
  disabled,
  onOpen,
  onToggle,
}: {
  row: NetworkEntry;
  pending: boolean;
  disabled?: boolean;
  onOpen: (row: NetworkEntry) => void;
  onToggle: (row: NetworkEntry) => void;
}) {
  const { t } = useTranslation();
  /// 断开态 = daemon 断开区占位(DISCONNECTED)+ planet 未运行的 STOPPED
  /// 占位;其余状态核心都已 join(含等待授权/被拒/设备层失败),开关一律
  /// on 位——开关反映「是否在网」而非「是否拿到了配置」
  const off = row.status === 'DISCONNECTED' || row.status === 'STOPPED';
  /// 等待控制器授权(REQUESTING_CONFIGURATION):可能长时间停留的稳态而非
  /// in-flight 请求,黄光呼吸承载,开关位留给真实开关(短时 loading 由
  /// pending spinner 负责,两者不混)
  const awaiting = row.status === 'REQUESTING_CONFIGURATION';
  /// 加入被拒/失败(ACCESS_DENIED/NOT_FOUND/PORT_ERROR/CLIENT_TOO_OLD/
  /// AUTHENTICATION_REQUIRED):红灯 + 红 badge,文案走 statusMap
  const failed =
    !off && !awaiting && row.status !== 'OK' && row.status !== 'DEVICE_ERROR';
  /// 设备层失败(daemon 合成:核心 OK 但本地 feth/地址/路由没建起来,数据面
  /// 不通):黄灯警示不降透明,与 conflict 同色系;详情走诊断面板事件流
  const deviceError = row.status === 'DEVICE_ERROR';
  return (
    <section
      data-testid="network-card"
      className={`flex flex-col gap-2 rounded-md border border-[var(--border)] bg-[var(--veil)] px-3.5 py-3 transition-[border-color,background-color] duration-[120ms] ease-[ease] ${
        !pending && !disabled ? 'hover:border-[var(--border-h)] hover:bg-secondary' : ''
      } ${pending || disabled ? 'cursor-default opacity-70' : 'cursor-pointer'}`}
      onClick={() => !pending && !disabled && onOpen(row)}
    >
      <header className="flex items-center gap-2.5">
        <span
          className={`h-2 w-2 flex-none rounded-[2px] ${
            row.conflict || deviceError
              ? 'bg-[var(--warn)] shadow-[0_0_4px_var(--warn)]'
              : failed
                ? 'bg-[var(--danger)] shadow-[0_0_4px_var(--danger)]'
                : !off
                  ? awaiting
                    ? 'animate-pulse bg-[var(--warn)]'
                    : 'bg-[var(--lamp)] shadow-[0_0_4px_var(--lamp)]'
                  : 'bg-[var(--off)]'
          }`}
          aria-hidden="true"
        />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-foreground">
          {row.name ?? row.nwid}
        </span>
        {pending ? (
          <span
            className="inline-block h-4 w-4 flex-none animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--primary)] [animation-duration:0.8s]"
            aria-label="pending"
          />
        ) : (
          // 卡头开关:stopPropagation 挡住卡面 onClick 进详情;Radix 点击仍会
          // 内部翻转并回调 onCheckedChange,业务回调保持传 row 的原签名;
          // disabled 期 Radix 原生拦截,防御挡外部程序化触发
          <Switch
            checked={!off}
            disabled={disabled}
            onCheckedChange={() => !disabled && onToggle(row)}
            onClick={e => e.stopPropagation()}
          />
        )}
      </header>
      <div className="flex flex-col items-start gap-[3px]">
        <span className="max-w-full truncate font-mono text-xs leading-none tracking-[0.1px] text-muted-foreground">
          {row.nwid}
        </span>
        <div className="flex w-full items-center gap-2">
          {awaiting || failed || row.conflict ? (
            // 连接中间态/错误态/冲突:状态文字占用 IP 位,纯文字仅颜色区分,
            // 不加修饰(冲突可伴随 IP 在网,信息让位状态,详情走 title)。
            // min-h-7 对齐 CopyableValue 盒高,两态卡片行距节奏一致(缺了它
            // 整卡变矮、第三行基线错位)
            <span
              className={`inline-flex min-h-7 min-w-0 items-center truncate font-mono text-xs leading-none tracking-[0.1px] ${
                awaiting || row.conflict ? 'text-[var(--warn)]' : 'text-[var(--danger)]'
              }`}
              title={row.conflict ?? undefined}
            >
              {row.conflict ? t('networks.conflict') : t(`networks.statusMap.${row.status}`)}
            </span>
          ) : (
            <CopyableValue
              small
              className={`min-w-0 font-mono text-xs leading-none tracking-[0.1px] [&>span]:truncate ${
                row.assignedAddrs.length === 0 ? 'cursor-default opacity-45' : ''
              }`}
              text={pickDisplayAddr(row.assignedAddrs) || '-'}
            />
          )}
          {/* 接口名与 IP 同行、行尾右对齐(ml-auto 推右,CopyableValue 保内容宽
              靠左——其根自带 justify-end,拉伸即把 IP 推右,故不给 flex-1) */}
          {row.interface && (
            <span className="ml-auto flex-none font-mono text-xs leading-none text-[var(--muted)]">{row.interface}</span>
          )}
        </div>
        {deviceError && (
          <span
            className="mt-0.5 inline-flex items-center rounded-full border border-[color:color-mix(in_srgb,var(--warn)_34%,transparent)] px-[9px] py-[3px] font-mono text-xs leading-none text-[var(--warn)]"
          >
            {t('networks.statusMap.DEVICE_ERROR')}
          </span>
        )}
      </div>
    </section>
  );
}
