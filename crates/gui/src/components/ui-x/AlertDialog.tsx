import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  type ComponentProps,
} from 'react';
import { AlertDialog as AlertDialogPrimitive } from 'radix-ui';
import {
  AlertDialogOverlay as OverlayPrimitive,
  AlertDialogTitle as TitlePrimitive,
} from '../ui/alert-dialog';
import { cn } from '@/lib/utils';
import {
  EnteredContext,
  dialogMotionClasses,
  dialogOverlayBaseClasses,
  dialogOverlayClasses,
  dialogSurfaceClasses,
  useDialogPresence,
} from './Dialog';

/// 项目警示弹窗:包 shadcn 原件系 ui/alert-dialog.tsx + radix-ui AlertDialog 原语
/// (官方源码不动)。面板/遮罩/开合动画基线与 ui-x/Dialog 共用同一套定义(导入),
/// 差异在交互语义:role="alertdialog" + 焦点圈定(Radix AlertDialog 原生不响应
/// outside click),遮罩点击关闭由封装层经遮罩 onClick 补回基线行为——本应用
/// ConfirmDialog 点遮罩 = 取消;Esc 关闭走 Radix 原生 dismiss → Root
/// onOpenChange(false)。ui/alert-dialog.tsx 的 Content 内联 Portal+Overlay 不透出
/// className,故 Portal/Content 直接用同源 radix-ui 原语组装(Overlay/Title 仍走
/// ui/ 原件经 className 覆盖),ui/ 保持零改动。
/// 已知取舍:Radix AlertDialog 无 modal prop(恒 modal=true,强警示语义),
/// ConfirmDialog 叠在业务弹窗上时底层弹窗 portal 会被挂 aria-hidden——现有
/// 测试锚点不查叠层下的底层内容故无碍;若后续需要,再评估换 Dialog 原语
/// +role="alertdialog" 方案。
/// 原件的 AlertDialogAction/Cancel 语义 = Radix Close(点击即关),与本应用
/// ConfirmDialog「确认按钮等异步完成后才关」不符,不出口;按钮由业务层经
/// ui-x/Button 直配。

/// Root 层把「请求关闭」(遮罩点击)经 context 下发给 Content 内的遮罩
const CloseRequestContext = createContext<() => void>(() => {});

type AlertDialogProps = ComponentProps<typeof AlertDialogPrimitive.Root> & {
  /// Radix 语义:Esc(原生)/遮罩点击(封装层补回)请求关闭(false)
  onOpenChange?: (open: boolean) => void;
  /// 退出动画结束、内容真正卸载那一刻回调(对位原 DrawerShell onClosed)
  onClosed?: () => void;
};

export function AlertDialog({
  open = false,
  onOpenChange,
  onClosed,
  children,
  ...props
}: AlertDialogProps) {
  const { visible, entered } = useDialogPresence(open, onClosed);
  /// onOpenChange 存 ref:调用方常传内联箭头函数,新引用不重建回调
  const onOpenChangeRef = useRef(onOpenChange);
  useEffect(() => {
    onOpenChangeRef.current = onOpenChange;
  }, [onOpenChange]);
  const requestClose = useCallback(() => onOpenChangeRef.current?.(false), []);
  return (
    <EnteredContext.Provider value={entered}>
      <CloseRequestContext.Provider value={requestClose}>
        <AlertDialogPrimitive.Root
          data-slot="alert-dialog"
          open={visible}
          onOpenChange={onOpenChange}
          {...props}
        >
          {children}
        </AlertDialogPrimitive.Root>
      </CloseRequestContext.Provider>
    </EnteredContext.Provider>
  );
}

type AlertDialogContentProps = ComponentProps<typeof AlertDialogPrimitive.Content> & {
  /// Content 上透传 data-testid
  testId?: string;
};

export function AlertDialogContent({
  className,
  children,
  testId,
  ...props
}: AlertDialogContentProps) {
  const entered = useContext(EnteredContext);
  const requestClose = useContext(CloseRequestContext);
  return (
    <AlertDialogPrimitive.Portal data-slot="alert-dialog-portal">
      <OverlayPrimitive
        data-slot="alert-dialog-overlay"
        onClick={requestClose}
        className={cn(dialogOverlayBaseClasses, dialogOverlayClasses(entered))}
      />
      <AlertDialogPrimitive.Content
        data-slot="alert-dialog-content"
        data-testid={testId}
        aria-describedby={undefined}
        className={cn(dialogSurfaceClasses, dialogMotionClasses(entered), className)}
        {...props}
      >
        {children}
      </AlertDialogPrimitive.Content>
    </AlertDialogPrimitive.Portal>
  );
}

export function AlertDialogTitle({ className, ...props }: ComponentProps<typeof TitlePrimitive>) {
  return (
    <TitlePrimitive
      data-slot="alert-dialog-title"
      /// 基线 .dialog h3:14px 500,与正文净距 14px;行高对齐 body 1.5 基线
      className={cn('m-0 mb-3.5 text-sm leading-[1.5] font-medium', className)}
      {...props}
    />
  );
}
