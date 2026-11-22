/// 占位实现(Task 10 替换为完整抽屉)
export default function SettingsDrawer({ open }: { open: boolean; onClose: () => void }) {
  if (!open) return null;
  return <div className="drawer-placeholder" />;
}
