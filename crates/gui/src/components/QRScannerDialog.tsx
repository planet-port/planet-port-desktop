import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import jsQR from 'jsqr';

/// 扫码结果提取器(同 iOS QRScannerView.extract):返回 null 表示不是目标
/// 内容继续扫。网络场景取 16hex;行星场景取 http(s) 链接(扫码导入只处理
/// 链接二维码 → 回填 URL 下载,ZTP 动态分片聚合接收暂不做)
export function extractNetworkId(s: string): string | null {
  const m = /[0-9a-fA-F]{16}/.exec(s.trim());
  return m ? m[0].toLowerCase() : null;
}

export function extractHttpUrl(s: string): string | null {
  const t = s.trim();
  return /^https?:\/\//.test(t) ? t : null;
}

/// 摄像头扫码弹层(iOS QRScannerView 对位):getUserMedia 实时取流,200ms
/// 抽帧 jsQR 解码,提取器命中即回调并停流。遮罩/关闭即释放摄像头。
/// 权限被拒时给出系统设置引导(mac 无 deep-link 到隐私面板,文本指引)
export default function QRScannerDialog({
  extract,
  onScanned,
  onClose,
}: {
  extract: (s: string) => string | null;
  onScanned: (value: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const videoRef = useRef<HTMLVideoElement>(null);
  const [denied, setDenied] = useState(false);
  useEffect(() => {
    let stream: MediaStream | null = null;
    let timer: number | undefined;
    let stopped = false;
    navigator.mediaDevices
      .getUserMedia({ video: true })
      .then(s => {
        if (stopped) {
          s.getTracks().forEach(tr => tr.stop());
          return;
        }
        stream = s;
        const v = videoRef.current;
        if (!v) return;
        v.srcObject = s;
        void v.play();
        const canvas = document.createElement('canvas');
        const ctx = canvas.getContext('2d', { willReadFrequently: true });
        if (!ctx) return;
        timer = window.setInterval(() => {
          if (!v.videoWidth || !v.videoHeight) return;
          canvas.width = v.videoWidth;
          canvas.height = v.videoHeight;
          ctx.drawImage(v, 0, 0);
          const img = ctx.getImageData(0, 0, canvas.width, canvas.height);
          const code = jsQR(img.data, img.width, img.height);
          if (!code?.data) return;
          const val = extract(code.data);
          if (val) {
            window.clearInterval(timer);
            onScanned(val);
          }
        }, 200);
      })
      .catch(() => setDenied(true));
    return () => {
      stopped = true;
      window.clearInterval(timer);
      stream?.getTracks().forEach(tr => tr.stop());
    };
  }, [extract, onScanned]);
  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog qr-scan" onClick={e => e.stopPropagation()}>
        <h3>{t('qr.scanTitle')}</h3>
        {denied ? (
          <div className="qr-denied">
            <p className="dialog-message">{t('qr.deniedTitle')}</p>
            <p className="small">{t('qr.deniedMessage')}</p>
          </div>
        ) : (
          <video ref={videoRef} className="qr-video" playsInline muted aria-label={t('qr.scanTitle')} />
        )}
      </div>
    </div>
  );
}
