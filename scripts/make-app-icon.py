#!/usr/bin/env python3
"""加工 macOS 规范应用图标源(需 Pillow/numpy)。

原始彩色 logo 是满幅 iOS 小圆角方块;macOS Big Sur 起 Dock/Launchpad 图标
为 squircle(超椭圆连续圆角)底板,内容区约 824/1024、四周留边——满幅方块
在 Dock 中比邻居大一圈且圆角风格不合。tauri icon 只缩放不加底板,故先经
本脚本重绘加工源:

  1. 从彩色源采样对角渐变两端色(左上亮/右下暗;仅取角部 6% 区域且剔除
     近白像素,避开图形与圆角外透明区)
  2. 824px 底板 = 对角线性渐变 × 超椭圆 |x/a|^5+|y/a|^5=1 mask
     (Apple squircle 通行近似;4x 超采样后 LANCZOS 缩回保证边缘平滑)
  3. 图形层用白色单色形状源(shape-light-1024)裁 alpha bbox 缩至底板
     75% 宽居中——与彩色源图形同款,免从实底做颜色剥离;辐射臂朝向
     八方,bbox 角部留白天然充足,不会被 squircle 圆角裁到
  4. 输出 1024 透明画布加工源,随后 npx tauri icon 全套生成
     (icns/ico/Square 系;android/ios 顺带产物无引用,删去)

托盘图标不走本脚本(见 make-icons.py);窗口内 22px 顶栏 logo 用满幅原版
(src/assets),小尺寸无系统网格压力,保持饱满。
"""
import os

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ICONS = os.path.join(HERE, "..", "crates", "gui", "src-tauri", "icons")
CANVAS = 1024
PLATE = 824  # macOS 图标内容区(约 80.5% 画布)
SHAPE_RATIO = 0.75  # 图形层占底板宽
SS = 4  # mask 超采样倍数


def sample_gradient(im: Image.Image) -> tuple[tuple[int, ...], tuple[int, ...]]:
    """采角部中位色。角部 6%×6% 区域,剔近白(图形)像素后取中位数"""
    a = np.asarray(im.convert("RGBA"), dtype=np.int16)
    h, w = a.shape[:2]
    r = int(min(h, w) * 0.06)
    near_white = (a[..., 0] > 230) & (a[..., 1] > 230) & (a[..., 2] > 230)
    opaque = a[..., 3] > 0

    def corner(y0, y1, x0, x1):
        m = opaque[y0:y1, x0:x1] & ~near_white[y0:y1, x0:x1]
        px = a[y0:y1, x0:x1][m]
        return tuple(int(v) for v in np.median(px[:, :3], axis=0))

    return corner(0, r, 0, r), corner(h - r, h, w - r, w)


def squircle_mask(size: int) -> Image.Image:
    """超椭圆 n=5 mask,SS 倍超采样后 LANCZOS 缩回"""
    s = size * SS
    half = s / 2
    ys, xs = np.mgrid[0:s, 0:s]
    nx = np.abs(xs - half + 0.5) / half
    ny = np.abs(ys - half + 0.5) / half
    inside = (nx**5 + ny**5) <= 1
    return Image.fromarray((inside * 255).astype(np.uint8), "L").resize(
        (size, size), Image.LANCZOS
    )


def gradient_plate(c1, c2, size: int) -> Image.Image:
    """对角线性渐变 RGB(左上 c1 → 右下 c2),向量化逐像素"""
    c1, c2 = np.array(c1, dtype=float), np.array(c2, dtype=float)
    xs = np.linspace(0, 1, size)
    t = (xs[None, :] + xs[:, None]) / 2  # 对角参数
    rgb = (c1[None, None, :] * (1 - t[..., None]) + c2[None, None, :] * t[..., None])
    return Image.fromarray(rgb.astype(np.uint8), "RGB")


def main() -> None:
    color_src = Image.open(os.path.join(ICONS, "logo-color-1024.png"))
    c1, c2 = sample_gradient(color_src)
    print(f"gradient {c1} -> {c2}")

    plate = gradient_plate(c1, c2, PLATE)
    plate.putalpha(squircle_mask(PLATE))

    shape = Image.open(os.path.join(ICONS, "shape-light-1024.png")).convert("RGBA")
    bbox = shape.getchannel("A").getbbox()
    shape = shape.crop(bbox)
    target = int(PLATE * SHAPE_RATIO)
    scale = target / shape.width
    shape = shape.resize((target, round(shape.height * scale)), Image.LANCZOS)

    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    off = (CANVAS - PLATE) // 2
    out.paste(plate, (off, off), plate)
    out.paste(
        shape,
        (off + (PLATE - shape.width) // 2, off + (PLATE - shape.height) // 2),
        shape,
    )
    dst = os.path.join(ICONS, "app-icon-mac.png")
    out.save(dst)
    print("wrote", dst)


if __name__ == "__main__":
    main()
