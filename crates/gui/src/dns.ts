/// DNS 槽位模型(添加网络抽屉与网络详情共用;对齐 iOS 四槽:
/// IPv4×2 + IPv6×2,custom 段至少填一个)
export interface DnsSlots {
  v4a: string;
  v4b: string;
  v6a: string;
  v6b: string;
}

export const emptySlots = (): DnsSlots => ({ v4a: '', v4b: '', v6a: '', v6b: '' });

export const isIPv4 = (s: string): boolean => {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(s.trim());
  return !!m && m.slice(1).every(o => Number(o) <= 255);
};

/// 近似 inet_pton:十六进制冒号、每段 ≤4 位、「::」至多压缩一次;
/// 压缩态下空段仅允许出现在「::」所在处(首尾空段仅当以「::」开头/结尾,
/// 其余如 ':::'、'a::b:'、':a::b' 一律拒绝),且十六进制组总数 ≤8;
/// 常见 ULA/链路本地形态覆盖,畸形输入拒绝
export const isIPv6 = (s: string): boolean => {
  const t = s.trim();
  if (!t.includes(':')) return false;
  if (!/^[0-9a-fA-F:]+$/.test(t)) return false;
  const compressed = t.split('::');
  if (compressed.length > 2) return false;
  let segs: string[];
  if (compressed.length === 2) {
    // 「::」两侧不得再出现裸冒号产生的空段
    const head = compressed[0] === '' ? [] : compressed[0].split(':');
    const tail = compressed[1] === '' ? [] : compressed[1].split(':');
    if (head.some(seg => seg === '') || tail.some(seg => seg === '')) return false;
    segs = [...head, ...tail];
  } else {
    // 无「::」压缩时不得有裸冒号产生的空段(如 ':fd00:1'、'fd00:1:')
    segs = t.split(':');
    if (segs.some(seg => seg === '')) return false;
  }
  if (segs.length > 8) return false;
  return segs.every(seg => /^[0-9a-fA-F]{1,4}$/.test(seg));
};

export function slotsValid(s: DnsSlots): boolean {
  const all = [s.v4a, s.v4b, s.v6a, s.v6b].map(x => x.trim());
  if (all.every(x => !x)) return false; // custom 至少一个
  return [s.v4a, s.v4b].every(x => !x.trim() || isIPv4(x)) && [s.v6a, s.v6b].every(x => !x.trim() || isIPv6(x));
}

/// 线格式(string[]):v4 在前(与 daemon customDns 语义一致)
export function slotsToServers(s: DnsSlots): string[] {
  return [s.v4a, s.v4b, s.v6a, s.v6b].map(x => x.trim()).filter(Boolean);
}

/// 线格式 → 槽位:v4 段进 v4 槽、其余进 v6 槽,各取前两个,超出丢弃
export function serversToSlots(list: string[]): DnsSlots {
  const v4 = list.filter(isIPv4);
  const v6 = list.filter(x => !isIPv4(x));
  return { v4a: v4[0] ?? '', v4b: v4[1] ?? '', v6a: v6[0] ?? '', v6b: v6[1] ?? '' };
}
