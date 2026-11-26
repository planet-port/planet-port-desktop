#!/usr/bin/env python3
"""全端品牌 logo 分发管线。

设计稿在 Resources/Logo/ 同名覆盖源图后,跑本脚本一次刷新五端图源,
再于各仓提交。文件名契约见 docs/superpowers/specs/2026-11-26-logo-refresh-design.md,
仅认 6 个源文件,设计师其余导出中间产物一律忽略。

用法: python3 scripts/sync-logo.py [--src <Resources/Logo 路径>]
                [--ends desktop,android,ios,harmony,website]"""
import argparse
import os
import shutil
import subprocess
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # planet-port-desktop
ZT = os.path.dirname(ROOT)  # Zerotier/（五仓与 Resources 的公共父目录）
ICONS = os.path.join(ROOT, "crates", "gui", "src-tauri", "icons")

SOURCES = {
    "main1024": "Planet Port - 1024.png",
    "color256": "Planet Port - 256.png",
    "android_bg": "Planet Port - Android - 1024 - bg.png",
    "android_fg48": "Planet Port - Android - 1024 - fg48.png",
    "shape_light_1024": "Planet Port - Android - 1024 - full shape.png",
    "shape_light_256": "Planet Port Shape - 256 - light.png",
}

# iOS appiconset 输出文件名 → 像素尺寸（对齐 Contents.json 各条目 size×scale）
IOS_ICON_SIZES = {
    "icon_20@2x.png": 40,
    "icon_20@3x.png": 60,
    "icon_29@2x.png": 58,
    "icon_29@3x.png": 87,
    "icon_40@2x.png": 80,
    "icon_40@3x.png": 120,
    "icon_60@2x.png": 120,
    "icon_60@3x.png": 180,
    "ipad_icon_20@1x.png": 20,
    "ipad_icon_20@2x.png": 40,
    "ipad_icon_29@1x.png": 29,
    "ipad_icon_29@2x.png": 58,
    "ipad_icon_40@1x.png": 40,
    "ipad_icon_40@2x.png": 80,
    "ipad_icon_76@1x.png": 76,
    "ipad_icon_76@2x.png": 152,
    "ipad_icon_83@2x.png": 167,
    "icon_1024.png": 1024,
}


def copy(src: str, dst: str) -> None:
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(src, dst)
    print("copied", dst)


def resize(src: str, dst: str, size: int) -> None:
    im = Image.open(src).convert("RGBA")
    if im.width != im.height:
        sys.exit(f"源图非正方形: {src} {im.size}")
    im = im.resize((size, size), Image.LANCZOS)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    im.save(dst)
    print("wrote", dst, f"{size}x{size}")


def webp(src: str, dst: str, size: int) -> None:
    im = Image.open(src).convert("RGBA")
    if im.width != im.height:
        sys.exit(f"源图非正方形: {src} {im.size}")
    im = im.resize((size, size), Image.LANCZOS)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    im.save(dst, "WEBP", quality=90)
    print("wrote", dst, f"{size}x{size} webp")


def load_sources(src_dir: str) -> dict[str, str]:
    out = {k: os.path.join(src_dir, v) for k, v in SOURCES.items()}
    missing = [p for p in out.values() if not os.path.isfile(p)]
    if missing:
        sys.exit("缺少图源:\n  " + "\n  ".join(missing))
    return out


def desktop(s: dict[str, str]) -> None:
    copy(s["main1024"], os.path.join(ICONS, "logo-color-1024.png"))
    copy(s["color256"], os.path.join(ICONS, "logo-color.png"))
    copy(s["shape_light_256"], os.path.join(ICONS, "shape-light.png"))
    copy(s["shape_light_1024"], os.path.join(ICONS, "shape-light-1024.png"))
    copy(s["main1024"], os.path.join(ROOT, "crates", "gui", "src", "assets", "icon-1024.png"))

    subprocess.run([sys.executable, os.path.join(HERE, "make-app-icon.py")], check=True)
    # tauri icon 会顺带产出 android/ ios/ 子目录(无引用),沿用既往口径删去
    subprocess.run(
        ["npx", "tauri", "icon", os.path.join(ICONS, "app-icon-mac.png")],
        cwd=os.path.join(ROOT, "crates", "gui"),
        check=True,
    )
    for d in ("android", "ios"):
        p = os.path.join(ICONS, d)
        if os.path.isdir(p):
            shutil.rmtree(p)
            print("removed", p)
    subprocess.run([sys.executable, os.path.join(HERE, "make-icons.py")], check=True)


def android(s: dict[str, str]) -> None:
    repo = os.path.join(ZT, "planet-port-android")
    res = os.path.join(repo, "app", "src", "main", "res")
    copy(s["android_bg"], os.path.join(res, "drawable-nodpi", "ic_launcher_bg.png"))
    copy(s["android_fg48"], os.path.join(res, "drawable-nodpi", "ic_launcher_fg.png"))
    copy(s["main1024"], os.path.join(res, "drawable-nodpi", "planet_port_logo.png"))
    webp(s["main1024"], os.path.join(repo, "docs", "logo.webp"), 240)


def ios(s: dict[str, str]) -> None:
    repo = os.path.join(ZT, "planet-port-ios")
    assets = os.path.join(repo, "PlanetPort", "Assets.xcassets")
    appicon = os.path.join(assets, "AppIcon.appiconset")
    copy(s["main1024"], os.path.join(appicon, "icon_1024.png"))
    for name, size in IOS_ICON_SIZES.items():
        if name == "icon_1024.png":
            continue  # icon_1024 走上方 copy 直拷,重采样会破坏直拷语义
        resize(s["main1024"], os.path.join(appicon, name), size)
    copy(s["main1024"], os.path.join(assets, "PlanetPortLogo.imageset", "planetportlogo.png"))
    webp(s["main1024"], os.path.join(repo, "docs", "logo.webp"), 1024)


def harmony(s: dict[str, str]) -> None:
    repo = os.path.join(ZT, "planet-port-harmony")
    resize(s["main1024"], os.path.join(repo, "AppScope", "resources", "base", "media", "app_icon.png"), 216)
    resize(s["main1024"], os.path.join(repo, "entry", "src", "main", "resources", "base", "media", "app_icon.png"), 216)


def website(s: dict[str, str]) -> None:
    repo = os.path.join(ZT, "planet-port-website")
    webp(s["color256"], os.path.join(repo, "public", "assets", "planet-port-logo.webp"), 240)


ENDS = {"desktop": desktop, "android": android, "ios": ios, "harmony": harmony, "website": website}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--src", default=os.path.join(ZT, "Resources", "Logo"))
    ap.add_argument("--ends", default=",".join(ENDS), help="逗号分隔: desktop,android,ios,harmony,website")
    args = ap.parse_args()
    s = load_sources(args.src)
    for name in args.ends.split(","):
        name = name.strip()
        if name not in ENDS:
            sys.exit(f"未知端: {name}（可选: {','.join(ENDS)}）")
        print(f"== {name} ==")
        ENDS[name](s)


if __name__ == "__main__":
    main()
