#!/usr/bin/env python3
"""生成 GUI 图标：应用占位图标（实心圆，视觉 polish 后续版本再做）与
托盘两态品牌图标（源图 icons/brand-shape.png，纯黑形状+透明底）。
占位圆用 stdlib（zlib/struct）可重复执行；托盘图需 Pillow 做 resize。"""
import struct, zlib, os

def png(path, size, rgb, alpha=255):
    cx = cy = (size - 1) / 2
    r_out = size * 0.46
    raw = bytearray()   # bytearray += 均摊 O(1)，bytes += 逐像素拼接在大尺寸下是 O(n²)
    for y in range(size):
        raw += b"\x00"
        for x in range(size):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if d <= r_out:
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

def tray(path, size, alpha_scale):
    """从品牌 shape 源图导出托盘态：纯黑形状+透明底(macOS 模板图，
    系统按菜单栏深浅自动着色)；离线态按 alpha_scale 淡化区分。
    注:画布留白对 macOS 渲染无效——系统按 alpha bbox 紧裁后归一化到
    菜单栏标准高度,形状视觉密度由系统决定。"""
    from PIL import Image
    here = os.path.dirname(os.path.abspath(__file__))
    src = os.path.join(here, "..", "crates", "gui", "src-tauri", "icons", "brand-shape.png")
    im = Image.open(src).convert("RGBA").resize((size, size), Image.LANCZOS)
    if alpha_scale < 1:
        a = im.getchannel("A").point(lambda v: int(v * alpha_scale))
        im.putalpha(a)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    im.save(path)
    print("wrote", path)

root = os.path.join(os.path.dirname(__file__), "..", "crates", "gui")
png(os.path.join(root, "app-icon.png"), 1024, (63, 81, 181))                        # 应用图标占位：靛蓝实心圆
tray(os.path.join(root, "src-tauri", "icons", "tray-online.png"), 512, 1.0)         # 托盘在线：实心形状
tray(os.path.join(root, "src-tauri", "icons", "tray-offline.png"), 512, 0.45)       # 托盘离线：淡化形状
