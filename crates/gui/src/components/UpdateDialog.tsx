import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Button } from './ui-x/Button';
import { Dialog, DialogContent, DialogTitle } from './ui-x/Dialog';

/// 检查更新四态:checking(请求中)/found(发现新版)/latest(已是最新)/failed(失败可重试)
export type UpdatePhase = 'checking' | 'found' | 'latest' | 'failed';

/// GUI 更新检查结果弹窗:phase 与数据都由 App 层持有(托盘入口与关于区入口共用),
/// 本组件纯受控展示。宽 380px 对译 .dialog.confirm;正文/按钮区间距同 ConfirmDialog
/// (行高 1.6 + mt-4 右对齐)。found 态「前往 GitHub 下载」经 opener 拉起系统浏览器
/// (ACL 仅放行 github.com),打开即关弹窗——浏览下载与弹窗生命周期无关。
export default function UpdateDialog({
  open, phase, current, latest, error, onClose, onRetry,
}: {
  open: boolean;
  phase: UpdatePhase;
  current: string;
  latest?: string;
  error?: string;
  onClose: () => void;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  const go = () => {
    void openUrl(
      `https://github.com/planet-port/planet-port-desktop/releases/tag/planet-port-gui/v${latest}`,
    );
    onClose();
  };
  return (
    <Dialog open={open} onOpenChange={o => { if (!o) onClose(); }}>
      <DialogContent testId="update-dialog" className="w-[380px]">
        <DialogTitle>
          {phase === 'found' ? t('update.foundTitle') : phase === 'latest' ? t('update.upToDateTitle') : phase === 'failed' ? t('update.failedTitle') : t('update.checking')}
        </DialogTitle>
        {phase === 'checking' && (
          <div className="flex items-center gap-2.5" data-testid="update-checking">
            <span className="inline-block size-3.5 animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--primary)] [animation-duration:0.8s]" aria-label="checking" />
            <span className="text-[13px] text-muted-foreground">{t('update.checking')}</span>
          </div>
        )}
        {phase === 'found' && (
          <>
            <p className="m-0 leading-[1.6]">{t('update.foundBody', { current, latest })}</p>
            <div className="mt-4 flex justify-end gap-2">
              <Button onClick={onClose}>{t('update.later')}</Button>
              <Button variant="primary" data-testid="update-go" onClick={go}>{t('update.goToGithub')}</Button>
            </div>
          </>
        )}
        {phase === 'latest' && (
          <>
            <p className="m-0 leading-[1.6]">{t('update.upToDateBody', { current })}</p>
            <div className="mt-4 flex justify-end"><Button variant="primary" onClick={onClose}>{t('common.confirm')}</Button></div>
          </>
        )}
        {phase === 'failed' && (
          <>
            <p className="m-0 leading-[1.6] break-words text-destructive">{error ?? '-'}</p>
            <div className="mt-4 flex justify-end gap-2">
              <Button onClick={onClose}>{t('common.close')}</Button>
              <Button variant="primary" onClick={onRetry}>{t('settings.retry')}</Button>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
