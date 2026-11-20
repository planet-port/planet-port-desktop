#!/usr/bin/env python3
"""生成 GUI 占位图标：实心圆风格（在线=靛蓝 / 离线=灰 / 应用=靛蓝 1024）。
仅用 stdlib（zlib/struct），可重复执行。视觉 polish 后续版本再做。"""
import struct, zlib, os

def png(path, size, rgb, alpha=255, ring=False):
    cx = cy = (size - 1) / 2
    r_out, r_in = size * 0.46, size * 0.30
    raw = bytearray()   # bytearray += 均摊 O(1)，bytes += 逐像素拼接在大尺寸下是 O(n²)
    for y in range(size):
        raw += b"\x00"
        for x in range(size):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if ring and r_in <= d <= r_out:
                raw += bytes(rgb) + bytes([alpha])
            elif not ring and d <= r_out:
                raw += bytes(rgb) + bytes([alpha])
            else:
                raw += b"\x00\x00\x00\x00"
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(out)
    print("wrote", path)

root = os.path.join(os.path.dirname(__file__), "..", "crates", "gui")
png(os.path.join(root, "app-icon.png"), 1024, (63, 81, 181))                       # 应用图标：靛蓝实心圆
png(os.path.join(root, "src-tauri", "icons", "tray-online.png"), 512, (63, 81, 181), ring=True)   # 托盘在线：靛蓝圆环
png(os.path.join(root, "src-tauri", "icons", "tray-offline.png"), 512, (158, 158, 158), ring=True) # 托盘离线：灰圆环
