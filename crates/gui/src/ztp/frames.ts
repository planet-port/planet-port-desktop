import { deflateSync } from 'fflate';
import * as QRCode from 'qrcode';
import { base45Encode } from './base45';
import { crc32 } from './crc32';

/// 分帧协议(与移动端 QRTransfer 一致):
/// 帧 = "ZTQR1|" + total(2) + "|" + index(2) + "|" + crc32(8,hex) + "|" + len(5) + "|" + chunk
/// total/index 两位十进制(1..99,index 从 1 起);len 为 Base45 全长(每帧重复携带);
/// 均匀分片等宽(末帧补 '0' 对齐 → 各帧模块数一致)。
export const MAGIC = 'ZTQR1';
export const HEADER_LEN = MAGIC.length + 1 + 2 + 1 + 2 + 1 + 8 + 1 + 5 + 1; // = 27

/// 容量探测前缀:与真实帧头等形等长,携带 '|' 强制 byte mode,
/// 尾部用 'z' 填充避免 numeric/segment 优化导致容量高估
const PROBE_PREFIX = 'ZTQR1|99|99|ffffffff|00000|';

export class QRTooLargeError extends Error {
  constructor() {
    super('payload too large for QR frames');
  }
}

/// 文本纠错 M 生成 QR 的模块数(17+4v);不可编码返回 null
export function qrModules(text: string): number | null {
  try {
    return QRCode.create(text, { errorCorrectionLevel: 'M' }).modules.size;
  } catch {
    return null;
  }
}

/// 每帧容量二分探测:最大分片字符数,使探测串模块数 ≤ maxModules
function probeCapacity(maxModules: number): number | null {
  let lo = 1;
  let hi = 2700; // v25(byte mode)字符上界的宽松起点
  let best: number | null = null;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const m = qrModules(PROBE_PREFIX + 'z'.repeat(mid));
    if (m != null && m <= maxModules) {
      best = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

/// 数据 → 压缩容器(4B 小端原始长度 + raw deflate) → Base45 → 分帧
export function encodeFrames(data: Uint8Array, maxModules = 117, maxFrames = 32): string[] {
  const deflated = deflateSync(data, { level: 6 });
  const container = new Uint8Array(4 + deflated.length);
  new DataView(container.buffer).setUint32(0, data.length, true);
  container.set(deflated, 4);
  const encoded = base45Encode(container);
  const capacity = probeCapacity(maxModules);
  if (capacity == null || capacity < 1) throw new QRTooLargeError();
  const frameCount = Math.ceil(encoded.length / capacity);
  if (frameCount > maxFrames) throw new QRTooLargeError();
  const chunkSize = Math.ceil(encoded.length / frameCount);
  const crc = crc32(data).toString(16).padStart(8, '0');
  const len = String(encoded.length).padStart(5, '0');
  const frames: string[] = [];
  for (let i = 1; i <= frameCount; i++) {
    const start = (i - 1) * chunkSize;
    let chunk = encoded.slice(start, Math.min(start + chunkSize, encoded.length));
    if (chunk.length < chunkSize) chunk += '0'.repeat(chunkSize - chunk.length);
    frames.push(`${MAGIC}|${String(frameCount).padStart(2, '0')}|${String(i).padStart(2, '0')}|${crc}|${len}|${chunk}`);
  }
  return frames;
}
