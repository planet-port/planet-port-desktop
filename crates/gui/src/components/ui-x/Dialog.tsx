import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type ComponentProps,
} from 'react';
import { Dialog as DialogPrimitive } from 'radix-ui';
import { DialogOverlay as OverlayPrimitive, DialogTitle as TitlePrimitive } from '../ui/dialog';
import { cn } from '@/lib/utils';

/// 项目弹窗:包 shadcn 原件系 ui/dialog.tsx + radix-ui Dialog 原语(官方源码不动),
/// 在封装层定格基线弹层(.overlay/.dialog/.dialog.confirm/.dialog-wide,styles.css 已删):
/// 遮罩 rgba(0,0,0,0.25)(原件 bg-black/50 经 twMerge 互斥消解)、z-40 保持基线 z 序
/// (.overlay 40:抽屉 31+layer 之上、toast z-50 之下);面板 --panel-2 底 + 1px --border
/// 边 + --radius 圆角 + 18px 20px 内边距,无阴影(深度由遮罩承担,基线 .dialog 无
/// box-shadow);宽 460px 两档之二,wide 档 = min(760px, calc(100vw-40px))
/// (.dialog-wide);max-width 100vw-40px(基线口径)。
/// max-h calc(100vh-80px) + overflow-y auto:居中弹窗对位原抽屉容器的自滚
/// (.drawer overflow-y:auto),内容超高不出屏——基线 .dialog 本身不滚,但被替换的
/// 七个抽屉壳弹层都靠壳滚动,容器层承接该职责。
/// 开合动画:原件 animate-in/out 系 tw-animate-css 工具类(本仓库未引入,不生效),
/// 封装层用 DrawerShell 同款两拍手法补回——useDialogPresence:open=true 挂载后 16ms
/// 置 entered 触发 250ms ease 过渡(遮罩淡入 + 面板 0.97→1 缩放淡入);open=false 先
/// 撤 entered 播退出,250ms 后才摘 Root open(Radix 卸载 portal)并回调 onClosed。
/// 父层条件渲染的清理动作挂 onClosed,退出动画播完组件才被卸载(与原 DrawerShell
/// open/onClose/onClosed 契约一致);jsdom 不跑 transition,卸载由定时承载。
/// Radix 交互语义:Esc 与遮罩点击 = 原生 dismiss → Root onOpenChange(false),业务层
/// 在此翻自身 open state;弹层打开期间锁 body 滚动、aria-hidden 外层内容(焦点圈定)。
/// 结构组装:原件 Content 内联的 Overlay 与关闭钮不透出 className,故 Portal/Content/
/// Close 直接用同源 radix-ui 原语组装(Overlay/Title 仍走 ui/ 原件经 className 覆盖),
/// ui/ 保持零改动。testId 经 Content 透传 data-testid(延续壳上挂 testid 的测试锚点);
/// closeLabel 传入即渲染右上角 28px 关闭钮(基线 .drawer-close 对译),原 overlay 类
/// 弹层(主题色/背景/扫码)无关闭钮则不传。

/// 开合动画时长与曲线:对齐现有 .drawer/.drawer-mask 的 250ms ease
const MOTION_MS = 250;

/// entered(入场/退场相位)由 Root 层持有、经 context 下发:Overlay/Content 按
/// 相位切换透明度/缩放类;AlertDialog(独立文件)复用同一 context 与类定义
export const EnteredContext = createContext(false);

/// 瞬时型弹层共用开合时序(DrawerShell 同款手法):open=true 挂载后 16ms 置
/// entered 触发 CSS 过渡;open=false 先撤 entered 播退出,MOTION_MS 后才摘
/// Radix Root 的 open(卸载 portal)并回调 onClosed。jsdom 无 transitionend,
/// 卸载由定时兜底;从未开过(open=false 挂载,如常驻的 ConfirmDialog)则不触发
/// onClosed,避免挂载即回调
export function useDialogPresence(open: boolean, onClosed?: () => void) {
  const [visible, setVisible] = useState(open);
  const [entered, setEntered] = useState(false);
  const everOpenRef = useRef(open);
  const onClosedRef = useRef(onClosed);
  useEffect(() => {
    onClosedRef.current = onClosed;
  }, [onClosed]);
  useEffect(() => {
    if (open) {
      everOpenRef.current = true;
      setVisible(true);
      const id = window.setTimeout(() => setEntered(true), 16);
      return () => window.clearTimeout(id);
    }
    setEntered(false);
    if (!everOpenRef.current) return;
    const id = window.setTimeout(() => {
      everOpenRef.current = false;
      setVisible(false);
      onClosedRef.current?.();
    }, MOTION_MS);
    return () => window.clearTimeout(id);
  }, [open]);
  return { visible, entered };
}

