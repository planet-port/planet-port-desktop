import type { ComponentProps } from 'react';
import { TabsList as TabsListPrimitive, TabsTrigger as TabsTriggerPrimitive } from '../ui/tabs';
import { cn } from '@/lib/utils';

/// 项目分段控件:包 shadcn 原件 ui/tabs.tsx(Radix Tabs,官方源码不动),在
/// 封装层定格 styles.css .seg 全系(429-443 行,待全量替换后由收尾批删除):
/// 整组一个圆角凹陷轨道(--bg 底 + 1px --border 边 + 7px 圆角 + 2px 内衬 +
/// 2px 项间距),项 flex-1 平铺、5px 0 内边距、12px 正常字重字、5px 圆角;
/// 非选中 muted 字,hover 提字色;选中浮起为面板块(--panel 底 + 0 0 0 1px
/// --border 描边阴影,暗色 active #2c313c + #3a4050 描边——基线即字面值,
/// 无变量可挂,按原值对译),选中态由"块"而非颜色承担。
/// 覆盖手法与 Button/Switch 范式一致:基座类经 twMerge 与原件同名组互斥消解;
/// 原件 active shadow-sm 走 group/variant 组合选择器(特异性更高),对译串须
/// 带同组前缀压过;原件聚焦 ring 换基线口径的 1px 主色 outline。原件
/// disabled:opacity-50 为有意采用的 shadcn 语义(基线 .seg button:disabled
/// 只降字色,保留原件增强)。List 的 h-9 由原件 group-data 变体给出,同样带
/// 变体前缀复位为 auto。Tabs 根与 Content 为纯再出口(List/Trigger 即本
/// 分段形态的完整消费面)。

const tabsListClasses = cn(
  'w-full my-1.5 gap-0.5 rounded-[7px] border border-[color:var(--border)] bg-[var(--bg)] dark:bg-[var(--bg)] p-0.5',
  'group-data-[orientation=horizontal]/tabs:h-auto',
);

const tabsTriggerClasses = cn(
  // 几何:平铺 + 5px 0 内边距 + 12px 正常字重字 + 5px 圆角(压原件 px-2/h 按内容)
  'h-auto flex-1 px-0 py-[5px] gap-0 rounded-[5px] border-0 text-xs font-normal cursor-pointer',
  // 非选中 muted 字,hover 提字色(压原件 text-foreground/60 与 dark 分支)
  'text-[var(--muted)] dark:text-[var(--muted)] hover:text-[var(--text)] dark:hover:text-[var(--text)]',
  // 选中:面板块 + 1px 描边阴影(暗色 active 为基线字面值对译)
  'data-[state=active]:bg-[var(--panel)] data-[state=active]:text-[var(--text)] dark:data-[state=active]:bg-[#2c313c] dark:data-[state=active]:text-[var(--text)] dark:data-[state=active]:border-transparent',
  'data-[state=active]:shadow-[0_0_0_1px_var(--border)] dark:data-[state=active]:shadow-[0_0_0_1px_#3a4050]',
  // 原件 active shadow-sm / dark active border-input 走 group/variant 组合选择器,
  // 特异性更高,须带同组前缀才能压过
  'group-data-[variant=default]/tabs-list:data-[state=active]:shadow-[0_0_0_1px_var(--border)]',
  'dark:group-data-[variant=default]/tabs-list:data-[state=active]:shadow-[0_0_0_1px_#3a4050]',
  // 基线过渡仅字色/底色 .12s;聚焦换基线口径 1px 主色 outline(原件 ring 归零)
  'transition-[color,background-color] duration-[120ms] ease-[ease]',
  'focus-visible:ring-0 focus-visible:border-transparent',
);

export function TabsList({ className, ...props }: ComponentProps<typeof TabsListPrimitive>) {
  return <TabsListPrimitive className={cn(tabsListClasses, className)} {...props} />;
}

export function TabsTrigger({ className, ...props }: ComponentProps<typeof TabsTriggerPrimitive>) {
  return <TabsTriggerPrimitive className={cn(tabsTriggerClasses, className)} {...props} />;
}

export { Tabs, TabsContent } from '../ui/tabs';
