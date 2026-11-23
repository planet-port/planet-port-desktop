import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Sheet } from './ui-x/Sheet';

/// 项目抽屉壳:ui-x/Sheet 的项目侧门面。对外 API(open/onClose/onClosed/testId/
/// children)与迁移前完全一致——三个抽屉与 App.tsx 的挂载结构(父层条件渲染、
/// 子层自持 open 滑出、onClosed 卸载)保持;多层堆叠、滑入滑出动画、Radix
/// portal/focus trap/Esc 全部由 ui-x/Sheet 承载,本层只注入关闭钮的 i18n 文案。
/// 经 portal 挂到 body 后抽屉的 translate 不再构成 fixed 后代的包含块(旧
/// transform 方案会),嵌套弹层的平铺挂载约定不变。
export default function DrawerShell({
  open, onClose, onClosed, testId, children,
}: {
  open: boolean; onClose: () => void; onClosed?: () => void; testId?: string; children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <Sheet
      open={open}
      onClose={onClose}
      onClosed={onClosed}
      testId={testId}
      closeLabel={t('common.close')}
    >
      {children}
    </Sheet>
  );
}
