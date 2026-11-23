import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import ConfirmDialog from './ConfirmDialog';
import { useToast } from './Toaster';

type IdentityMode = 'shared' | 'independent';

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
      <h4 className="group-title">{t('identity.section')}</h4>
      <section className="drawer-section">
        <button
          className="kv"
          data-testid="id-mode"
          disabled={runningCount > 0 || modeMut.isPending}
          onClick={() => setConfirming(true)}
        >
          <span>{t('identity.modeLabel')}</span>
          <span className="kv-val">
            <span className="small">{t(`identity.mode.${mode}`)}</span>
            <span className="chev" aria-hidden="true">›</span>
          </span>
        </button>
      </section>
      <p className="group-foot">
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
