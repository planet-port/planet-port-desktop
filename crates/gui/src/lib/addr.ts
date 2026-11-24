/// IP 展示口径(2026-11-24 用户指定):网络卡片/托盘菜单/网络详情统一
/// IPv4 优先。ZeroTier 分配地址形如 10.x.x.x / fd56:...,含 . 无 : 判为
/// IPv4;两段各自保持原序(稳定)。
/// 详情页仍逐行展示全部地址,仅排序;卡片与托盘只取一个(pickDisplayAddr)。
export function sortAddrsPreferV4(addrs: string[]): string[] {
  const isV4 = (a: string) => a.includes('.') && !a.includes(':');
  return [...addrs.filter(isV4), ...addrs.filter(a => !isV4(a))];
}

/// 卡片显示地址:只取一个,IPv4 优先;无地址返回空串(调用方自行占位)
export function pickDisplayAddr(addrs: string[]): string {
  return sortAddrsPreferV4(addrs)[0] ?? '';
}
