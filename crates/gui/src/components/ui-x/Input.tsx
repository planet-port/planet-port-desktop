import type { ComponentProps } from 'react';
import { Input as InputPrimitive } from '../ui/input';
import { cn } from '@/lib/utils';

/// 项目输入框:包 shadcn 原件 ui/input.tsx(官方源码不动),封装层定格
/// styles.css .input 全系(99-113 行,待 Task 4 全量替换完成后删除):
/// 1px --border 边、6px 圆角、6px 10px 内边距、--panel-2 底、13px 字(基线
/// font: inherit,正文 13px/1.5);hover 边 --border-h,译作 enabled:hover:
/// (基线 :hover:not(:disabled);封装层把原件 disabled:pointer-events-none
/// 复位成 auto,裸 hover: 会让禁用态也亮边,textarea 分支共用同一类串);
/// focus 主色边 + --panel
/// 底 + 1px 主色 ring(双层反馈)。基线用 :focus,文本输入框聚焦恒匹配
/// :focus-visible,故沿用原件 focus-visible: 复刻:原件 focus-visible:border-ring
/// 恰为主色边(= --color-ring 映射)保留,ring 宽度归零、投影覆盖为基线的
/// 0 0 0 1px。placeholder --muted:原件 placeholder:text-muted-foreground 映射
/// 同一变量,保留。disabled --off 字 + --panel-2 底 + cursor default:原件的
/// opacity-50 / pointer-events-none 复位(基线不禁交互只改观感)。原件
/// dark:bg-input/30 暗色补底按 dark: 显式补份规则覆盖为同一 var(--panel-2)。
/// 与基线不符的原件几何一律交还调用方:h-auto/w-auto(styles.css 605/619/621
/// 的 .id-input/.identity-name/.url-input 定宽类与 .dns-slots 自然宽度都依赖
/// 默认宽,w-full 会以 utilities 层后置压过这些类)、text-[13px] 双断点覆盖
/// text-base md:text-sm、6px 10px 内边距、6px 圆角、transition 仅边色/投影。
/// mono 字族不内置,由调用方传 className(基线经 .mono/调用方类名叠加)。
/// 原件 selection/file 语义与基线无冲突,保留。textarea 经 as="textarea" 分派:
/// 原件写死渲染 <input> 且未导出类名串,textarea 分支复用同一覆盖集另补
/// placeholder/focus 边两件原件语义 + 基线 textarea.input 的 resize:vertical
/// (当前仓库无 textarea 调用方,接口为 styles.css 兼容预留)。
const inputClasses =
  'h-auto w-auto min-w-0 rounded-[6px] border border-border bg-[var(--panel-2)] dark:bg-[var(--panel-2)] px-2.5 py-1.5 text-[13px] md:text-[13px] shadow-none transition-[border-color,box-shadow] duration-[120ms] ease-[ease] outline-none enabled:hover:border-input focus-visible:ring-0 focus-visible:bg-[var(--panel)] focus-visible:shadow-[0_0_0_1px_var(--primary)] disabled:pointer-events-auto disabled:cursor-default disabled:opacity-100 disabled:text-[var(--off)] disabled:bg-[var(--panel-2)] file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground';

type InputProps<T extends 'input' | 'textarea' = 'input'> = ComponentProps<T> & {
  as?: T;
};

export function Input<T extends 'input' | 'textarea' = 'input'>({
  as = 'input' as T,
  className,
  ...props
}: InputProps<T>) {
  if (as === 'textarea') {
    return (
      <textarea
        data-slot="input"
        className={cn(
          inputClasses,
          'resize-y placeholder:text-muted-foreground focus-visible:border-ring',
          className,
        )}
        {...(props as ComponentProps<'textarea'>)}
      />
    );
  }
  return (
    <InputPrimitive
      className={cn(inputClasses, className)}
      {...(props as ComponentProps<'input'>)}
    />
  );
}
