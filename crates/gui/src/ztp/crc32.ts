/// CRC-32(IEEE 802.3,反射多项式 0xEDB88320),与 Android/iOS 端 CRC32.kt 一致。
/// 查表由代码生成,不硬编码 256 项。返回无符号 32 位。
const table = (() => {
  const t = new Int32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[i] = c;
  }
  return t;
})();

export function crc32(data: Uint8Array): number {
  let crc = -1;
  for (const b of data) {
    crc = table[(crc ^ b) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ -1) >>> 0;
}
