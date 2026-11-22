import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';

/// 通用右侧抽屉壳:统一滑入/滑出动画与右上角关闭按钮。open 挂载后下一帧
/// 加 in 触发 transition;关闭先撤 in,250ms(jsdom 无 transitionend,定时兜底)
/// 后卸载。onClosed 在动画结束真正卸载那一刻回调——瞬时型弹层由父层条件
/// 渲染控制卸载,父层只能凭它清理状态,否则 onClose 一触发组件即被卸载,滑出被砍。
/// 经 portal 挂到 body:.drawer.slide 的 transform 会成为 fixed 后代的
/// 包含块,嵌套抽屉(确认框/二维码/图标选择等)若留在抽屉 DOM 内会被
/// 裁剪错位——平铺为 body 兄弟节点后与遮罩同为全局层级,后者居上。
/// 多层抽屉层叠:挂载后按 body 下 aside.drawer 的 DOM 序取层号 --stack,
/// 每深一层右偏 44px 露出下层边缘(被本层遮罩压暗);兄弟抽屉增删由
/// MutationObserver 监听 body childList 重排,关闭动画期尚未卸载的层仍占位。
export default function DrawerShell({
  open, onClose, onClosed, testId, children,
}: {
  open: boolean; onClose: () => void; onClosed?: () => void; testId?: string; children: ReactNode;
}) {
  const { t } = useTranslation();
  const [mounted, setMounted] = useState(open);
  const [entered, setEntered] = useState(false);
  const [stack, setStack] = useState(0);
  const ref = useRef<HTMLElement | null>(null);
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
  /// 层号在真正渲染出 aside 后才可取(mounted 翻 true 时本 effect 重跑)
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const restack = () => {
      setStack(Math.max(0, Array.from(document.querySelectorAll('aside.drawer')).indexOf(el)));
    };
    restack();
    const mo = new MutationObserver(restack);
    mo.observe(document.body, { childList: true });
    return () => mo.disconnect();
  }, [mounted]);
  if (!mounted) return null;
  return createPortal(
    <>
      <div
        className={`drawer-mask fade ${entered ? 'in' : ''}`}
        style={{ '--stack': stack } as CSSProperties}
        onClick={onClose}
      />
      <aside
        ref={ref}
        className={`drawer slide ${entered ? 'in' : ''}`}
        style={{ '--stack': stack } as CSSProperties}
        data-testid={testId}
      >
        <button className="drawer-close" aria-label={t('common.close')} onClick={onClose}>×</button>
        {children}
      </aside>
    </>,
    document.body,
  );
}
