import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { api, asError } from '../api/client';
import { randomAnimal, randomColorHex } from '../icons/planetIcon';
import IconPicker from './IconPicker';
import PlanetIconView from './PlanetIconView';
import { useToast } from './Toaster';

/// 添加行星抽屉(字段同 iOS AddPlanetForm):图标预览(随机)+名称必填+
/// 文件源双通道(文件选择/URL;扫码接收桌面无摄像头不做)。提交即
/// POST /planets {source,name,icon,color};worldId 去重由 daemon 409 承载
export default function AddPlanetDrawer({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const toast = useToast();
  const [animal, setAnimal] = useState(randomAnimal());
  const [color, setColor] = useState(randomColorHex());
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [source, setSource] = useState(''); // 文件路径或 URL,二选一后填
  const [showPicker, setShowPicker] = useState(false);

  const add = useMutation({
    mutationFn: () => api.addPlanet(source.trim(), name.trim(), animal, color),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['status'] });
      void qc.invalidateQueries({ queryKey: ['planets'] });
      void qc.invalidateQueries({ queryKey: ['networks'] });
      toast.ok(t('addplanet.imported'));
      onClose();
    },
    onError: e => {
      const ge = asError(e);
      const cand = ge?.candidates?.length ? `\n${ge.candidates.join(', ')}` : '';
      toast.error(`${ge?.message ?? String(e)}${cand}`);
    },
  });

  const pickFile = async () => {
    const picked = await open({ multiple: false, directory: false, title: t('addplanet.chooseFile') });
    if (!picked || Array.isArray(picked)) return;
    setSource(picked);
    setUrl('');
  };
  const canAdd = source.trim() !== '' && name.trim() !== '' && !add.isPending;

  return (
    <>
      <div className="drawer-mask" onClick={onClose} />
      <aside className="drawer" data-testid="add-planet-drawer">
        <header>
          <h3>{t('addplanet.title')}</h3>
          <button className="btn icon" onClick={onClose}>×</button>
        </header>
        <section className="drawer-section addplanet-preview">
          <button className="plain" aria-label={t('icon.title')} onClick={() => setShowPicker(true)}>
            <PlanetIconView icon={animal} color={color} name={name || animal} id={animal} />
          </button>
          <button className="btn" onClick={() => { setAnimal(randomAnimal()); setColor(randomColorHex()); }}>
            {t('addplanet.randomIcon')}
          </button>
        </section>
        <section className="drawer-section">
          <h4>{t('addplanet.name')}</h4>
          <input className="input" placeholder={t('addplanet.namePlaceholder')} value={name}
            onChange={e => setName(e.target.value)} />
        </section>
        <section className="drawer-section">
          <h4>{t('addplanet.source')}</h4>
          <button className="btn" onClick={() => void pickFile()}>{t('addplanet.chooseFile')}</button>
          {source && !url && <p className="small mono">{source}</p>}
          <div className="url-row">
            <input className="input" placeholder={t('addplanet.urlPlaceholder')} value={url}
              onChange={e => setUrl(e.target.value)} />
            <button className="btn" disabled={!url.trim()} onClick={() => setSource(url.trim())}>{t('common.import')}</button>
          </div>
        </section>
        <button className="btn primary" disabled={!canAdd} onClick={() => add.mutate()}>
          {t('addplanet.action')}
        </button>
      </aside>
      {showPicker && (
        <IconPicker
          animal={animal}
          color={color}
          onSelect={(a, c) => { setAnimal(a); setColor(c); }}
          onClose={() => setShowPicker(false)}
        />
      )}
    </>
  );
}
