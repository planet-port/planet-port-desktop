import { describe, expect, it } from 'vitest';
import { deflateSync } from 'fflate';
import { base45Encode } from '../ztp/base45';
import { crc32 } from '../ztp/crc32';
import { encodeFrames, QRTooLargeError, qrModules, HEADER_LEN } from '../ztp/frames';

const enc = (s: string) => new TextEncoder().encode(s);

/// 确定性伪随机字节(deflate 不可压缩)。多帧/超限用例必须用它驱动——
/// 'x'.repeat(N) 这类重复数据会被 deflate 压到几十字节,永远落单帧
function pseudoRandom(n: number): Uint8Array {
  const out = new Uint8Array(n);
  let s = 0x12345678;
  for (let i = 0; i < n; i++) {
    s ^= s << 13; s ^= s >>> 17; s ^= s << 5; // xorshift32
    out[i] = s & 0xff;
  }
  return out;
}

describe('qrModules', () => {
  it('已知模块数: 纠错 M 下 "ZTP" 短串落最小版本(v1=21)', () => {
    // qrcode 库 modules.size = 17 + 4*version
    expect(qrModules('ZTP')).toBe(21);
  });
  it('超出 QR 最大版本容量 → null', () => {
    // v40 byte mode 容量约 2953 字节,超长文本不可编码
    expect(qrModules('a'.repeat(3000))).toBeNull();
  });
});

describe('encodeFrames', () => {
  it('单帧: 帧串 = magic|01|01|crc|len|chunk(手工重算逐字段对齐)', () => {
    const data = enc('hello ztp');
    const frames = encodeFrames(data);
    expect(frames).toHaveLength(1);
    const crc = crc32(data).toString(16).padStart(8, '0');
    // 期望帧: 4B 小端原始长度 + raw deflate(level 6) → base45
    const deflated = deflateSync(data, { level: 6 });
    const withLen = new Uint8Array(4 + deflated.length);
    new DataView(withLen.buffer).setUint32(0, data.length, true);
    withLen.set(deflated, 4);
    const b45 = base45Encode(withLen);
    expect(frames[0]).toBe(`ZTQR1|01|01|${crc}|${String(b45.length).padStart(5, '0')}|${b45}`);
  });

  it('多帧: total/index 两位十进制,每帧等宽,末帧补 0', () => {
    const data = pseudoRandom(300);
    const frames = encodeFrames(data, 53); // 压低 maxModules 强制多帧
    expect(frames.length).toBeGreaterThan(1);
    for (const [i, f] of frames.entries()) {
      const parts = f.split('|');
      expect(parts[0]).toBe('ZTQR1');
      expect(parts[1]).toBe(String(frames.length).padStart(2, '0'));
      expect(parts[2]).toBe(String(i + 1).padStart(2, '0'));
      expect(parts[3]).toBe(crc32(data).toString(16).padStart(8, '0'));
    }
    const chunks = frames.map(f => f.split('|')[5]);
    expect(new Set(chunks.map(c => c.length)).size).toBe(1);
  });

  it('超 maxFrames 抛 QRTooLargeError', () => {
    expect(() => encodeFrames(pseudoRandom(400), 53, 2)).toThrow(QRTooLargeError);
  });

  it('HEADER_LEN 为 27(与移动端 QRTransfer.headerLen 一致)', () => {
    expect(HEADER_LEN).toBe(27);
  });
});
