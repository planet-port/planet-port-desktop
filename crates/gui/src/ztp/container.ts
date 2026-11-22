/// ZTP KV 容器(wire format),与移动端 PayloadContainer 一致:
/// 容器 = magic "ZTP" + 无序条目流,条目 = key(1B) + len(4B 小端) + value。
const MAGIC = [0x5a, 0x54, 0x50]; // "ZTP"

function le32(n: number): number[] {
  return [n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff];
}

/// key 为 0(非法哨兵)的字段直接丢弃,空列表产出仅含 magic 的合法空容器
export function ztpEncode(fields: Array<[number, Uint8Array]>): Uint8Array {
  const valid = fields.filter(([k]) => k !== 0);
  const size = MAGIC.length + valid.reduce((s, [, v]) => s + 5 + v.length, 0);
  const out = new Uint8Array(size);
  let pos = 0;
  for (const b of MAGIC) out[pos++] = b;
  for (const [key, value] of valid) {
    out[pos++] = key;
    for (const b of le32(value.length)) out[pos++] = b;
    out.set(value, pos);
    pos += value.length;
  }
  return out;
}

/// PlanetPort 应用层 key 约定:1=planet 二进制(必备) 2=名称 3=图标 key 4=图标底色 hex
const KEY_PLANET = 1;
const KEY_NAME = 2;
const KEY_ICON = 3;
const KEY_ICON_COLOR = 4;

/// 名称必带字段但空名防御性省略;icon/iconColor 为 null 时省略条目
export function planetPayloadContainer(
  planet: Uint8Array,
  name: string,
  icon: string | null,
  iconColor: string | null,
): Uint8Array {
  const fields: Array<[number, Uint8Array]> = [[KEY_PLANET, planet]];
  if (name) fields.push([KEY_NAME, new TextEncoder().encode(name)]);
  if (icon != null) fields.push([KEY_ICON, new TextEncoder().encode(icon)]);
  if (iconColor != null) fields.push([KEY_ICON_COLOR, new TextEncoder().encode(iconColor)]);
  return ztpEncode(fields);
}
