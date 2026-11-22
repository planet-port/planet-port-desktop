import { useTranslation } from 'react-i18next';
import type { NetworkEntry } from '../api/types';

/// 网络卡片:信息区点击进设置弹层,右下角开关切换加入/离开(同移动端)。
/// pending 期开关换 spinner 并整卡禁点(防抖动)。
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
    <section className={`netcard ${pending ? 'pending' : ''}`} onClick={() => !pending && onOpen(row)}>
      <header>
        <span className="name">{row.name ?? row.nwid}</span>
        <span className={`badge ${on ? 'ok' : 'muted'}`}>{t(`networks.statusMap.${row.status}`, { defaultValue: row.status })}</span>
        {row.conflict && <span className="badge warn" title={row.conflict}>{t('networks.conflict')}</span>}
      </header>
      <dl>
        <dt className="mono">{row.nwid}</dt>
        <dd className="mono small">{row.assignedAddrs.join(', ') || '-'}</dd>
      </dl>
      <footer>
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
      </footer>
    </section>
  );
}
