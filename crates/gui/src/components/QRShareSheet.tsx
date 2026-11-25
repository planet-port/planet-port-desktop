import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import * as QRCode from 'qrcode';
import { encodeFrames, QRTooLargeError } from '../ztp/frames';
import DrawerShell from './DrawerShell';
import { Button } from './ui-x/Button';

/// 二维码分享抽屉:单帧静态直出保存按钮;多帧轮播(速度四档+手动切换),
/// 与移动端 QRShareSheet 行为一致。编码在 useMemo、渲染 toDataURL 异步逐张。
/// 容器统一右侧抽屉(DrawerShell,2026-11-24 口径):瞬时型时序——内部持有
/// open,onClose 只翻 state 走退出动画,动画结束(onClosed)才回调 onDismiss
/// 让父层卸载。内容布局类对译基线 .qr-* 系(styles.css 已删,本组件独占):
/// 居中排版 380px,图 260px pixelated,计数徽标挂图右下角
const SPEEDS = [0.3, 0.5, 0.8, 1.2];

export default function QRShareSheet({
  title,
  data,
  subtitle,
  onCopy,
  onSave,
  onDismiss,
}: {
  title: string;
  data: Uint8Array;
  subtitle?: string;
  /// 单帧时的文本复制动作(如网络 ID);多帧(行星数据轮播)无对应单值,不传
  onCopy?: () => void;
  onSave?: (dataUrl: string) => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  /// 瞬时型弹层:内部持有 open,onClose 只翻 state 走退出动画,动画结束
  /// (DrawerShell onClosed)才回调 onDismiss 让父层卸载
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  const frames = useMemo(() => {
    try {
      return encodeFrames(data);
    } catch (e) {
      if (e instanceof QRTooLargeError) return null;
      throw e;
    }
  }, [data]);
  const [urls, setUrls] = useState<string[]>([]);
  const [index, setIndex] = useState(0);
  const [speed, setSpeed] = useState(1); // SPEEDS 下标,默认 0.5s
  const [manual, setManual] = useState(false);

  // 逐帧生成 dataURL(多帧时可能几十张,先出首帧再补)
  useEffect(() => {
    let alive = true;
    setUrls([]);
    setIndex(0);
    if (!frames) return;
    void (async () => {
      const out: string[] = [];
      for (const f of frames) {
        const url = await QRCode.toDataURL(f, { errorCorrectionLevel: 'M', margin: 2, scale: 8 });
        if (!alive) return;
        out.push(url);
        setUrls([...out]);
      }
    })();
    return () => {
      alive = false;
    };
  }, [frames]);

  // 自动轮播:全部就绪后按速度档循环;手动模式暂停
  useEffect(() => {
    if (manual || !frames || frames.length <= 1 || urls.length !== frames.length) return;
    const id = setInterval(() => setIndex(i => (i + 1) % frames.length), SPEEDS[speed] * 1000);
    return () => clearInterval(id);
  }, [manual, speed, frames, urls.length]);

  return (
    <DrawerShell open={open} onClose={close} onClosed={onDismiss}>
      <header className="flex items-center justify-between pr-[30px]">
        <h3 className="m-0 text-sm font-medium">{title}</h3>
      </header>
        <div className="w-[380px] text-center">
          {subtitle && <p className="small">{subtitle}</p>}
          {frames === null ? (
            <div className="py-12 text-center text-[var(--muted)]">{t('qrtransfer.tooLarge')}</div>
          ) : urls.length === 0 ? (
            <div className="py-12 text-center text-[var(--muted)]">{t('qrtransfer.generating')}</div>
          ) : (
            <>
              <div className="relative my-2 inline-block">
                <img src={urls[Math.min(index, urls.length - 1)]} alt="QR" className="size-[260px] [image-rendering:pixelated]" />
                {frames.length > 1 && (
                  <span className="absolute -right-2 -bottom-2 rounded-full border border-border bg-[var(--panel)] px-2.5 py-px text-xs text-[var(--muted)]">
                    {index + 1}/{frames.length}
                  </span>
                )}
              </div>
              {frames.length > 1 && (
                <div className="mt-1.5 flex items-center justify-center gap-3.5">
                  <label className="small">
                    <input type="checkbox" checked={manual} onChange={e => setManual(e.target.checked)} /> {t('qrtransfer.manual')}
                  </label>
                  <div className="flex gap-1">
                    {SPEEDS.map((s, i) => (
                      <Button key={s} size="sm" variant={speed === i ? 'active' : 'default'} onClick={() => setSpeed(i)}>
                        {s}s
                      </Button>
                    ))}
                  </div>
                  {manual && (
                    <div className="flex gap-1">
                      <Button onClick={() => setIndex(i => (i - 1 + frames.length) % frames.length)}>‹</Button>
                      <Button onClick={() => setIndex(i => (i + 1) % frames.length)}>›</Button>
                    </div>
                  )}
                </div>
              )}
              {frames.length === 1 && (onCopy || onSave) && (
                <div className="mt-4 flex justify-center gap-2">
                  {onCopy && <Button onClick={onCopy}>{t('guide.copy')}</Button>}
                  {onSave && <Button variant="primary" onClick={() => onSave(urls[0])}>{t('qrtransfer.savePng')}</Button>}
                </div>
              )}
            </>
          )}
        </div>
    </DrawerShell>
  );
}
