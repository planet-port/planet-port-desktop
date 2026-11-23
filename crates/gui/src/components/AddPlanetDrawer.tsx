import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import { randomAnimal, randomColorHex } from '../icons/planetIcon';
import DrawerShell from './DrawerShell';
import IconPicker from './IconPicker';
import PlanetIconView from './PlanetIconView';
import QRScannerDialog, { extractHttpUrl } from './QRScannerDialog';
import { useToast } from './Toaster';

/// 添加行星抽屉(布局同 iOS AddPlanetPage/Form):身份预览区(虚线占位卡:
/// 图标点击进选择器 + 行内名称 + 随机重掷)+ 来源三通道(文件行/扫码导入行/
/// URL 行)。扫码只处理 http(s) 链接二维码回填 URL(ZTP 动态分片聚合接收
/// 暂不做)。提交即 POST /planets;worldId 去重由 daemon 409 承载
export default function AddPlanetDrawer({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  /// 瞬时型弹层:由 App 条件渲染挂载,内部持有 open,onClose 只翻 state 走
  /// 滑出动画,动画结束(DrawerShell onClosed)才回调父层 onClose 卸载
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const [animal, setAnimal] = useState(randomAnimal);
  const [color, setColor] = useState(randomColorHex);
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [source, setSource] = useState(''); // 文件路径或 URL,二选一后填
  const [showPicker, setShowPicker] = useState(false);
  const [showScanner, setShowScanner] = useState(false);

  const add = useMutation({
    mutationFn: () => api.addPlanet(source.trim(), name.trim(), animal, color),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['status'] });
      void qc.invalidateQueries({ queryKey: ['planets'] });
      void qc.invalidateQueries({ queryKey: ['networks'] });
      toast.ok(t('addplanet.imported'));
      close();
    },
    onError: e => {
      const ge = asError(e);
      const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
      toast.error(`${ge?.message ?? String(e)}${cand}`);
    },
  });
  /// 一键官方行星:Rust 内置官方 world,无需来源表单;专用图标+品牌黑底
  const official = useMutation({
    mutationFn: () => api.addOfficialPlanet('zerotier', '#161c24'),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['status'] });
      void qc.invalidateQueries({ queryKey: ['planets'] });
      void qc.invalidateQueries({ queryKey: ['networks'] });
      toast.ok(t('addplanet.imported'));
      close();
    },
    onError: e => {
      const ge = asError(e);
      const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
      toast.error(`${ge?.message ?? String(e)}${cand}`);
    },
  });

  const rollIdentity = () => {
    setAnimal(randomAnimal());
    setColor(randomColorHex());
  };
  const pickFile = async () => {
    const picked = await openDialog({ multiple: false, directory: false, title: t('addplanet.chooseFile') });
    if (!picked || Array.isArray(picked)) return;
    setSource(picked);
    setUrl('');
  };
  const canAdd = source.trim() !== '' && name.trim() !== '' && !add.isPending;

  return (
    <>
      <DrawerShell open={open} onClose={close} onClosed={onClose} testId="add-planet-drawer">
        <header>
          <h3>{t('addplanet.title')}</h3>
        </header>
        {/* 身份预览区(虚线占位卡):图标/名称/随机三件套,"尚未创建"的占位形态 */}
        <section className="addplanet-identity">
          <button className="plain" aria-label={t('icon.title')} onClick={() => setShowPicker(true)}>
            <PlanetIconView icon={animal} color={color} name={name || animal} id={animal} size={56} />
          </button>
          <input
            className="input identity-name"
            placeholder={t('addplanet.namePlaceholder')}
            value={name}
            aria-label={t('addplanet.name')}
            onChange={e => setName(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && canAdd && add.mutate()}
          />
          <button className="btn icon" aria-label={t('addplanet.randomIcon')} title={t('addplanet.randomIcon')} onClick={rollIdentity}>↻</button>
        </section>
        <h4 className="group-title">{t('addplanet.source')}</h4>
        <section className="detail-section">
          <button className="kv" onClick={() => void pickFile()}>
            <span>{t('addplanet.chooseFile')}</span>
            {source && !/^https?:\/\//.test(source) ? (
              <span className="small ready-mark">✓</span>
            ) : (
              <span className="chev" aria-hidden="true">›</span>
            )}
          </button>
          <button className="kv" onClick={() => setShowScanner(true)}>
            <span>{t('addplanet.scanImport')}</span>
            <span className="chev" aria-hidden="true">›</span>
          </button>
          <div className="kv url-kv">
            <span>URL</span>
            <span className="kv-val">
              <input
                className="input mono url-input"
                placeholder={t('addplanet.urlPlaceholder')}
                value={url}
                onChange={e => setUrl(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter' && url.trim()) setSource(url.trim());
                }}
              />
              <button className="btn sm" disabled={!url.trim()} onClick={() => setSource(url.trim())}>{t('common.import')}</button>
            </span>
          </div>
          {source && <p className="small mono source-path">{source}</p>}
        </section>
        <button className="btn primary btn-block" disabled={!canAdd} onClick={() => add.mutate()}>
          {t('addplanet.action')}
        </button>
        {/* 主流程(自定义来源)与官方一键添加二选一,细分割隔开 */}
        <div className="or-sep" aria-hidden="true" />
        <button className="btn btn-block" data-testid="add-official-planet" disabled={official.isPending} onClick={() => official.mutate()}>
          {t('addplanet.official')}
        </button>
      </DrawerShell>
      {/* 图标选择/扫码弹层与添加抽屉平级(DrawerShell 经 portal 挂 body):后挂者居上 */}
      {showPicker && (
        <IconPicker
          animal={animal}
          color={color}
          onSelect={(a, c) => { setAnimal(a); setColor(c); }}
          onClose={() => setShowPicker(false)}
        />
      )}
      {showScanner && (
        <QRScannerDialog
          extract={extractHttpUrl}
          onScanned={scanned => {
            setShowScanner(false);
            setUrl(scanned);
            setSource(scanned);
          }}
          onClose={() => setShowScanner(false)}
        />
      )}
    </>
  );
}
