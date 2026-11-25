import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import type { EmbeddedManifest } from '../api/types';
import CmdBlock from '../components/CmdBlock';
import { Button } from '../components/ui-x/Button';
import type { InstallPhase } from './setup';

/// install-done 后等待 daemon-online 的时限:超时本地转 failed,交还重试出口
const DONE_WAIT_MS = 15_000;

/// 安装失败 reason → i18n key;事件通道只承载这两个枚举(其余 reject 串走 failGeneric)
const FAIL_KEY: Record<string, string> = {
  'install-exit-nonzero': 'guide.failExit',
  'daemon-timeout': 'guide.failTimeout',
};

/// 安装态 UI:内嵌资源一键安装/升级。成败不经 invoke 返回值,走 install-done/failed 事件;
/// 成功后 daemon-online 事件让 App 切主界面,本组件仅做有限等待兜底(见 DONE_WAIT_MS)
export default function SetupWizard({
  manifest,
  installed,
  sudo,
}: {
  manifest: EmbeddedManifest;
  installed: boolean;
  sudo: string;
}) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<InstallPhase>('idle');
  const [reason, setReason] = useState('');
  // install-done 兜底计时器句柄:失败事件或组件卸载时撤除
  const doneTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    const unsubs = [
      listen('install-progress', () => setPhase('installing')),
      // done 后 daemon-online 接管切页;维持安装态避免成功瞬间闪出按钮。
      // 但 tcp_probe 裸 TCP 误判(API 端口被无关进程占用)时 Rust 照发 done,
      // HTTP 探活却判离线,daemon-online 永不到达 —— 故起 15s 有限等待,
      // 超时按 daemon-timeout 转 failed(failTimeout 文案),避免死锁在「正在安装」
      listen('install-done', () => {
        setPhase('installing');
        clearTimeout(doneTimer.current);
        doneTimer.current = setTimeout(() => {
          setPhase('failed');
          setReason('daemon-timeout');
        }, DONE_WAIT_MS);
      }),
      listen<string>('install-failed', e => {
        // 真实失败事件优先于本地超时兜底,先撤掉待触发的计时器以免覆盖 reason
        clearTimeout(doneTimer.current);
        setPhase('failed');
        setReason(String(e.payload));
      }),
    ].map(p => p.then(f => f));
    // 每个 unlisten 必须真正执行,否则重复挂载后监听泄漏
    return () => {
      // daemon-online 到达 → App 切页卸载本组件,计时器随之撤除
      clearTimeout(doneTimer.current);
      for (const u of unsubs) void u.then(f => f());
    };
  }, []);

  const startInstall = () => {
    setReason('');
    // 乐观置安装态:点击到事件回路之间禁止重复触发(与后端 in-flight 位互为纵深)
    setPhase('installing');
    api
      .startInstall()
      .catch((e: unknown) => {
        const msg = String(e);
        // 后端已有安装在跑:保持 installing,等事件回报结果
        if (msg.includes('already in progress')) return;
        // 提权前失败(缺资源/拷贝 staging)走 invoke rejection,事件不会来,就地转 failed
        setPhase('failed');
        setReason(msg);
      });
  };

  const failKey = FAIL_KEY[reason];
  return (
    <div>
      <h2 className="mb-1.5 text-[17px] font-semibold">{t('guide.autoTitle')}</h2>
      <p className="text-muted-foreground">{t('guide.autoDesc', { version: manifest.daemon })}</p>
      {phase === 'failed' ? (
        <>
          <div className="text-destructive" data-testid="install-fail">
            <p className="my-2">{failKey ? t(failKey) : t('guide.failGeneric')}</p>
            {/* 非枚举 reason(invoke reject 串)原样展示,错误详情 = 详情 + 日志指引 */}
            {failKey ? null : (
              <p className="my-2 text-xs text-muted-foreground">
                <code className="font-mono">{reason}</code>
              </p>
            )}
            <Button
              data-testid="install-retry"
              onClick={() => {
                setReason('');
                setPhase('idle');
              }}
            >
              {t('guide.retry')}
            </Button>
          </div>
          <CmdBlock label={t('guide.logs')} cmd={`${sudo}planet-port service logs`} />
        </>
      ) : (
        <p>
          <Button
            variant="primary"
            data-testid="install-btn"
            disabled={phase === 'installing'}
            onClick={startInstall}
          >
            {phase === 'installing'
              ? t('guide.installing')
              : installed
                ? t('guide.upgrade', { version: manifest.daemon })
                : t('guide.installNow')}
          </Button>
        </p>
      )}
    </div>
  );
}
