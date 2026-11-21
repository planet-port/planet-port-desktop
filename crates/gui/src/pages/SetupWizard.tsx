import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { api } from '../api/client';
import type { EmbeddedManifest } from '../api/types';
import CmdBlock from '../components/CmdBlock';
import type { InstallPhase } from './setup';

/// 安装失败 reason → i18n key;事件通道只承载这两个枚举(其余 reject 串走 failGeneric)
const FAIL_KEY: Record<string, string> = {
  'install-exit-nonzero': 'guide.failExit',
  'daemon-timeout': 'guide.failTimeout',
};

/// 安装态 UI:内嵌资源一键安装/升级。成败不经 invoke 返回值,走 install-done/failed 事件;
/// 成功后本组件不做任何动作 —— daemon-online 事件让 App 切主界面
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

  useEffect(() => {
    const unsubs = [
      listen('install-progress', () => setPhase('installing')),
      // done 后 daemon-online 接管切页;维持安装态避免成功瞬间闪出按钮
      listen('install-done', () => setPhase('installing')),
      listen<string>('install-failed', e => {
        setPhase('failed');
        setReason(String(e.payload));
      }),
    ].map(p => p.then(f => f));
    // 每个 unlisten 必须真正执行,否则重复挂载后监听泄漏
    return () => {
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
    <div className="wizard">
      <h2>{t('guide.autoTitle')}</h2>
      <p className="muted">{t('guide.autoDesc', { version: manifest.version })}</p>
      {phase === 'failed' ? (
        <>
          <div className="fail" data-testid="install-fail">
            <p>{failKey ? t(failKey) : t('guide.failGeneric')}</p>
            {/* 非枚举 reason(invoke reject 串)原样展示,错误详情 = 详情 + 日志指引 */}
            {failKey ? null : (
              <p className="small">
                <code className="mono">{reason}</code>
              </p>
            )}
            <button
              className="btn"
              data-testid="install-retry"
              onClick={() => {
                setReason('');
                setPhase('idle');
              }}
            >
              {t('guide.retry')}
            </button>
          </div>
          <CmdBlock label={t('guide.logs')} cmd={`${sudo}planet-port service logs`} />
        </>
      ) : (
        <p>
          <button
            className="btn primary"
            data-testid="install-btn"
            disabled={phase === 'installing'}
            onClick={startInstall}
          >
            {phase === 'installing'
              ? t('guide.installing')
              : installed
                ? t('guide.upgrade', { version: manifest.version })
                : t('guide.installNow')}
          </button>
        </p>
      )}
    </div>
  );
}
