import { useEffect, useState, type CSSProperties } from 'react';
import { Toaster as Sonner, toast } from 'sonner';
import { cn } from '@/lib/utils';

/// 项目全局 toast:直取 sonner 包原 Toaster(不组合 shadcn 原件 ui/sonner.tsx,
/// 原件零改动)。绕开原因与 ui-x/Sheet 同款:原件把 theme 绑死在 next-themes
/// 的 useTheme() 上,而项目主题态唯一来源是 documentElement[data-theme]
/// (theme.ts 自持,无 ThemeProvider),引 next-themes 等于再挂一套主题源双头
/// 并存;原件自带的 icons 调色板也与基线纯文字 toast 不符——故按原件同源
/// (sonner 包)在封装层重排。
/// sonner 在模块 import 时即向 head 注入自带 styles.css;Tailwind v4 工具类
/// 全在 @layer 内,sonner 注入规则无 layer——unlayered 对 layered 无条件优先
/// (与注入顺序、特异性均无关),className 覆盖不可靠;toast 条目因此走
/// toast.custom 自绘内容——jsx 型 toast 在 sonner 侧自动 data-styled=false,
/// 原生壳样式(16px 内距/13px 字号/投影/图标)整体失效,条目文案层完全归
/// 本层掌控。
/// 视觉对译基线 .toast 系(styles.css 已删):面底 --panel /
/// 1px --border 描边 / 6px 圆角 / --text 文字 / 8px 12px 内距 / 12px 字号 /
/// 宽度上限 380px(sonner 默认把 ol 定宽 --width:356px 且 li 右锚 absolute,
/// 内容 max-w 永不可达,故行内定 --width:380px 由容器直接承担)/ 文本可选;
/// error 型 = danger 45% 混色边框 + danger 文字。
/// kind('ok'/'error')以类名标在内容根元素上:测试锚点是「错误 toast 的文本
/// 元素带 error 类」,基线 .toast 系已删,该类仅作语义标记。
/// 容器对译基线 .toasts(styles.css 已删):position bottom-right + offset 16px(右/下)、z-index 50
/// (行内样式压掉 sonner 内置 z 999999999)、expand 常开 + gap 8px 还原基线
/// 「全部纵排常显」;条目行内 left/width:auto 压掉 sonner 窄窗(≤600px)的
/// 全宽规则——桌面 minWidth 820 该分支实际不可达,纯预留(跨平台复用 webview
/// 时才可能触及),未做窄窗实测。
/// 结构差异如实声明:sonner 每条 toast 是绝对定位 li,进出带 400ms transform/
/// opacity 过渡 + 200ms 卸载延迟(基线瞬时增删),条目可手势挥出(swipe),
/// 键盘可聚焦且 focus-visible 有 ring(shadcn/sonner 可访问性语义,基线无);
/// 文字换行由 sonner li 的 overflow-wrap:anywhere 承担(基线默认断行,长串
/// 不再溢出)。自动消失 4000ms(显式写死不依赖上游默认值);计时在悬停于
/// toast 区/交互中/窗口隐藏(托盘化)时暂停,基线为无条件 4s——停留语义算
/// 改进,保留;
/// visibleToasts 取 99 为基线「无上限」的等价大值(sonner 默认 3 会静默隐匿
/// 第 4 条起,且在常显展开态留空洞)。
/// 主题:读 documentElement[data-theme](仅 light/dark,与 theme.ts 落地值
/// 对齐),属性级 MutationObserver 跟随切换;theme 只落到 sonner 的
/// data-sonner-theme,自绘样式不依赖它,显式传值同时避开 sonner 'system' 态
/// 的 matchMedia 路径。

type ToastKind = 'ok' | 'error';
type Theme = 'light' | 'dark';

function domTheme(): Theme {
  return document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light';
}

const toastContentClasses = cn(
  // 基线 .toast:面底 + 1px 描边 + 6px 圆角,内距 8px 12px、字号 12px、
  // 宽度上限由容器 --width 承担(见头注释)、文本可选;文字色 --text
  'max-w-[380px] rounded-[6px] border border-[color:var(--border)] bg-[var(--panel)]',
  'px-3 py-2 text-xs text-[color:var(--text)] select-text',
);

const toastKindClasses: Record<ToastKind, string> = {
  // ok = 基线默认态(基线无 .toast.ok 变体,即 .toast 本身)
  ok: '',
  // error = 基线 .toast.error:danger 45% srgb 混色边框 + danger 文字
  error:
    'border-[color:color-mix(in_srgb,var(--danger)_45%,transparent)] text-[color:var(--danger)]',
};

function ToastContent({ kind, text }: { kind: ToastKind; text: string }) {
  return <div className={cn(toastContentClasses, toastKindClasses[kind], kind)}>{text}</div>;
}

/// 入队一条 toast(custom 型):内容由本层自绘,供 ToastProvider 薄层调用
export function pushToast(kind: ToastKind, text: string) {
  toast.custom(
    () => <ToastContent kind={kind} text={text} />,
    // 行内 left/width 压掉 sonner 窄窗全宽规则(见头注释),保持右锚自适应宽
    { style: { left: 'auto', width: 'auto' } },
  );
}

export function Toaster() {
  const [theme, setTheme] = useState<Theme>(domTheme);
  useEffect(() => {
    // data-theme 由 theme.ts 写在 documentElement;属性变化监听跟随切主题
    const mo = new MutationObserver(() => setTheme(domTheme()));
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => mo.disconnect();
  }, []);
  return (
    <Sonner
      theme={theme}
      position="bottom-right"
      offset={16}
      mobileOffset={16}
      gap={8}
      expand
      duration={4000}
      visibleToasts={99}
      // z-index 50 行内定格(压掉 sonner 注入样式里的 z 999999999,对齐基线 .toasts);
      // --width 380 还原基线宽度上限(sonner 默认 ol 定宽 356px,见头注释)
      style={{ zIndex: 50, '--width': '380px' } as CSSProperties}
    />
  );
}
