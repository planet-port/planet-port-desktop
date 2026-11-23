import type { ComponentProps } from 'react';
import { Switch as SwitchPrimitive } from '../ui/switch';
import { cn } from '@/lib/utils';

/// 项目开关:包 shadcn 原件 ui/switch.tsx(Radix Switch,保持官方源码不动),
/// 在封装层用项目 CSS 变量定格既有视觉基线(原 styles.css .switch 已删):
/// 轨道 34×20 胶囊 + 3px 内缩,滑块 14px,开启 = --primary / 关闭 = --track,
/// 滑块关 = --knob / 开 = --knob-on(亮暗两主题各自成立),带 0 1px 2px 投影。
/// 原件 focus ring 与 disabled:opacity-50 为有意采用的 shadcn 语义(基线无
/// 禁用态样式,属可访问性增强)。几何尺寸在此固定,故 Omit 原件的 size 旋钮。
/// 原件滑块样式只能经 [data-slot=switch-thumb] 后代选择器覆盖(原件不透出
/// Thumb props);基座尺寸类须带 data-[size=default] 变体,让 cn/twMerge 与
/// 原件同变体类互斥消解。滑块侧无法走 twMerge(不在同一 className 串),统一
/// 用 data-[state=checked|unchecked] 前缀把选择器特异性抬到 0,3,0,压过原件
/// 滑块类(最高 0,2,0)且不依赖 Tailwind 类序,规则升级安全。
type SwitchProps = Omit<ComponentProps<typeof SwitchPrimitive>, 'size'>;

export function Switch({ className, ...props }: SwitchProps) {
  return (
    <SwitchPrimitive
      className={cn(
        // 基座:34×20 胶囊 + 3px 内缩(滑块离轨道边 3px);border-0 消掉原件
        // 透明 1px 边(否则内容盒被吃 1px,滑块行程与基线差 1px);去 shadow-xs;
        // Tailwind v4 preflight 不再给 button 默认手型,基线是 pointer,补回
        'data-[size=default]:h-5 data-[size=default]:w-[34px] p-[3px] border-0 shadow-none cursor-pointer',
        // 基座配色:关闭 = --track(压掉原件 bg-input,亮暗各一份),开启沿用原件 bg-primary
        'data-[state=unchecked]:bg-[var(--track)] dark:data-[state=unchecked]:bg-[var(--track)]',
        // 滑块尺寸 14px(原件 size-4);checked 平移一个身位到右缘(原件算式按无内缩布局)
        'data-[state=unchecked]:[&_[data-slot=switch-thumb]]:size-3.5 data-[state=checked]:[&_[data-slot=switch-thumb]]:size-3.5',
        'data-[state=checked]:[&_[data-slot=switch-thumb]]:translate-x-full',
        // 滑块配色与投影(--knob/--knob-on 与投影值同基线)
        'data-[state=unchecked]:[&_[data-slot=switch-thumb]]:bg-[var(--knob)] data-[state=checked]:[&_[data-slot=switch-thumb]]:bg-[var(--knob-on)]',
        '[&_[data-slot=switch-thumb]]:shadow-[0_1px_2px_rgba(0,0,0,0.25)]',
        className,
      )}
      {...props}
    />
  );
}
