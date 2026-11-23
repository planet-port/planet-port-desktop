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
import { Button } from './ui-x/Button';
import { Input } from './ui-x/Input';

/// .kv 语义类 Tailwind 对译(与报告速查表同口径;出处 styles.css 待全量替换后删除)
const sectionCard = 'flex flex-col rounded-md border border-[color:var(--border)] bg-[var(--panel)] dark:bg-[#22252d] dark:border-[#2c3038] px-2 py-0.5';
const rowDivider = 'relative before:absolute before:top-0 before:left-4 before:right-0 before:border-t before:border-[color:var(--border-soft)] before:content-[""]';
const kvRow = 'flex items-center justify-between gap-3 -mx-2 px-2 py-1.5 text-[13px]';
const kvButtonRow = `${kvRow} w-full cursor-pointer border-0 bg-transparent text-left rounded-[6px] transition-colors duration-[120ms] ease-[ease] enabled:hover:bg-[var(--panel)]`;
const kvLabel = 'shrink-0 text-[var(--text)]';
const kvVal = 'inline-flex min-w-0 items-center justify-end gap-1.5';
const chev = 'shrink-0 text-sm leading-none text-[var(--faint)]';
const groupTitle = 'mt-0.5 -mb-1.5 mr-2 text-xs font-medium text-[var(--muted)]';

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
        <header className="flex items-center justify-between pr-[30px]">
          <h3 className="m-0 text-sm font-medium">{t('addplanet.title')}</h3>
        </header>
        {/* 身份预览区(虚线占位卡):图标/名称/随机三件套,"尚未创建"的占位形态 */}
        <section className="mb-2 flex flex-col items-center gap-3 rounded-md border border-dashed border-[color:var(--border-h)] px-3.5 py-5">
          <button className="plain" aria-label={t('icon.title')} onClick={() => setShowPicker(true)}>
            <PlanetIconView icon={animal} color={color} name={name || animal} id={animal} size={56} />
          </button>
          <Input
            className="w-[220px] text-center"
            placeholder={t('addplanet.namePlaceholder')}
            value={name}
            aria-label={t('addplanet.name')}
            onChange={e => setName(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && canAdd && add.mutate()}
          />
          <Button variant="icon" aria-label={t('addplanet.randomIcon')} title={t('addplanet.randomIcon')} onClick={rollIdentity}>↻</Button>
        </section>
        <h4 className={groupTitle}>{t('addplanet.source')}</h4>
        <section className={sectionCard}>
          <button className={kvButtonRow} onClick={() => void pickFile()}>
            <span className={kvLabel}>{t('addplanet.chooseFile')}</span>
            {source && !/^https?:\/\//.test(source) ? (
              <span className="text-xs text-[var(--ok)]">✓</span>
            ) : (
              <span className={chev} aria-hidden="true">›</span>
            )}
          </button>
          <button className={`${kvButtonRow} ${rowDivider}`} onClick={() => setShowScanner(true)}>
            <span className={kvLabel}>{t('addplanet.scanImport')}</span>
            <span className={chev} aria-hidden="true">›</span>
          </button>
          <div className={`${kvRow} ${rowDivider}`}>
            <span className={kvLabel}>URL</span>
            <span className={`${kvVal} flex-1`}>
              <Input
                className="w-full min-w-0 text-right font-mono"
                placeholder={t('addplanet.urlPlaceholder')}
                value={url}
                onChange={e => setUrl(e.target.value)}
                onKeyDown={e => {
                  if (e.key === 'Enter' && url.trim()) setSource(url.trim());
                }}
              />
              <Button size="sm" disabled={!url.trim()} onClick={() => setSource(url.trim())}>{t('common.import')}</Button>
            </span>
          </div>
          {source && <p className={`${rowDivider} m-0 select-text break-all text-xs font-mono text-[var(--muted)]`}>{source}</p>}
        </section>
        <Button variant="primary" block className="mt-2.5" disabled={!canAdd} onClick={() => add.mutate()}>
          {t('addplanet.action')}
        </Button>
        {/* 主流程(自定义来源)与官方一键添加二选一,细分割隔开 */}
        <div className="mx-1.5 h-px bg-[color:var(--border-soft)]" aria-hidden="true" />
        <Button block className="mt-2.5" data-testid="add-official-planet" disabled={official.isPending} onClick={() => official.mutate()}>
          {t('addplanet.official')}
        </Button>
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