/// 面板基线(.dialog 对译,弹窗/警示弹窗两个封装共用);details 见文件头注释
export const dialogSurfaceClasses =
  'fixed top-1/2 left-1/2 z-40 flex max-h-[calc(100vh-80px)] w-[460px] max-w-[calc(100vw-40px)] -translate-x-1/2 -translate-y-1/2 flex-col overflow-y-auto rounded-md border border-border bg-[var(--panel-2)] dark:bg-[var(--panel-2)] px-5 py-[18px] shadow-none outline-none transition-[opacity,scale] duration-[250ms] ease-[ease]';

/// 遮罩基线(.overlay 对译);原件 bg-black/50 z-50 经 twMerge 同组互斥消解
export const dialogOverlayBaseClasses =
  'z-40 bg-black/25 transition-opacity duration-[250ms] ease-[ease]';

export function dialogOverlayClasses(entered: boolean) {
  return entered ? 'opacity-100' : 'pointer-events-none opacity-0';
}

export function dialogMotionClasses(entered: boolean) {
  return entered ? 'scale-100 opacity-100' : 'pointer-events-none scale-[0.97] opacity-0';
}

/// modal={false} 下 Radix 在 document 层监听指针/焦点,toast 层(z-50,在遮罩
/// z-40 之上)不在弹层内,点 toast 会被判 outside 而误关弹窗——对 sonner 的
/// toast 容器([data-sonner-toaster])放行不 dismiss
export function allowToastLayer(e: {
  detail: { originalEvent: { target: EventTarget | null } };
  preventDefault(): void;
}) {
  const t = e.detail.originalEvent.target;
  if (t instanceof Element && t.closest('[data-sonner-toaster]')) e.preventDefault();
}

type DialogProps = ComponentProps<typeof DialogPrimitive.Root> & {
  /// Radix 语义:Esc/遮罩点击请求关闭(false),业务层在此翻自身 open state
  onOpenChange?: (open: boolean) => void;
  /// 退出动画结束、内容真正卸载那一刻回调(对位原 DrawerShell onClosed)
  onClosed?: () => void;
};

export function Dialog({ open = false, onOpenChange, onClosed, children, ...props }: DialogProps) {
  const { visible, entered } = useDialogPresence(open, onClosed);
  return (
    <EnteredContext.Provider value={entered}>
      {/* modal={false} 对位基线:原抽屉/overlay 弹层无焦点圈定、无滚动锁,portal 外
          内容不进 aria-hidden(视口本已锁死,滚动锁无意义;遮罩层 z-40 覆盖全局,
          背景交互实际不可达)。modal 默认值会把外层内容标 aria-hidden,改变读屏
          语义且让背景 heading/role 查询失效(SettingsDrawer.test 的语言页断言) */}
      <DialogPrimitive.Root
        data-slot="dialog"
        modal={false}
        open={visible}
        onOpenChange={onOpenChange}
        {...props}
      >
        {children}
      </DialogPrimitive.Root>
    </EnteredContext.Provider>
  );
}

type DialogContentProps = ComponentProps<typeof DialogPrimitive.Content> & {
  /// Content 上透传 data-testid
  testId?: string;
  /// 宽档二档:.dialog-wide = min(760px, calc(100vw-40px))(表格类宽弹层用)
  wide?: boolean;
  /// 传入即渲染右上角关闭钮(aria-label),不传不渲染
  closeLabel?: string;
};

export function DialogContent({
  className,
  children,
  testId,
  wide = false,
  closeLabel,
  ...props
}: DialogContentProps) {
  const entered = useContext(EnteredContext);
  return (
    <DialogPrimitive.Portal data-slot="dialog-portal">
      <OverlayPrimitive
        data-slot="dialog-overlay"
        className={cn(dialogOverlayBaseClasses, dialogOverlayClasses(entered))}
      />
      <DialogPrimitive.Content
        data-slot="dialog-content"
        data-testid={testId}
        aria-describedby={undefined}
        onPointerDownOutside={allowToastLayer}
        onFocusOutside={allowToastLayer}
        className={cn(
          dialogSurfaceClasses,
          wide && 'w-[min(760px,calc(100vw-40px))]',
          dialogMotionClasses(entered),
          className,
        )}
        {...props}
      >
        {children}
        {closeLabel !== undefined && (
          <DialogPrimitive.Close
            data-slot="dialog-close"
            aria-label={closeLabel}
            className="absolute top-2.5 right-2.5 flex h-7 w-7 cursor-pointer items-center justify-center rounded-[6px] border border-border bg-[var(--panel)] p-0 text-lg leading-none text-[var(--muted)] transition-colors duration-[120ms] ease-[ease] hover:border-input hover:bg-[var(--panel-2)] hover:text-foreground focus-visible:border-input focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1"
          >
            ×
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

export function DialogTitle({ className, ...props }: ComponentProps<typeof TitlePrimitive>) {
  return (
    <TitlePrimitive
      data-slot="dialog-title"
      /// 基线 .dialog h3 / .drawer h3:14px 500;弹窗内与正文净距 14px;
      /// pr-8 避让右上角关闭钮(基线 .drawer header 的 padding-right:30px)
      className={cn('m-0 mb-3.5 pr-8 text-sm leading-[1.5] font-medium', className)}
      {...props}
    />
  );
}
