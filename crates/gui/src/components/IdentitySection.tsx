import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import ConfirmDialog from './ConfirmDialog';
import { useToast } from './Toaster';

type IdentityMode = 'shared' | 'independent';

/// .kv 语义类 Tailwind 对译(与报告速查表同口径)
const sectionCard = 'flex flex-col rounded-md border border-[color:var(--border)] bg-[var(--panel)] dark:bg-[#22252d] dark:border-[#2c3038] px-2 py-0.5';
const kvButtonRow = 'flex w-full items-center justify-between gap-3 -mx-2 px-2 py-1.5 text-left text-[13px] cursor-pointer border-0 bg-transparent rounded-[6px] transition-colors duration-[120ms] ease-[ease] enabled:hover:bg-[var(--panel)]';
const kvLabel = 'shrink-0 text-[var(--text)]';
const kvVal = 'inline-flex min-w-0 items-center justify-end gap-1.5';
const chev = 'shrink-0 text-sm leading-none text-[var(--faint)]';
const smallText = 'text-xs text-[var(--muted)]';
const groupTitle = 'mt-0.5 -mb-1.5 mr-2 text-xs font-medium text-[var(--muted)]';
const groupFoot = '-mt-1.5 mr-2 text-xs text-[var(--muted)]';

/// 节点身份区:身份模式值行(右侧当前模式+›)→点击弹确认(切换会改变
/// 行星节点地址、需重新授权,须显式确认)→确认即切。行星运行中禁用
/// (daemon 409 兜底)。节点 ID 展示与导入身份入口已移除。
export default function IdentitySection() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);

  const q = useQuery({ queryKey: ['identity'], queryFn: api.identityGet });
  const runningCount = (q.data?.planets ?? []).filter(p => p.running).length;
  const modeMut = useMutation({
    mutationFn: (m: IdentityMode) => api.identityModeSet(m),
    onSuccess: (_d, m) => {
      toast.ok(t('identity.modeSwitched', { mode: t(`identity.mode.${m}`) }));
      void qc.invalidateQueries({ queryKey: ['identity'] });
      void qc.invalidateQueries({ queryKey: ['status'] });
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });
  const mode = q.data?.mode ?? 'shared';
  const target: IdentityMode = mode === 'shared' ? 'independent' : 'shared';

  return (
    <>
      <h4 className={groupTitle}>{t('identity.section')}</h4>
      <section className={sectionCard}>
        <button
          className={kvButtonRow}
          data-testid="id-mode"
          disabled={runningCount > 0 || modeMut.isPending}
          onClick={() => setConfirming(true)}
        >
          <span className={kvLabel}>{t('identity.modeLabel')}</span>
          <span className={kvVal}>
            <span className={smallText}>{t(`identity.mode.${mode}`)}</span>
            <span className={chev} aria-hidden="true">›</span>
          </span>
        </button>
      </section>
      <p className={groupFoot}>
        {runningCount > 0
          ? t('identity.modeBlocked', { count: runningCount })
          : mode === 'independent'
            ? t('identity.modeIndependent')
            : t('identity.modeShared')}
      </p>
      <ConfirmDialog
        open={confirming}
        title={t('identity.modeSwitch')}
        message={t('identity.modeSwitchConfirm', { mode: t(`identity.mode.${target}`) })}
        confirmLabel={t('common.confirm')}
        onConfirm={() => {
          modeMut.mutate(target);
          setConfirming(false);
        }}
        onCancel={() => setConfirming(false)}
      />
    </>
  );
}
