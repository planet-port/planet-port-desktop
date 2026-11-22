/// Base45(RFC 9285)。字母表恰为 QR alphanumeric 全集且不含 '|',
/// 分帧协议可以安全用 '|' 分隔。仅 encode(桌面只发送)。
const ALPHABET = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:';

export function base45Encode(data: Uint8Array): string {
  let out = '';
  for (let i = 0; i < data.length; ) {
    if (data.length - i === 1) {
      const v = data[i];
      out += ALPHABET[v % 45] + ALPHABET[Math.floor(v / 45)];
      i += 1;
    } else {
      const v = (data[i] << 8) | data[i + 1];
      out += ALPHABET[v % 45] + ALPHABET[Math.floor(v / 45) % 45] + ALPHABET[Math.floor(v / 2025)];
      i += 2;
    }
  }
  return out;
}
