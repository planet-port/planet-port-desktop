import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import { Button } from './ui-x/Button';
import { Dialog, DialogContent, DialogTitle } from './ui-x/Dialog';

/// 重装等待兜底时限:后端自身 15/30s 有 daemon-online 超时,失败通常 1 分钟内
/// 回报;3 分钟纯防事件丢失/进程卡死(提权进程无法中止,超时≠失败)
const WAIT_TIMEOUT_MS = 180_000;
/// 成功态停留时长后自动关
const DONE_CLOSE_MS = 1_500;

type Phase = 'confirm' | 'waiting' | 'done' | 'failed' | 'timeout';

/// install-failed 短枚举串(bootstrap.rs stage_and_spawn)→ 文案键;
/// 不在表内(含提权前 rejection)走 failGeneric 并展示原始原因
const FAIL_REASON_KEYS: Record<string, string> = {
  'install-exit-nonzero': 'settings.failExit',
  'daemon-timeout': 'settings.failTimeout',
};

/// 重装 daemon 弹窗五阶段机:confirm → waiting → done/failed/timeout。
/// 安装全程弹窗保持打开(等待中任何关闭路径 = 取消等待,提权进程后台继续,
/// 结果由 SettingsDrawer 的 install-done/failed 兜底监听以 toast 回报);
/// done 停留 1.5s 自动关;3 分钟无任何事件转超时问询(继续等待/取消等待)。
export default function ReinstallDialog({
  open, embeddedDaemon, installed, onClose, onDone,
}: {
  open: boolean;
  embeddedDaemon: string;
  installed: boolean;
  onClose: () => void;
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>('confirm');
  const [reason, setReason] = useState('');
  /// 事件回调持有旧闭包读不到最新 phase,镜像进 ref
  const phaseRef = useRef(phase);
  const cbRef = useRef({ onClose, onDone });
  const waitTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    phaseRef.current = phase;
    cbRef.current = { onClose, onDone };
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
    if (!open) return;
    // 重开一律回确认态,不残留上一次流程的成败/超时画面
    setPhase('confirm');
    setReason('');
    const unsubs = [
      listen('install-progress', () => {
        // 超时问询开着时来进度 = 后端其实还活着:退回等待并重挂兜底计时
        if (phaseRef.current === 'timeout') {
          setPhase('waiting');
          armTimeout();
        }
      }),
      listen('install-done', () => {
        clearTimeout(waitTimer.current);
        setPhase('done');
        cbRef.current.onDone();
        closeTimer.current = setTimeout(() => cbRef.current.onClose(), DONE_CLOSE_MS);
      }),
      listen<string>('install-failed', e => {
        clearTimeout(waitTimer.current);
        setReason(String(e.payload));
        setPhase('failed');
      }),
    ].map(p => p.then(f => f));
    return () => {
      clearTimeout(waitTimer.current);
      clearTimeout(closeTimer.current);
      for (const u of unsubs) void u.then(f => f());
    };
    // 回调经 cbRef 取最新值,依赖只留 open,防父层每次渲染内联回调引发重订阅
  }, [open]);

  const failKey = FAIL_REASON_KEYS[reason];
  const title = installed ? t('settings.reinstallDaemon') : t('settings.installDaemon');
  return (
    <Dialog open={open} onOpenChange={o => { if (!o) onClose(); }}>
      <DialogContent testId="reinstall-dialog" className="w-[380px]">
        <DialogTitle>{title}</DialogTitle>
        {phase === 'confirm' && (
          <>
            <p className="m-0 leading-[1.6]">{t('settings.reinstallDesc', { version: embeddedDaemon })}</p>
            <div className="mt-4 flex justify-end gap-2">
              <Button onClick={onClose}>{t('common.cancel')}</Button>
              <Button variant="primary" data-testid="reinstall-confirm" onClick={beginInstall}>{t('common.confirm')}</Button>
            </div>
          </>
        )}
        {phase === 'waiting' && (
          <div className="flex items-center gap-2.5" data-testid="reinstall-waiting">
            <span className="inline-block size-3.5 animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--primary)] [animation-duration:0.8s]" aria-label="installing" />
            <span className="text-[13px] text-muted-foreground">{t('settings.installing')}</span>
          </div>
        )}
        {phase === 'done' && (
          <p className="m-0 leading-[1.6]" data-testid="reinstall-done">
            ✓ {t('settings.installSuccess')} · daemon v{embeddedDaemon}
          </p>
        )}
        {phase === 'failed' && (
          <div data-testid="reinstall-failed">
            <p className="m-0 leading-[1.6] text-destructive">
              {failKey ? t(failKey) : t('settings.failGeneric')}
              {failKey ? null : <code className="ml-1 font-mono text-xs text-muted-foreground">{reason}</code>}
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <Button onClick={onClose}>{t('common.close')}</Button>
              <Button variant="primary" onClick={beginInstall}>{t('settings.retry')}</Button>
            </div>
          </div>
        )}
        {phase === 'timeout' && (
          <div data-testid="reinstall-timeout">
            <p className="m-0 leading-[1.6]">{t('settings.installTimeout')}</p>
            <div className="mt-4 flex justify-end gap-2">
              <Button onClick={onClose}>{t('settings.stopWaiting')}</Button>
              <Button variant="primary" data-testid="reinstall-keep-waiting" onClick={() => { setPhase('waiting'); armTimeout(); }}>{t('settings.keepWaiting')}</Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
