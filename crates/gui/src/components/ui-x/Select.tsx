import type { ComponentProps } from 'react';
import {
  Select as SelectPrimitive,
  SelectContent as SelectContentPrimitive,
  SelectGroup as SelectGroupPrimitive,
  SelectItem as SelectItemPrimitive,
  SelectLabel as SelectLabelPrimitive,
  SelectSeparator as SelectSeparatorPrimitive,
  SelectTrigger as SelectTriggerPrimitive,
  SelectValue,
} from '../ui/select';
import { cn } from '@/lib/utils';

/// 项目下拉选择:包 shadcn 原件 ui/select.tsx(Radix Select,官方源码不动),
/// 在封装层对译抽屉内的 iOS 值行 + 勾选列表形态(styles.css .kv 值行与
/// .picker-pop,后者的行内展开列表由 Radix 浮层替代,浮层卡片定格
/// picker-pop 卡形:10px 圆角 + --border-soft 边 + 6px 内衬 + max-h 220px,
/// 宽随触发行;底色取 --panel 比抽屉底亮一档——基线 --panel-3 变量已不存在,
/// 实际渲染为透明,属基线遗留,此处按卡形正常成立取 --panel):
/// Trigger = .kv 值行:全宽撑满卡内宽(负 margin + padding 补偿)、13px 字、
///   透明底、hover 抬 --panel(可点信号);右侧自带 ChevronDown 译为基线
///   .chev 形态(14px faint、-rotate-90 指右)——原件图标类写死在内部,
///   经 [&>svg:last-child] 后代选择器覆盖(0,2,0 压原件 0,1,0)。
/// Item = 勾选行:13px 字 + 6px 8px 内边距(pr-8 让位右侧指示符),当前项
///   ✓ primary(基线 .check 语义,颜色落在指示符上);键盘高亮沿用原件
///   focus:bg-accent(shadcn 语义,基线无对应态)。
/// Root/Group/Label/Separator 为纯再出口。Radix Select 的空 value 限制由
/// 调用方用哨兵值承载(如 default 行),键盘导航/类型前置为原件原生语义。

const selectTriggerClasses = cn(
  // 值行几何:全宽、13px 字、6px 8px 内边距 + 负 margin 撑满卡内宽、6px 圆角
  'w-full h-auto justify-start gap-1.5 -mx-2 px-2 py-1.5 rounded-[6px] text-left text-[13px]',
  // 透明底描边归零(压原件 border-input/shadow-xs/bg/dark 分支),hover 抬 --panel
  'border-0 bg-transparent dark:bg-transparent shadow-none cursor-pointer',
  'enabled:hover:bg-[var(--panel)] dark:enabled:hover:bg-[var(--panel)]',
  // 聚焦换基线口径 1px 主色 outline(原件 ring 归零)
  'focus-visible:ring-0 focus-visible:shadow-none focus-visible:outline-1 focus-visible:outline-solid focus-visible:outline-ring focus-visible:outline-offset-1',
  // 原件右侧 ChevronDown 译为基线 .chev:14px faint、转正为右向
  '[&>svg:last-child]:size-3.5 [&>svg:last-child]:opacity-100 [&>svg:last-child]:text-[var(--faint)] [&>svg:last-child]:-rotate-90',
);

const selectContentClasses = cn(
  // picker-pop 卡形:10px 圆角、--border-soft 边、6px 内衬、max-h 220px、宽随触发行
  'min-w-0 w-[var(--radix-select-trigger-width)] max-h-[220px] rounded-[10px] border-[color:var(--border-soft)] p-1.5 shadow-none',
);

const selectItemClasses = cn(
  // 勾选行:13px 字 + 6px 8px 内边距(pr-8 让位右侧指示符,px-2 后置覆盖原件 pl/pr)
  'px-2 py-1.5 pr-8 rounded-[6px] text-[13px] text-[var(--text)] cursor-pointer',
  // 当前项 ✓ primary(基线 .check:primary + 600 字重,图标取 currentColor)
  '[&_[data-slot=select-item-indicator]]:text-[var(--primary)]',
);

export function SelectTrigger({ className, ...props }: ComponentProps<typeof SelectTriggerPrimitive>) {
  return <SelectTriggerPrimitive className={cn(selectTriggerClasses, className)} {...props} />;
}

export function SelectContent({ className, ...props }: ComponentProps<typeof SelectContentPrimitive>) {
  return <SelectContentPrimitive className={cn(selectContentClasses, className)} {...props} />;
}

export function SelectItem({ className, ...props }: ComponentProps<typeof SelectItemPrimitive>) {
  return <SelectItemPrimitive className={cn(selectItemClasses, className)} {...props} />;
}

export { SelectPrimitive as Select };
export { SelectGroupPrimitive as SelectGroup, SelectLabelPrimitive as SelectLabel, SelectSeparatorPrimitive as SelectSeparator, SelectValue };
