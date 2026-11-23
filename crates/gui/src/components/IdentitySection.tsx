import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { api, asError } from '../api/client';
import { useToast } from './Toaster';

type IdentityMode = 'shared' | 'independent';

/// 节点身份区:身份模式 switch(checked = 独立身份,拨动即生效,无确认弹窗;
/// 行星运行中禁用,daemon 409 兜底)。节点 ID 展示与导入身份入口已移除。
export default function IdentitySection() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();

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
  const independent = q.data?.mode === 'independent';

  return (
    <>
      <h4 className="group-title">{t('identity.section')}</h4>
      <section className="drawer-section">
        {/* 整行可点(button 内嵌 span.switch 视觉件,避免 button 嵌套) */}
        <button
          className="kv"
          data-testid="id-mode"
          disabled={runningCount > 0 || modeMut.isPending}
          aria-checked={independent}
          role="switch"
          onClick={() => modeMut.mutate(independent ? 'shared' : 'independent')}
        >
          <span>{t('identity.modeLabel')}</span>
          <span className={`switch ${independent ? 'on' : ''}`} aria-hidden="true" />
        </button>
      </section>
      <p className="group-foot">
        {runningCount > 0
          ? t('identity.modeBlocked', { count: runningCount })
          : independent
            ? t('identity.modeIndependent')
            : t('identity.modeShared')}
      </p>
    </>
  );
}
