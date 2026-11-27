import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import logo from '../assets/icon-1024.png';
import { Button } from './ui-x/Button';
import { FAIL_REASON_KEYS } from './ReinstallDialog';

/// 等待兜底时限与 ReinstallDialog 同源(提权进程无法中止,超时≠失败)
const WAIT_TIMEOUT_MS = 180_000;

type Phase = 'waiting' | 'failed' | 'timeout';

/// 启动安装门:daemon 未装(token 文件不存在)且 GUI 携带内嵌资源时全屏
/// 等待自动安装(用户口径:不询问,装好直接进首页)。事件链与
/// ReinstallDialog 同源:install-progress / done / failed;装好即落门,
/// 失败/超时门内重试或放弃(放弃进首页,设置抽屉仍可手动安装)
export default function BootInstallGate({ onFinished }: { onFinished: () => void }) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>('waiting');
  const [reason, setReason] = useState('');
  /// 事件回调持有旧闭包读不到最新 phase,镜像进 ref
  const phaseRef = useRef(phase);
  const finishedRef = useRef(onFinished);
  const waitTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    phaseRef.current = phase;
    finishedRef.current = onFinished;
  });

  const armTimeout = () => {
    clearTimeout(waitTimer.current);
    waitTimer.current = setTimeout(() => setPhase('timeout'), WAIT_TIMEOUT_MS);
  };
  const beginInstall = () => {
    setReason('');
    setPhase('waiting');
    armTimeout();
    api.startInstall().catch((e: unknown) => {
      const msg = String(e);
      // 后端已有安装在跑:保持等待,结果仍走事件回报
      if (msg.includes('already in progress')) return;
      // 提权前失败(缺资源/拷贝 staging)不会起后台线程,事件不会来,就地转失败态
      clearTimeout(waitTimer.current);
      setReason(msg);
      setPhase('failed');
    });
  };

  useEffect(() => {
    beginInstall();
    const unsubs = [
      listen('install-progress', () => {
        // 超时态下来进度 = 后端其实还活着:退回等待并重挂兜底计时
        if (phaseRef.current === 'timeout') {
          setPhase('waiting');
          armTimeout();
        }
      }),
      listen('install-done', () => {
        clearTimeout(waitTimer.current);
        finishedRef.current();
      }),
      listen<string>('install-failed', e => {
        clearTimeout(waitTimer.current);
        setReason(String(e.payload));
        setPhase('failed');
      }),
    ].map(p => p.then(f => f));
    return () => {
      clearTimeout(waitTimer.current);
      for (const u of unsubs) void u.then(f => f());
    };
    // 挂载即自动开始,流程只跑一次(beginInstall 内部状态自洽,无需依赖)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const failKey = FAIL_REASON_KEYS[reason];
  return (
    <div
      className="fixed inset-0 z-50 flex flex-col items-center justify-center gap-4 bg-background"
      data-testid="boot-install-gate"
    >
      <img src={logo} alt="logo" className="size-16 rounded-[12px]" />
      {phase === 'waiting' && (
        <div className="flex items-center gap-2.5" data-testid="boot-install-waiting">
          <span
            className="inline-block size-3.5 animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--primary)] [animation-duration:0.8s]"
            aria-label="installing"
          />
          <span className="text-[13px] text-muted-foreground">{t('settings.installing')}</span>
        </div>
      )}
      {phase === 'failed' && (
        <div className="flex flex-col items-center gap-3" data-testid="boot-install-failed">
          <p className="m-0 max-w-[320px] text-center leading-[1.6] text-destructive">
            {failKey ? t(failKey) : t('settings.failGeneric')}
            {failKey ? null : <code className="ml-1 font-mono text-xs text-muted-foreground">{reason}</code>}
          </p>
          <div className="flex gap-2">
            <Button onClick={onFinished}>{t('settings.skipInstall')}</Button>
            <Button variant="primary" onClick={beginInstall}>{t('settings.retry')}</Button>
          </div>
        </div>
      )}
      {phase === 'timeout' && (
        <div className="flex flex-col items-center gap-3" data-testid="boot-install-timeout">
          <p className="m-0 max-w-[320px] text-center leading-[1.6]">{t('settings.installTimeout')}</p>
          <div className="flex gap-2">
            <Button onClick={onFinished}>{t('settings.skipInstall')}</Button>
            <Button variant="primary" onClick={() => { setPhase('waiting'); armTimeout(); }}>
              {t('settings.keepWaiting')}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
