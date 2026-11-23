/// 项目表单标签:纯再出口 shadcn 原件 ui/label.tsx(Radix Label,官方源码不动)。
/// 现有项目 <label> 无独立样式基线(styles.css 未定义 label 规则),按迁移
/// 口径保持原件语义(text-sm 14px / font-medium,较原生 label 的 13px/400 略
/// 有增强);后续如需回落基线,在本文件改为函数封装 + cn 覆盖即可。
export { Label } from '../ui/label';
