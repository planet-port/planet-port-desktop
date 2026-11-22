import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import * as QRCode from 'qrcode';
import { encodeFrames, QRTooLargeError } from '../ztp/frames';

/// 二维码分享弹层:单帧静态直出保存按钮;多帧轮播(速度四档+手动切换),
/// 与移动端 QRShareSheet 行为一致。编码在 useMemo、渲染 toDataURL 异步逐张。
const SPEEDS = [0.3, 0.5, 0.8, 1.2];

export default function QRShareSheet({
  title,
  data,
  subtitle,
  onSave,
  onDismiss,
}: {
  title: string;
  data: Uint8Array;
  subtitle?: string;
  onSave?: (dataUrl: string) => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
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
    <div className="overlay" onClick={onDismiss}>
      <div className="dialog qr-sheet" onClick={e => e.stopPropagation()}>
        <h3>{title}</h3>
        {subtitle && <p className="small">{subtitle}</p>}
        {frames === null ? (
          <div className="empty">{t('qrtransfer.tooLarge')}</div>
        ) : urls.length === 0 ? (
          <div className="empty">{t('qrtransfer.generating')}</div>
        ) : (
          <>
            <div className="qr-stage">
              <img src={urls[Math.min(index, urls.length - 1)]} alt="QR" />
              {frames.length > 1 && <span className="qr-counter">{index + 1}/{frames.length}</span>}
            </div>
            {frames.length > 1 && (
              <div className="qr-controls">
                <label className="small">
                  <input type="checkbox" checked={manual} onChange={e => setManual(e.target.checked)} /> {t('qrtransfer.manual')}
                </label>
                <div className="qr-speeds">
                  {SPEEDS.map((s, i) => (
                    <button key={s} className={`btn qr-speed ${speed === i ? 'active' : ''}`} onClick={() => setSpeed(i)}>
                      {s}s
                    </button>
                  ))}
                </div>
                {manual && (
                  <div className="qr-manual">
                    <button className="btn" onClick={() => setIndex(i => (i - 1 + frames.length) % frames.length)}>‹</button>
                    <button className="btn" onClick={() => setIndex(i => (i + 1) % frames.length)}>›</button>
                  </div>
                )}
              </div>
            )}
            {frames.length === 1 && onSave && (
              <div className="dialog-actions">
                <button className="btn primary" onClick={() => onSave(urls[0])}>{t('qrtransfer.savePng')}</button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
