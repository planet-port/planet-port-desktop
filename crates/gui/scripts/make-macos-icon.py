#!/usr/bin/env python3
"""macOS 应用图标重制:Big Sur 规范 squircle + 红渐变 + 白色枢纽符号。

iOS 满铺源图直接上 Dock 会以尖角满幅方块呈现(macOS 不做自动圆角)。
本脚本从源图采样渐变色、按亮度阈值抠出白色符号,重排为:
  1024 透明画布 + 824 圆角容器(半径 185) + 符号缩至容器 62% 居中,
并生成 iconset 全档后经 iconutil 转 icns 覆盖 src-tauri/icons/icon.icns。
Windows/Linux 仍用 tauri 生成的满铺版(平台惯例即方图)。
用法: python3 make-macos-icon.py
依赖: Pillow;iconutil(仅 macOS 自带)。
"""
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

SRC = Path(__file__).resolve().parent.parent / "src" / "assets" / "icon-1024.png"
ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
PREVIEW = Path(__file__).resolve().parents[3] / "tmp" / "icon-mac-preview.png"

CANVAS = 1024
SQUIRCLE = 824
RADIUS = 185          # ≈ 824 × 22.5%
SYMBOL_RATIO = 0.62   # 符号外接框 / squircle(iOS 源图符号占 75-78%,直缩会顶边)
SYMBOL_COLOR = (247, 242, 236, 255)  # 源图符号实测暖白


def sample_gradient(src: Image.Image):
    """自源图左缘上下采样渐变端点色(符号 x 起点 ≈ 12.7%,取 2%-10% 带避让)"""
    rgb = src.convert("RGB")
    w, h = rgb.size

    def strip(yf):
        xs = range(int(w * 0.02), int(w * 0.10), 4)
        y = int(h * yf)
        rs = gs = bs = 0
        n = 0
        for x in xs:
            r, g, b = rgb.getpixel((x, y))
            rs += r
            gs += g
            bs += b
            n += 1
        return rs // n, gs // n, bs // n

    return strip(0.01), strip(0.99)


def load_symbol(src: Image.Image) -> Image.Image:
    """亮度阈值抠白色符号(红底亮度 ≈127、暖白符号 ≈242,分离干净)"""
    lum = src.convert("RGB").convert("L")
    mask = lum.point(lambda p: 255 if p > 190 else 0)
    box = mask.getbbox()
    sym = Image.new("RGBA", (box[2] - box[0], box[3] - box[1]), (0, 0, 0, 0))
    sym.paste(Image.new("RGBA", sym.size, SYMBOL_COLOR), (0, 0), mask.crop(box))
    return sym


def gradient_squircle(top, bottom) -> Image.Image:
    """824×824 纵向渐变圆角容器(透明背景)"""
    base = Image.new("RGB", (SQUIRCLE, SQUIRCLE))
    draw = ImageDraw.Draw(base)
    for y in range(SQUIRCLE):
        t = y / (SQUIRCLE - 1)
        draw.line(
            [(0, y), (SQUIRCLE, y)],
            fill=tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)),
        )
    mask = Image.new("L", (SQUIRCLE, SQUIRCLE), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, SQUIRCLE - 1, SQUIRCLE - 1], radius=RADIUS, fill=255)
    out = Image.new("RGBA", (SQUIRCLE, SQUIRCLE), (0, 0, 0, 0))
    out.paste(base, (0, 0), mask)
    return out


def main():
    src = Image.open(SRC)
    top, bottom = sample_gradient(src)
    icon = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    sq = gradient_squircle(top, bottom)
    icon.paste(sq, ((CANVAS - SQUIRCLE) // 2, (CANVAS - SQUIRCLE) // 2), sq)
    sym = load_symbol(src)
    side = round(SQUIRCLE * SYMBOL_RATIO)
    scale = side / max(sym.size)
    sym2 = sym.resize((round(sym.width * scale), round(sym.height * scale)), Image.LANCZOS)
    icon.alpha_composite(sym2, ((CANVAS - sym2.width) // 2, (CANVAS - sym2.height) // 2))

    PREVIEW.parent.mkdir(parents=True, exist_ok=True)
    icon.save(PREVIEW)
    print(f"preview -> {PREVIEW}")

    with tempfile.TemporaryDirectory() as td:
        iconset = Path(td) / "icon.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            icon.resize((size, size), Image.LANCZOS).save(iconset / f"icon_{size}x{size}.png")
            icon.resize((size * 2, size * 2), Image.LANCZOS).save(iconset / f"icon_{size}x{size}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset)], check=True, cwd=td)
        shutil.copy(Path(td) / "icon.icns", ICONS / "icon.icns")
    print(f"rewrote {ICONS / 'icon.icns'}")


if __name__ == "__main__":
    main()
