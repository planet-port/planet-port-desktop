import { ArrowUp } from 'lucide-react';

/// 可升级小箭头(纯展示无交互):绿色取泊位灯 --lamp,与在线状态点同源。
/// 悬停提示用 SVG 原生 <title> 子元素(lucide-react 类型已不接受 title 属性)
export default function UpgradeMark({ label = '可升级' }: { label?: string }) {
  return (
    <ArrowUp className="size-3 flex-none text-[var(--lamp)]" aria-label={label}>
      <title>{label}</title>
    </ArrowUp>
  );
}
