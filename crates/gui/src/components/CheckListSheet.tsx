import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogTitle } from './ui-x/Dialog';

/// iOS 设置下级选择页通用形态:选项行 + 右侧 ✓ 勾选,点击即选即生效。
/// 上级是「值行 + ›」,本层是二层弹层(遮罩/关闭钮返回上级)。
/// onPick 返回 true 表示已处理并关闭;返回 false 保持打开(交上级流程)。
/// 容器迁 shadcn Dialog(ui-x/Dialog):瞬时型时序——内部持有 open,onClose 只翻
/// state 走退出动画,动画结束(onClosed)才回调父层 onClose 清理(父层条件渲染才卸载)。
/// 选项行/勾选标(.kv/.check)与抽屉批共用,继续走 styles.css 规则
export default function CheckListSheet({
  title, options, value, onPick, onClose, testId,
}: {
  title: string;
  options: { value: string; label: string; disabled?: boolean }[];
  value: string;
  onPick: (v: string) => boolean;
  onClose: () => void;
  testId?: string;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const close = () => setOpen(false);
  return (
    <Dialog open={open} onOpenChange={o => { if (!o) close(); }} onClosed={onClose}>
      <DialogContent testId={testId} closeLabel={t('common.close')}>
        <DialogTitle>{title}</DialogTitle>
        <section className="drawer-section">
          {options.map(o => (
            <button
              key={o.value}
              className="kv"
              data-testid={testId ? `${testId}-${o.value}` : undefined}
              disabled={o.disabled}
              onClick={() => { if (onPick(o.value)) close(); }}
            >
              <span>{o.label}</span>
              {value === o.value && <span className="check" aria-hidden="true">✓</span>}
            </button>
          ))}
        </section>
      </DialogContent>
    </Dialog>
  );
}
