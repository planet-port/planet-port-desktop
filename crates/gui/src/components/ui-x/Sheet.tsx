import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { Dialog as SheetPrimitive } from 'radix-ui';
import { cn } from '@/lib/utils';

/// 项目右侧抽屉:包 Radix Dialog 原语(shadcn 原件 ui/sheet.tsx 的同一底层,
/// 原件零改动)。不经 ui/sheet.tsx 组合:SheetContent 不向其内部 Overlay/
/// Portal 透传 props,多层堆叠所需的逐层 z-index(遮罩 30+layer / 抽屉
/// 31+layer)与 --stack 内推 offset 无法经 SheetContent 注入(单独取用
/// SheetPortal/SheetOverlay 组合虽可行,Content 仍须原语直构,链路割裂);
/// 且 Radix Presence 只认 keyframes 动画、接管不了 transition 卸载时序,
/// 两拍动画的卸载控制必须自持——故整层按原件同源原语在封装层重排。
/// 对译基线 .drawer-mask/.drawer 全系(styles.css 已删):
/// 多层堆叠:--stack 反向序(最新层 0 贴右,底层逐层 right +44px 内推露左缘,
/// 被上层遮罩压暗);--layer 挂载序管 z-index(遮罩 30+layer、抽屉 31+layer,
/// 后挂居上)。兄弟抽屉增删由 MutationObserver 监听 body childList 重排,
/// 关闭动画期尚未卸载的层仍占位。Radix Portal 在 layout effect 后的下一轮
/// commit 才挂出内容,故抽屉元素经回调 ref 落 state、重排 effect 以其为依赖
/// (首帧 effect 拿不到节点,依赖 mounted 会扑空)。
/// 动画:进出同 250ms——挂载先以 data-state=closed(translate 全量右移)入树,
/// 16ms 双帧后翻 open 触发 translate/right 过渡滑入;关闭立即撤 open(data-state
/// =closed 平滑滑出),250ms(jsdom 无 transition,定时兜底)后卸载并回调
/// onClosed。onClosed 在真正卸载那一刻回调——瞬时型弹层由父层条件渲染控制
/// 卸载,父层只能凭它清理状态,否则 onClose 一触发组件即被卸载,滑出被砍。
/// Radix 侧保留 portal、focus trap、Esc、遮罩点击关闭(经 onOpenChange 收敛
/// 到 onClose);开启焦点经 onOpenAutoFocus preventDefault 归还调用方(基线
/// 由表单 autoFocus 定焦点,否则 Radix 默认抢焦到壳内关闭钮)。关闭动画期
/// (data-state=closed)遮罩/抽屉 pointer-events:none,防窗口内误触;16ms
/// 进入帧内 Radix 受控 open 未翻转,关闭钮/Esc 天然不可达,与浏览器可点窗口
/// 一致。forceMount 挂在 Portal 上透传 Overlay/Content:退出动画期间元素保持
/// 挂载,卸载时机由本层定时器独占(Radix Presence 只认 keyframes 动画,对
/// transition 无效,不能替行卸载时序)。关闭钮文案经 closeLabel 注入(封装层
/// 不做 i18n,shadcn 原件同口径),几何/配色定格基线 .drawer-close(28px 面底
/// 描边方块)。

const sheetOverlayClasses = cn(
  // 遮罩:全屏 rgba(0,0,0,0.25),z-index 随 --layer(30+layer);透明度过渡
  // 承载淡入/淡出(基线 .drawer-mask.fade)
  'fixed inset-0 z-[calc(30_+_var(--layer,_0))] bg-[rgba(0,0,0,0.25)]',
  'opacity-0 transition-opacity duration-[250ms] ease-[ease] data-[state=open]:opacity-100',
  'data-[state=closed]:pointer-events-none',
);

