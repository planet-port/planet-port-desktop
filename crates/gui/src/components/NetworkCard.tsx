import { useTranslation } from 'react-i18next';
import type { NetworkEntry } from '../api/types';
import CopyableValue from './CopyableValue';
import { Switch } from './ui-x/Switch';

/// 网络卡片(方案C卡片网格,弱透明卡面+泊位灯):信息区点击进设置弹层,卡头开关
/// 切换禁用/启用(OK↔DISABLED,占位行=join)。泊位灯=网络级状态(在线青光/DISABLED 灰/冲突黄),
/// 禁用卡整体降透明度。pending 期开关换 spinner 并整卡禁点(防抖动)。
/// IP 行为可复制值(小号图标);状态文字不展示,灯已承载。
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
  return (
    <section
      className={`netcard ${pending ? 'pending' : ''} ${on ? '' : 'is-off'}`}
      onClick={() => !pending && onOpen(row)}
    >
      <header>
        <span className={`sdot ${row.conflict ? 'warn' : on ? 'ok' : 'off'}`} aria-hidden="true" />
        <span className="name">{row.name ?? row.nwid}</span>
        {pending ? (
          <span className="spinner" aria-label="pending" />
        ) : (
          // 卡头开关:stopPropagation 挡住卡面 onClick 进详情;Radix 点击仍会
          // 内部翻转并回调 onCheckedChange,业务回调保持传 row 的原签名
          <Switch checked={on} onCheckedChange={() => onToggle(row)} onClick={e => e.stopPropagation()} />
        )}
      </header>
      <div className="netcard-body">
        <span className="nwid">{row.nwid}</span>
        <CopyableValue
          small
          className={`addr ${row.assignedAddrs.length === 0 ? 'is-blank' : ''}`}
          text={row.assignedAddrs.join(', ') || '-'}
        />
        {row.conflict && <span className="badge warn" title={row.conflict}>{t('networks.conflict')}</span>}
      </div>
    </section>
  );
}
