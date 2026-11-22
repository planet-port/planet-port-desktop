import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import type { PlanetStatus } from '../api/types';
import ConfirmDialog from './ConfirmDialog';
import DrawerShell from './DrawerShell';
import QRShareSheet from './QRShareSheet';
import QueryError from './QueryError';
import { useToast } from './Toaster';
import { planetPayloadContainer } from '../ztp/container';

/// 行星管理弹层(原 Planets 页收编):设置抽屉「高级」打开
export default function PlanetManager({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  /// 瞬时型弹层:内部持有 open,onClose 只翻 state 走滑出动画,动画结束
  /// (DrawerShell onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [source, setSource] = useState('');
  // 待删除 planet(wry 不支持 window.confirm,删除须应用内对话框二次确认)
  const [pendingRemove, setPendingRemove] = useState<PlanetStatus | null>(null);
  const [qrPlanet, setQrPlanet] = useState<PlanetStatus | null>(null);
  const [qrData, setQrData] = useState<Uint8Array | null>(null);

  // 与概览共享 status 缓存
  const q = useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 3000 });
  const planets = q.data?.planets ?? [];

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['status'] });
    void qc.invalidateQueries({ queryKey: ['networks'] });
  };
  const onErr = (e: unknown) => {
    const ge = asError(e);
    const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
    toast.error(`${ge?.message ?? String(e)}${cand}`);
  };

  const importP = useMutation({
    mutationFn: () => api.addPlanet(source.trim()),
    onSuccess: () => {
      setSource('');
      invalidate();
      toast.ok(t('planets.imported'));
    },
    onError: onErr,
  });
  const enable = useMutation({ mutationFn: (id: string) => api.enablePlanet(id), onSuccess: invalidate, onError: onErr });
  const disable = useMutation({ mutationFn: (id: string) => api.disablePlanet(id), onSuccess: invalidate, onError: onErr });
  const remove = useMutation({
    mutationFn: (id: string) => api.removePlanet(id),
    onSuccess: invalidate,
    onError: onErr,
  });

  /// 删除运行中 planet 需二次确认并说明影响(spec §6)
  const removeMessage = (p: PlanetStatus) =>
    p.status === 'running' ? t('planets.removeRunningConfirm', { name: p.name }) : t('planets.removeConfirm', { name: p.name });

  /// 导出 .planet 文件:daemon bytes → dialog save → write_file_bytes
  const shareFile = async (p: PlanetStatus) => {
    try {
      const bytes = await api.planetExport(p.id);
      const path = await save({ defaultPath: `${p.name || p.id}.planet` });
      if (!path) return;
      await api.writeBytes(path, bytes);
      toast.ok(t('common.saved'));
    } catch (e) {
      onErr(e);
    }
  };

  /// ZTP 二维码:planet 二进制 + name/icon/color 组容器
  const openQr = async (p: PlanetStatus) => {
    try {
      const bytes = await api.planetExport(p.id);
      setQrData(planetPayloadContainer(Uint8Array.from(bytes), p.name, p.icon || null, p.color || null));
      setQrPlanet(p);
    } catch (e) {
      onErr(e);
    }
  };

  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose}>
        <header>
          <h3>{t('planets.managerTitle')}</h3>
          <button className="btn icon" aria-label={t('common.close')} onClick={close}>×</button>
        </header>
        <div className="toolbar">
          <input className="input grow" placeholder={t('planets.sourcePlaceholder')} value={source} onChange={e => setSource(e.target.value)} />
          <button className="btn primary" disabled={!source.trim() || importP.isPending} onClick={() => importP.mutate()}>
            {t('planets.import')}
          </button>
        </div>
        {planets.length > 0 && (
          <div className="cards">
            {planets.map(p => (
              <section key={p.id} className="card">
                <header>
                  <span className="name">{p.name}</span>
                  <span className={`badge ${p.status === 'running' ? 'ok' : 'muted'}`}>{t(`planet.status.${p.status}`)}</span>
                  {!p.enabled && <span className="badge warn">{t('planet.disabled')}</span>}
                </header>
                <dl>
                  <dt>{t('planet.kind')}</dt>
                  <dd>{t(`planet.kind.${p.type}`)}</dd>
                  <dt>{t('planet.networks')}</dt>
                  <dd>{p.networkCount}</dd>
                  <dt>{t('planet.node')}</dt>
                  <dd className="mono">{p.nodeId ?? '-'}</dd>
                </dl>
                <footer className="card-actions">
                  {p.status === 'running' ? (
                    <button className="btn" disabled={disable.isPending} onClick={() => disable.mutate(p.id)}>
                      {t('planets.disable')}
                    </button>
                  ) : (
                    <button className="btn primary" disabled={enable.isPending} onClick={() => enable.mutate(p.id)}>
                      {t('planets.enable')}
                    </button>
                  )}
                  <button className="btn danger" disabled={remove.isPending} onClick={() => setPendingRemove(p)}>
                    {t('common.delete')}
                  </button>
                  {p.type === 'custom' && (
                    <>
                      <button className="btn" onClick={() => shareFile(p)}>{t('planets.shareFile')}</button>
                      <button className="btn" onClick={() => void openQr(p)}>{t('planets.shareQr')}</button>
                    </>
                  )}
                </footer>
              </section>
            ))}
          </div>
        )}
        {q.isError ? (
          <QueryError error={q.error} />
        ) : (
          planets.length === 0 && <div className="empty">{t('planets.empty')}</div>
        )}
        <div className="dialog-actions">
          <button className="btn" onClick={close}>{t('common.close')}</button>
        </div>
      </DrawerShell>
      {/* 确认框/二维码与行星管理抽屉平级(DrawerShell 经 portal 挂 body):避免嵌在
          transform 抽屉内被裁剪,且后挂者居上 */}
      <ConfirmDialog
        open={pendingRemove !== null}
        title={t('common.delete')}
        message={pendingRemove ? removeMessage(pendingRemove) : ''}
        confirmLabel={t('common.delete')}
        danger
        onConfirm={() => {
          if (pendingRemove) remove.mutate(pendingRemove.id);
          setPendingRemove(null);
        }}
        onCancel={() => setPendingRemove(null)}
      />
      {qrPlanet && qrData && (
        <QRShareSheet
          title={t('planets.shareQrTitle', { name: qrPlanet.name })}
          subtitle={t('planets.shareQrHint')}
          data={qrData}
          onDismiss={() => {
            setQrPlanet(null);
            setQrData(null);
          }}
        />
      )}
    </>
  );
}