const sheetContentClasses = cn(
  // 几何:右侧全高,fit-content 最小 360px;right 随 --stack 内推(44px/层);
  // z-index 31+layer;内容纵排 gap 12px、padding 16px 18px、纵向滚动
  'fixed inset-y-0 right-[calc(var(--stack,0)*44px)] z-[calc(31_+_var(--layer,_0))]',
  'flex w-fit min-w-[360px] flex-col gap-3 overflow-y-auto py-4 px-[18px]',
  // 面底/左缘 hairline/单侧投影(基线 .drawer)
  'bg-[var(--panel-2)] border-l border-l-[color:var(--border)] shadow-[-8px_0_24px_rgba(0,0,0,0.12)] outline-none',
  // 滑入滑出:translate 承载位移动画,translate/right 同步 250ms(多层内推平滑)
  'translate-x-full transition-[translate,right] duration-[250ms] ease-[ease] data-[state=open]:translate-x-0',
  'data-[state=closed]:pointer-events-none',
);

const sheetCloseClasses = cn(
  // 右上角统一关闭钮(基线 .drawer-close):常显面底+描边,hover 提亮
  'absolute top-2.5 right-2.5 inline-flex size-7 items-center justify-center p-0 cursor-pointer',
  'rounded-[6px] border border-[color:var(--border)] bg-[var(--panel)] text-[18px] leading-none text-[var(--muted)]',
  'transition-colors duration-[120ms] ease-[ease]',
  'hover:bg-[var(--panel-2)] hover:border-[color:var(--border-h)] hover:text-[var(--text)]',
);

export function Sheet({
  open,
  onClose,
  onClosed,
  testId,
  closeLabel = 'Close',
  children,
}: {
  open: boolean;
  onClose: () => void;
  onClosed?: () => void;
  testId?: string;
  closeLabel?: string;
  children: ReactNode;
}) {
  const [mounted, setMounted] = useState(open);
  const [entered, setEntered] = useState(false);
  const [stack, setStack] = useState(0);
  const [layer, setLayer] = useState(0);
  /// 抽屉元素经回调 ref 落 state(Radix Portal 延迟一轮 commit,见头注释)
  const [el, setEl] = useState<HTMLElement | null>(null);
  /// onClosed 存 ref:父层常传内联箭头函数,新引用不应重置卸载定时
  const onClosedRef = useRef(onClosed);
  useEffect(() => {
    onClosedRef.current = onClosed;
  }, [onClosed]);
  useEffect(() => {
    if (open) {
      setMounted(true);
      const id = window.setTimeout(() => setEntered(true), 16);
      return () => window.clearTimeout(id);
    }
    setEntered(false);
    const id = window.setTimeout(() => {
      setMounted(false);
      onClosedRef.current?.();
    }, 250);
    return () => window.clearTimeout(id);
  }, [open]);
  /// 层号在抽屉真正挂出后才可取(依赖 el,Portal 二轮 commit 后触发)
  useEffect(() => {
    if (!el) return;
    const restack = () => {
      const drawers = Array.from(document.querySelectorAll('aside[data-slot="sheet-content"]'));
      const i = Math.max(0, drawers.indexOf(el));
      setLayer(i);
      setStack(drawers.length - 1 - i);
    };
    restack();
    const mo = new MutationObserver(restack);
    mo.observe(document.body, { childList: true });
    return () => mo.disconnect();
  }, [el]);
  if (!mounted) return null;
  return (
    <SheetPrimitive.Root
      open={entered}
      onOpenChange={next => {
        if (!next) onClose();
      }}
    >
      <SheetPrimitive.Portal forceMount>
        <SheetPrimitive.Overlay
          data-slot="sheet-overlay"
          className={sheetOverlayClasses}
          style={{ '--layer': layer } as CSSProperties}
        />
        <SheetPrimitive.Content asChild onOpenAutoFocus={e => e.preventDefault()}>
          <aside
            ref={setEl}
            data-slot="sheet-content"
            data-testid={testId}
            className={sheetContentClasses}
            style={{ '--stack': stack, '--layer': layer } as CSSProperties}
          >
            <SheetPrimitive.Close className={sheetCloseClasses} aria-label={closeLabel}>
              ×
            </SheetPrimitive.Close>
            {children}
          </aside>
        </SheetPrimitive.Content>
      </SheetPrimitive.Portal>
    </SheetPrimitive.Root>
  );
}
