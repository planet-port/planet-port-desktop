import type { ComponentProps } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { Button as ButtonPrimitive } from '../ui/button';
import { cn } from '@/lib/utils';

/// 项目按钮:包 shadcn 原件 ui/button.tsx(cva,官方源码不动),在封装层用项目
/// CSS 变量 / Tailwind token 定格既有视觉基线(基线 .btn 全系与 .btn-block,
/// styles.css 已删):
/// 几何:高 28px、12px 字(line-height 1、正常字重)、1px 边框 6px 圆角、
///   内边距 0 10px、gap 6px;icon 变体 28px 方、padding 0、14px 字;sm 仅
///   内边距收 0 8px(高度不变,基线 .btn.sm 不动 height)。
/// 配色:default 透明底描边(--border 边 / --text 字),hover 边 --border-h +
///   底 --panel-2(按下同 --panel-2);primary 主色实底白字,hover 换
///   --primary-h(基线字色为字面 #fff,--color-primary-foreground 映射的是
///   var(--bg),暗色主题会变深色字,与基线不符,故用 text-white);danger
///   状态色字 + 45% 透明边(srgb color-mix arbitrary 复刻,Tailwind 透明度
///   修饰符按 oklab 混色与基线有色相偏差,不可用),hover 满边 + 12% 底;
///   active 主色字满边(hover 仍回落 --border-h 边,与基线 :hover 高特异性
///   胜 .btn.active 一致);block(w-full)对应 .btn-block 的宽度,其
///   margin-top:10px 属调用方布局间距,不进封装层。
/// 覆盖手法:基座类经 twMerge 与原件同名组互斥消解(原件 default variant 的
/// bg-primary/text-primary-foreground/hover:bg-primary/90、h-9/px-4、rounded-md、
/// focus ring、disabled:opacity-50、[&_svg]:size-4 均被对应覆盖;基线聚焦只走
/// 1px outline(--primary、offset 1px),ring 归零;aria-invalid 基线无、保留
/// 原件作为可访问性增强)。原件 focus-visible:border-ring 与基线"聚焦不变边色"
/// 冲突,各变体带同组 focus-visible:border-* 复位为本变体边色。
/// 禁用态按基线级联复刻:基线 .btn:disabled 在源序上先于
/// .btn.primary/.danger/.active,同特异性后者胜——即非 default 变体
/// 禁用后仍保持原配色,仅 default 禁用转 --off 字 / --border 边 / 透明底。
/// 因此 disabled 配色复位只写进 default 变体,其余变体不写,pointer-events-none
/// 保留原件语义(等效基线 :hover:not(:disabled) 守卫)。
/// 轴设计:现有代码无 icon 与颜色变体组合(grep `btn icon` 佐证),icon 进
/// variant 单选轴;size 声明在 variant 之前,使 icon 的 px-0 能压过 sm 的 px-2
/// (twMerge 后者胜)。原件 data-variant/data-size 因 variant 不透传会写死
/// default/default,此处经 props 同名键覆盖为项目语义值,供测试与样式钩子。
const buttonVariants = cva(
  // 基座:几何 + default 描边配色 + hover/按下(default 的 --border-h 边 +
  // --panel-2 底,各变体按需覆盖)+ 聚焦 outline + 禁用交互。hover 行为必须
  // 放基座:变体串不叠加 default 变体类,放 default 里会让其余变体失去 hover
  // 覆盖、漏出原件 hover:bg-primary/90
  'inline-flex shrink-0 items-center justify-center gap-1.5 h-7 px-2.5 py-0 has-[>svg]:px-2.5 rounded-[6px] border border-border bg-transparent text-foreground text-xs leading-none font-normal whitespace-nowrap cursor-pointer transition-[color,border-color,background-color] duration-[120ms] ease-[ease] outline-none hover:border-input hover:bg-accent active:bg-accent focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1 focus-visible:ring-0 focus-visible:border-border disabled:pointer-events-none disabled:cursor-default disabled:opacity-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*=\'size-\'])]:size-auto',
  {
    variants: {
      // size 先声明:twMerge 后者胜,icon 变体的 px-0 需压过 sm 的 px-2
      size: {
        default: '',
        sm: 'px-2 has-[>svg]:px-2',
      },
      variant: {
        // 禁用配色复位按基线级联只属于 default(.btn:disabled 在源序上先于
        // .btn.primary/.danger/.active,同特异性后者胜,即其余变体禁用保持原配色)
        default:
          'disabled:text-[var(--off)] disabled:border-border disabled:bg-transparent',
        primary:
          // 主色实底白字,hover --primary-h(无映射 token,走 arbitrary var)
          'bg-primary text-white border-primary hover:bg-[var(--primary-h)] hover:border-[var(--primary-h)] focus-visible:border-primary',
        danger:
          // srgb color-mix arbitrary 复刻基线 45%/12% 混色(不用 oklab 透明度修饰符)
          'text-destructive border-[color:color-mix(in_srgb,var(--danger)_45%,transparent)] hover:border-destructive hover:bg-[color:color-mix(in_srgb,var(--danger)_12%,transparent)] focus-visible:border-[color:color-mix(in_srgb,var(--danger)_45%,transparent)]',
        active:
          // 主色字满边;hover 边/底沿用基座回落(基线 :hover 特异性胜 .btn.active)
          'text-primary border-primary focus-visible:border-primary',
        // icon 禁用落 --off:基线 .btn:disabled(139)对 icon 同样生效(icon 规则
        // 只动几何不动配色);边/底经基座 default 值与禁用复位天然一致,仅字色需补
        icon: 'w-7 px-0 has-[>svg]:px-0 text-sm disabled:text-[var(--off)]',
      },
    },
    defaultVariants: {
      size: 'default',
      variant: 'default',
    },
  },
);

// Omit 挖掉原件同名 prop 再交封装层 VariantProps:两个 variant 联合仅
// "default" 重叠,直接交类型会收窄成 "default",primary/icon 等全 ts 报错
type ButtonProps = Omit<ComponentProps<typeof ButtonPrimitive>, 'variant' | 'size'> &
  VariantProps<typeof buttonVariants> & {
    block?: boolean;
  };

export function Button({
  className,
  variant = 'default',
  size = 'default',
  block = false,
  asChild = false,
  ...props
}: ButtonProps) {
  return (
    <ButtonPrimitive
      asChild={asChild}
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size }), block && 'w-full', className)}
      {...props}
    />
  );
}

export { buttonVariants };
export type { ButtonProps };
