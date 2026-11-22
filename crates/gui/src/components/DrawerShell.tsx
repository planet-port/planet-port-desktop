import { useEffect, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

/// 通用右侧抽屉壳:统一滑入/滑出动画。open 挂载后下一帧加 in 触发
/// transition;关闭先撤 in,250ms(jsdom 无 transitionend,定时兜底)后卸载。
/// 经 portal 挂到 body:.drawer.slide 的 transform 会成为 fixed 后代的
/// 包含块,嵌套抽屉(确认框/二维码/图标选择等)若留在抽屉 DOM 内会被
/// 裁剪错位——平铺为 body 兄弟节点后与遮罩同为全局层级,后者居上。
export default function DrawerShell({
  open, onClose, width, wide, testId, children,
}: {
  open: boolean; onClose: () => void; width?: number; wide?: boolean; testId?: string; children: ReactNode;
}) {
  const [mounted, setMounted] = useState(open);
  const [entered, setEntered] = useState(false);
  useEffect(() => {
    if (open) {
      setMounted(true);
      const id = window.setTimeout(() => setEntered(true), 16);
      return () => window.clearTimeout(id);
    }
    setEntered(false);
    const id = window.setTimeout(() => setMounted(false), 250);
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
