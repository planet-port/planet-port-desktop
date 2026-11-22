import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import ConfirmDialog from './ConfirmDialog';
import { useToast } from './Toaster';

/// 节点身份区:全局/各行星身份展示 + 导入身份(文件选择→预检→确认→导入)。
/// daemon 恒 shared,无模式切换;导入固定 mode=shared(spec 决策)。
export default function IdentitySection() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [confirming, setConfirming] = useState<{ path: string; nodeId: string; oldNodeId: string | null } | null>(null);

  const q = useQuery({ queryKey: ['identity'], queryFn: api.identityGet });
  const importMut = useMutation({
    mutationFn: (a: { path: string; force: boolean }) => api.identityImport(a.path, a.force),
    onSuccess: () => {
      toast.ok(t('identity.imported'));
      void qc.invalidateQueries({ queryKey: ['identity'] });
      void qc.invalidateQueries({ queryKey: ['status'] });
    },
    onError: e => toast.error(asError(e)?.message ?? String(e)),
  });

  /// 文件选择 → 预检 → 带事实进确认弹层
  const startImport = async () => {
    const picked = await open({ multiple: false, directory: false, title: t('identity.pickTitle') });
    if (!picked || Array.isArray(picked)) return;
    try {
      const pre = await api.identityPreflight(picked as string);
      if (pre.preflight !== true) {
        toast.error(t('identity.needNewDaemon'));
        return;
      }
      setConfirming({
        path: picked as string,
        nodeId: String(pre.nodeId ?? '-'),
        oldNodeId:
          (pre.globalIdentity as { exists?: boolean; nodeId?: string } | undefined)?.exists === true
            ? String((pre.globalIdentity as { nodeId?: string }).nodeId ?? '-')
            : null,
      });
    } catch (e) {
      const ge = asError(e);
      if (ge?.status === 404) {
        toast.error(t('identity.needNewDaemon'));
        return;
      }
      toast.error(ge?.message ?? String(e));
    }
  };

  return (
    <section className="drawer-section">
      <h4>{t('identity.section')}</h4>
      {q.data && (
        <>
          <div className="id-row">
            <span className="mono">{q.data.global.nodeId ?? '-'}</span>
            <button className="btn sm" onClick={() => { void navigator.clipboard.writeText(q.data!.global.nodeId ?? ''); toast.ok(t('guide.copied')); }}>
              {t('networks.copyId')}
            </button>
          </div>
          {q.data.planets.map(p => (
            <div key={p.id} className="id-row small">
              <span>{p.name}</span>
              <span className="mono">{p.nodeId ?? '-'}{p.running ? '' : ' ·' + t('planet.status.stopped')}</span>
            </div>
          ))}
        </>
      )}
      <button className="btn" onClick={() => void startImport()} disabled={importMut.isPending}>
        {t('identity.import')}
      </button>
      <p className="small hint">{t('identity.hint')}</p>
      <ConfirmDialog
        open={confirming !== null}
        title={t('identity.import')}
        message={confirming
          ? (confirming.oldNodeId != null
              ? t('identity.confirmOverwrite', { old: confirming.oldNodeId, nodeId: confirming.nodeId })
              : t('identity.confirmNew', { nodeId: confirming.nodeId }))
          : ''}
        confirmLabel={t('common.confirm')}
        onConfirm={() => {
          if (confirming) importMut.mutate({ path: confirming.path, force: confirming.oldNodeId != null });
          setConfirming(null);
        }}
        onCancel={() => setConfirming(null)}
      />
    </section>
  );
}
