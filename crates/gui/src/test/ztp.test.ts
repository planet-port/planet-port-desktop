import { describe, expect, it } from 'vitest';
import { base45Encode } from '../ztp/base45';
import { crc32 } from '../ztp/crc32';
import { planetPayloadContainer, ztpEncode } from '../ztp/container';

const enc = (s: string) => new TextEncoder().encode(s);
const le32 = (n: number) => [n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff];

describe('crc32 (IEEE 802.3, 反射 0xEDB88320)', () => {
  it('标准校验向量', () => {
    expect(crc32(enc('123456789'))).toBe(0xcbf43926);
    expect(crc32(new Uint8Array(0))).toBe(0x00000000);
  });
});

describe('base45 (RFC 9285)', () => {
  it('规范示例: "AB" → "BB8", "Hello!!" → "%69 VD92EX0"', () => {
    expect(base45Encode(enc('AB'))).toBe('BB8');
    expect(base45Encode(enc('Hello!!'))).toBe('%69 VD92EX0');
  });
  it('落单字节走两位编码', () => {
    // 0x41=65: 65%45=20→'K', 65/45=1→'1'
    expect(base45Encode(new Uint8Array([0x41]))).toBe('K1');
  });
});

describe('ZTP 容器', () => {
  it('布局: magic + key(1B) + len(4B 小端) + value', () => {
    const out = ztpEncode([[2, enc('gozi25')]]);
    expect(Array.from(out)).toEqual([...enc('ZTP'), 2, ...le32(6), ...enc('gozi25')]);
  });
  it('key=0 条目丢弃; 空列表产出仅 magic', () => {
    expect(Array.from(ztpEncode([]))).toEqual(Array.from(enc('ZTP')));
    expect(Array.from(ztpEncode([[0, enc('x')]]))).toEqual(Array.from(enc('ZTP')));
  });
  it('PlanetPayload: planet 必带,空名省略,icon/color null 省略', () => {
    const p = new Uint8Array([1, 2, 3]);
    const only = planetPayloadContainer(p, '', null, null);
    expect(Array.from(only)).toEqual([...enc('ZTP'), 1, ...le32(3), 1, 2, 3]);
    const full = planetPayloadContainer(p, 'n', 'cat', '#ff0000');
    // 4 个条目: 1=planet 2=name 3=icon 4=color
    expect(full[3]).toBe(1);
    expect(full[3 + 5 + 3]).toBe(2);
    expect(full[3 + 5 + 3 + 5 + 1]).toBe(3);
    expect(full[3 + 5 + 3 + 5 + 1 + 5 + 3]).toBe(4);
  });
});
