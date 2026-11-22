import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

/// 通用右侧抽屉壳:统一滑入/滑出动画。open 挂载后下一帧加 in 触发
/// transition;关闭先撤 in,250ms(jsdom 无 transitionend,定时兜底)后卸载。
/// onClosed 在动画结束真正卸载那一刻回调——瞬时型弹层由父层条件渲染控制
/// 卸载,父层只能凭它清理状态,否则 onClose 一触发组件即被卸载,滑出被砍。
/// 经 portal 挂到 body:.drawer.slide 的 transform 会成为 fixed 后代的
/// 包含块,嵌套抽屉(确认框/二维码/图标选择等)若留在抽屉 DOM 内会被
/// 裁剪错位——平铺为 body 兄弟节点后与遮罩同为全局层级,后者居上。
export default function DrawerShell({
  open, onClose, onClosed, width, wide, testId, children,
}: {
  open: boolean; onClose: () => void; onClosed?: () => void; width?: number; wide?: boolean; testId?: string; children: ReactNode;
}) {
  const [mounted, setMounted] = useState(open);
  const [entered, setEntered] = useState(false);
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
  if (!mounted) return null;
  return createPortal(
    <>
      <div className={`drawer-mask fade ${entered ? 'in' : ''}`} onClick={onClose} />
      <aside className={`drawer slide ${entered ? 'in' : ''} ${wide ? 'wide' : ''}`} style={width ? { width } : undefined} data-testid={testId}>
        {children}
      </aside>
    </>,
    document.body,
  );
}
