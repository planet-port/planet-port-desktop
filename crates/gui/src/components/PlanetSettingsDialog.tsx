import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import type { PlanetStatus } from '../api/types';
import ConfirmDialog from './ConfirmDialog';
import DrawerShell from './DrawerShell';
import IconPicker from './IconPicker';
import PlanetIconView from './PlanetIconView';
import QRShareSheet from './QRShareSheet';
import { useToast } from './Toaster';
import { animalFor, colorFor } from '../icons/planetIcon';
import { planetPayloadContainer } from '../ztp/container';

/// 文件大小 1000 进制(iOS 口径):B→KB→MB→GB→TB
function locFileSize(n: number): string {
  if (n < 1000) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = -1;
  do {
    v /= 1000;
    i++;
  } while (v >= 1000 && i < units.length - 1);
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

/// 行星设置(参数面同 iOS PlanetSettingsView):改名/图标仅 custom 可编辑;
/// 只读 WorldID(16 位十六进制)/节点 ID/类型/文件大小/修订时间;分享+删除
export default function PlanetSettingsDialog({ planet, onClose, onDone }: {
  planet: PlanetStatus; onClose: () => void; onDone: () => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const custom = planet.type === 'custom';
  /// 瞬时型弹层:内部持有 open,onClose 只翻 state 走滑出动画,动画结束
  /// (DrawerShell onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  /// 编辑初值:空 icon/color 用回退值落定(打开选择器即见当前有效形态)
  const [initialIcon] = useState(() => planet.icon || animalFor(null, planet.name));
  const [initialColor] = useState(() => planet.color || colorFor(null, planet.id));
  const [name, setName] = useState(planet.name);
  const [icon, setIcon] = useState(initialIcon);
  const [color, setColor] = useState(initialColor);
  const [showPicker, setShowPicker] = useState(false);
  const [confirmDel, setConfirmDel] = useState(false);
  const [qrData, setQrData] = useState<Uint8Array | null>(null);
  const detail = useQuery({ queryKey: ['planet', planet.id], queryFn: () => api.planetDetail(planet.id) });

  const onErr = (e: unknown) => {
    const ge = asError(e);
    const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
    toast.error(`${ge?.message ?? String(e)}${cand}`);
  };
  /// 未改动不落 PATCH(iOS 同语义:仅显式修改才保存)
  const dirty = custom && (name.trim() !== planet.name || icon !== initialIcon || color !== initialColor);
  const update = useMutation({
    mutationFn: () => api.planetUpdate(planet.id, name.trim(), icon, color),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['status'] });
      void qc.invalidateQueries({ queryKey: ['planet', planet.id] });
      toast.ok(t('common.saved'));
    },
    onError: onErr,
  });
  const remove = useMutation({
    mutationFn: () => api.removePlanet(planet.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['status'] });
      void qc.invalidateQueries({ queryKey: ['networks'] });
      onDone();
      setConfirmDel(false);
      close();
    },
    onError: onErr,
  });
  /// 导出 .planet 文件:daemon bytes → dialog save → write_file_bytes(同 PlanetManager)
  const shareFile = async () => {
    try {
      const bytes = await api.planetExport(planet.id);
      const path = await save({ defaultPath: `${planet.name || planet.id}.planet` });
      if (!path) return;
      await api.writeBytes(path, bytes);
      toast.ok(t('common.saved'));
    } catch (e) {
      onErr(e);
    }
  };
  /// ZTP 二维码:planet 二进制 + name/icon/color 组容器(同 PlanetManager)
  const openQr = async () => {
    try {
      const bytes = await api.planetExport(planet.id);
      setQrData(planetPayloadContainer(Uint8Array.from(bytes), name.trim(), icon, color));
    } catch (e) {
      onErr(e);
    }
  };

  const d = detail.data;
  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose} wide testId="planet-settings">
        <header>
          <h3>{t('planet.settingsTitle', { name: planet.name })}</h3>
          <button className="btn icon" aria-label={t('common.close')} onClick={close}>×</button>
        </header>
        <section className="detail-section addplanet-preview">
          <PlanetIconView icon={icon} color={color} name={name || planet.name} id={planet.id} size={56} />
          {custom && <button className="btn" onClick={() => setShowPicker(true)}>{t('icon.title')}</button>}
        </section>
        {custom && (
          <section className="detail-section">
            <h4>{t('addplanet.name')}</h4>
            <input className="input" value={name} onChange={e => setName(e.target.value)} />
          </section>
        )}
        <section className="detail-section">
          <div className="kv"><span>World ID</span><span className="mono">{d ? d.worldId.toString(16).padStart(16, '0') : '-'}</span></div>
          <div className="kv"><span>{t('planet.node')}</span><span className="mono">{d?.lastNodeId ?? planet.nodeId ?? '-'}</span></div>
          <div className="kv"><span>{t('planet.kind')}</span><span>{t(`planet.kind.${planet.type}`)}</span></div>
          <div className="kv"><span>{t('planet.fileSize')}</span><span>{d ? locFileSize(d.fileSize) : '-'}</span></div>
          <div className="kv"><span>{t('planet.revision')}</span><span>{d ? new Date(d.revisionTimeMs).toLocaleString() : '-'}</span></div>
        </section>
        <div className="dialog-actions">
          {custom && <button className="btn danger" onClick={() => setConfirmDel(true)}>{t('common.delete')}</button>}
          <span className="grow" />
          <button className="btn" onClick={close}>{t('common.close')}</button>
          {custom && (
            <button className="btn primary" disabled={!dirty || name.trim() === '' || update.isPending} onClick={() => update.mutate()}>
              {t('common.save')}
            </button>
          )}
        </div>
        {custom && (
          <div className="share-row">
            <button className="btn" onClick={() => void shareFile()}>{t('planets.shareFile')}</button>
            <button className="btn" onClick={() => void openQr()}>{t('planets.shareQr')}</button>
          </div>
        )}
      </DrawerShell>
      {/* 图标选择/确认框/二维码与设置抽屉平级(DrawerShell 经 portal 挂 body):避免嵌在
          transform 抽屉内被裁剪,且后挂者居上 */}
      {showPicker && (
        <IconPicker
          animal={icon}
          color={color}
          onSelect={(a, c) => {
            setIcon(a);
            setColor(c);
          }}
          onClose={() => setShowPicker(false)}
        />
      )}
      <ConfirmDialog
        open={confirmDel}
        title={t('common.delete')}
        message={planet.status === 'running'
          ? t('planets.removeRunningConfirm', { name: planet.name })
          : t('planets.removeConfirm', { name: planet.name })}
        confirmLabel={t('common.delete')}
        danger
        onConfirm={() => remove.mutate()}
        onCancel={() => setConfirmDel(false)}
      />
      {qrData && (
        <QRShareSheet
          title={t('planets.shareQrTitle', { name: planet.name })}
          subtitle={t('planets.shareQrHint')}
          data={qrData}
          onDismiss={() => setQrData(null)}
        />
      )}
    </>
  );
}
