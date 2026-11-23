#!/usr/bin/env python3
"""生成托盘两色四态图标(需 Pillow)。

口径(用户指定):mac/linux 托盘恒用白色单色形状,win 恒用彩色 logo,
均取 256 源图;每色两态:online 实心 / offline 按 alpha 45% 淡化区分。
裁掉形状外围透明像素,源图尺度各留 12px 边距保证饱满,再缩到目标尺寸。
应用图标(dock/安装器/快捷方式)不走本脚本,由 `tauri icon <彩色 1024 源图>` 生成。"""
import os

def tray(path, size, alpha_scale, src_name):
    from PIL import Image
    here = os.path.dirname(os.path.abspath(__file__))
    src = os.path.join(here, "..", "crates", "gui", "src-tauri", "icons", src_name)
    base = Image.open(src).convert("RGBA")
    x0, y0, x1, y1 = base.getchannel("A").getbbox()
    pad = 12
    box = (max(0, x0 - pad), max(0, y0 - pad), min(base.width, x1 + pad), min(base.height, y1 + pad))
    im = base.crop(box).resize((size, size), Image.LANCZOS)
    if alpha_scale < 1:
        a = im.getchannel("A").point(lambda v: int(v * alpha_scale))
        im.putalpha(a)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    im.save(path)
    print("wrote", path)

icons = os.path.join(os.path.dirname(__file__), "..", "crates", "gui", "src-tauri", "icons")
for tone in ("light", "color"):
    tray(os.path.join(icons, f"tray-{tone}-online.png"), 256, 1.0, f"shape-{tone}.png" if tone == "light" else "logo-color.png")
    tray(os.path.join(icons, f"tray-{tone}-offline.png"), 256, 0.45, f"shape-{tone}.png" if tone == "light" else "logo-color.png")
