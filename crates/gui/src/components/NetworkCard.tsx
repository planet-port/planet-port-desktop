import { useTranslation } from 'react-i18next';
import type { NetworkEntry } from '../api/types';

/// 网络行(40px 单行,面板容器 .netcards 内 1px 分隔):信息区点击进设置弹层,行尾开关
/// 切换禁用/启用(OK↔DISABLED,占位行=join)。左=状态点(OK 绿/DISABLED 灰/冲突黄)+网络名+nwid,
/// 右=地址+状态徽标+开关。pending 期开关换 spinner 并整行禁点(防抖动)。
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
      <span className={`sdot ${row.conflict ? 'warn' : on ? 'ok' : 'off'}`} aria-hidden="true" />
      <span className="name">{row.name ?? row.nwid}</span>
      <span className="nwid">{row.nwid}</span>
      <span className="grow" />
      <span className={`addr ${row.assignedAddrs.length === 0 ? 'is-blank' : ''}`}>
        {row.assignedAddrs.join(', ') || '-'}
      </span>
      <span className={`badge ${on ? 'ok' : 'muted'}`}>{t(`networks.statusMap.${row.status}`, { defaultValue: row.status })}</span>
      {row.conflict && <span className="badge warn" title={row.conflict}>{t('networks.conflict')}</span>}
      {pending ? (
        <span className="spinner" aria-label="pending" />
      ) : (
        <button
          className={`switch ${on ? 'on' : ''}`}
          role="switch"
          aria-checked={on}
          onClick={e => {
            e.stopPropagation();
            onToggle(row);
          }}
        />
      )}
    </section>
  );
}
