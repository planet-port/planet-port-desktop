import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import CheckListSheet from './CheckListSheet';
import ConfirmDialog from './ConfirmDialog';
import { useToast } from './Toaster';

type IdentityMode = 'shared' | 'independent';

/// 节点身份区:身份模式管理(shared/independent seg+确认+运行中预检)、
/// 全局/各行星身份展示 + 导入身份(文件选择→预检→确认→导入)。
/// 导入不带 mode:daemon 缺省保持现有模式,模式切换走 PUT /identity;
/// 行星运行中 daemon 409 拒绝,前端以禁用预检为主、409 toast 兜底。
export default function IdentitySection() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [confirming, setConfirming] = useState<{ path: string; nodeId: string; oldNodeId: string | null } | null>(null);
  const [pendingMode, setPendingMode] = useState<IdentityMode | null>(null);
  const [modeSheet, setModeSheet] = useState(false);

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
    <>
      <h4 className="group-title">{t('identity.section')}</h4>
      <section className="drawer-section">
      {q.data && (
        <>
          {/* 模式行:当前值 + › 进下级勾选页(iOS 设置形态);运行中禁用 */}
          <button
            className="kv"
            data-testid="id-mode"
            disabled={runningCount > 0 || modeMut.isPending}
            onClick={() => setModeSheet(true)}
          >
            <span>{t('identity.modeLabel')}</span>
            <span className="kv-val">
              <span className="small">{t(`identity.mode.${q.data.mode}`)}</span>
              <span className="chev" aria-hidden="true">›</span>
            </span>
          </button>
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
      </section>
      <p className="group-foot">{t('identity.hint')}</p>
      {modeSheet && q.data && (
        <CheckListSheet
          title={t('identity.modeLabel')}
          value={q.data.mode}
          options={(['shared', 'independent'] as const).map(m => ({
            value: m,
            label: t(`identity.mode.${m}`),
            disabled: runningCount > 0 || modeMut.isPending,
          }))}
          onPick={v => {
            if (v !== q.data!.mode) setPendingMode(v as IdentityMode);
            return true; // 选完即回,确认弹窗浮在设置抽屉上层
          }}
          onClose={() => setModeSheet(false)}
          testId="id-mode"
        />
      )}
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
      <ConfirmDialog
        open={pendingMode !== null}
        title={t('identity.modeSwitch')}
        message={pendingMode ? t('identity.modeSwitchConfirm', { mode: t(`identity.mode.${pendingMode}`) }) : ''}
        onConfirm={() => {
          if (pendingMode) modeMut.mutate(pendingMode);
          setPendingMode(null);
        }}
        onCancel={() => setPendingMode(null)}
      />
    </>
  );
}
