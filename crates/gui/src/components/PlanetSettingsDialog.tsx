import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import type { PlanetStatus } from '../api/types';
import ConfirmDialog from './ConfirmDialog';
import CopyableValue from './CopyableValue';
import DrawerShell from './DrawerShell';
import IconPicker from './IconPicker';
import PeersDialog from './PeersDialog';
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

/// 行星设置(布局同 iOS PlanetSettingsView,全部即时生效无保存按钮):
/// 名称与图标组(custom 行内编辑失焦即存/图标行选完即存,官方只读+footer)、
/// 只读信息组(节点 ID 可复制+分享行内联)、诊断组(Peers 入口;Moons/ApplyTrace
/// daemon 无 API 不做)、删除组居中红字(custom)
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
  /// 编辑态与已保存态分开跟踪:即时保存按"与上次已保存值有差异"判定,
  /// 避免每次失焦都发请求。空 icon/color 用回退值落定(打开选择器即见当前有效形态)
  const [name, setName] = useState(planet.name);
  const [savedName, setSavedName] = useState(planet.name);
  const [icon, setIcon] = useState(() => planet.icon || animalFor(null, planet.name));
  const [color, setColor] = useState(() => planet.color || colorFor(null, planet.id));
  const [savedIcon, setSavedIcon] = useState(icon);
  const [savedColor, setSavedColor] = useState(color);
  const [showPicker, setShowPicker] = useState(false);
  const [showPeers, setShowPeers] = useState(false);
  const [confirmDel, setConfirmDel] = useState(false);
  const [qrData, setQrData] = useState<Uint8Array | null>(null);
  const detail = useQuery({ queryKey: ['planet', planet.id], queryFn: () => api.planetDetail(planet.id) });

  const onErr = (e: unknown) => {
    const ge = asError(e);
    const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
    toast.error(`${ge?.message ?? String(e)}${cand}`);
  };
  /// 即时保存(全量 PATCH,未改动不发);成功静默失效缓存,失败 toast
  const update = useMutation({
    mutationFn: (next: { name: string; icon: string; color: string }) =>
      api.planetUpdate(planet.id, next.name, next.icon, next.color),
    onSuccess: (_d, next) => {
      setSavedName(next.name);
      setSavedIcon(next.icon);
      setSavedColor(next.color);
      void qc.invalidateQueries({ queryKey: ['status'] });
      void qc.invalidateQueries({ queryKey: ['planet', planet.id] });
    },
    onError: onErr,
  });
  /// 名称失焦/回车即存(空名不发)
  const saveName = () => {
    if (!custom || name.trim() === '' || name.trim() === savedName) return;
    update.mutate({ name: name.trim(), icon, color });
  };
  /// 图标选择完成(onClose 时机)统一保存一次,避免选色过程多次 PATCH
  const closePicker = () => {
    setShowPicker(false);
    if (custom && (icon !== savedIcon || color !== savedColor) && name.trim() !== '') {
      update.mutate({ name: name.trim(), icon, color });
    }
  };
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
  /// ZTP 二维码:planet 二进制 + name/icon/color 组容器(同 PlanetManager;
  /// 用已保存值——分享出去的应是服务端真值而非编辑中间态)
  const openQr = async () => {
    try {
      const bytes = await api.planetExport(planet.id);
      setQrData(planetPayloadContainer(Uint8Array.from(bytes), planet.name, planet.icon, planet.color));
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
        </header>
        <section className="detail-section">
          <div className="kv">
            <span>{t('addplanet.name')}</span>
            {custom ? (
              <input
                className="inline-edit"
                value={name}
                aria-label={t('addplanet.name')}
                onChange={e => setName(e.target.value)}
                onBlur={saveName}
                onKeyDown={e => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
              />
            ) : (
              <span className="mono">{planet.name}</span>
            )}
          </div>
          <div className="kv">
            <span>{t('planet.iconLabel')}</span>
            {custom ? (
              <button className="kv" onClick={() => setShowPicker(true)} aria-label={t('icon.title')}>
                <PlanetIconView icon={icon} color={color} name={name || planet.name} id={planet.id} size={20} />
                <span className="chev" aria-hidden="true">›</span>
              </button>
            ) : (
              <PlanetIconView icon={planet.icon} color={planet.color} name={planet.name} id={planet.id} size={20} />
            )}
          </div>
          {!custom && <p className="section-foot">{t('planet.officialFooter')}</p>}
        </section>
        <section className="detail-section">
          <div className="kv"><span>World ID</span><span className="mono">{d ? d.worldId.toString(16).padStart(16, '0') : '-'}</span></div>
          <div className="kv">
            <span>{t('planet.node')}</span>
            <CopyableValue text={d?.lastNodeId ?? planet.nodeId ?? '-'} />
          </div>
          <div className="kv"><span>{t('planet.kind')}</span><span>{t(`planet.kind.${planet.type}`)}</span></div>
          <div className="kv"><span>{t('planet.fileSize')}</span><span className="mono">{d ? locFileSize(d.fileSize) : '-'}</span></div>
          <div className="kv"><span>{t('planet.revision')}</span><span className="mono">{d ? new Date(d.revisionTimeMs).toLocaleString() : '-'}</span></div>
          {custom && (
            <div className="kv">
              <span>{t('planet.share')}</span>
              <span className="kv-val">
                <button className="link" onClick={() => void shareFile()}>{t('planets.shareFile')}</button>
                <button className="link" onClick={() => void openQr()}>{t('planets.shareQr')}</button>
              </span>
            </div>
          )}
        </section>
        <section className="detail-section">
          <h4>{t('planet.diagnostics')}</h4>
          <button className="kv" onClick={() => setShowPeers(true)}>
            <span>{t('peers.title')}</span>
            <span className="chev" aria-hidden="true">›</span>
          </button>
        </section>
        {custom && (
          <section className="detail-section">
            <button className="danger-row" onClick={() => setConfirmDel(true)}>{t('common.delete')}</button>
          </section>
        )}
      </DrawerShell>
      {/* 图标选择/节点表/确认框/二维码与设置抽屉平级(DrawerShell 经 portal 挂 body):
          避免嵌在 transform 抽屉内被裁剪,且后挂者居上 */}
      {showPicker && (
        <IconPicker
          animal={icon}
          color={color}
          onSelect={(a, c) => {
            setIcon(a);
            setColor(c);
          }}
          onClose={closePicker}
        />
      )}
      {showPeers && <PeersDialog planet={planet.id} onClose={() => setShowPeers(false)} />}
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
