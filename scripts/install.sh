#!/bin/bash
# planet-port 安装脚本
#
# 用法:
#   curl -fsSL https://raw.githubusercontent.com/planet-port/planet-port-desktop/main/scripts/install.sh | sudo bash
#   或下载后执行: sudo ./install.sh [版本号] [--no-service]
#
# 行为:
#   1. 识别平台与架构,从 GitHub Releases 下载对应预编译包
#      (命名参考 zerotier 发布规范: planet-port_<ver>_<platform>_<arch>.tar.gz;
#      macOS: macos_aarch64 / macos_x86_64;Linux: linux_amd64 / linux_arm64)
#   2. --no-service 时仅落盘二进制:
#      - macOS: /Library/Application Support/planet-portd/bin/ + /usr/local/bin/planet-port 软链
#      - Linux: 直接落 /usr/local/bin/planet-port 与 /usr/local/bin/planet-portd(无软链)
#   3. 否则执行 planet-port install(安装主体在 CLI 内:
#      macOS 装 launchd 服务;Linux 装 /etc/systemd/system/planet-portd.service 并启动)
#   4. 升级场景:daemon 按 registry 期望状态自动恢复 planet;CLI 另做
#      拉回兜底(旧版 daemon 升级时 SIGTERM 会落 stopped,需恢复)
#
# 环境变量:
#   PLANET_PORT_VERSION  指定版本(如 v0.1.1),默认取 Releases 最新
#   PLANET_PORT_TARBALL  用本地 tar.gz 代替下载(测试用)

set -euo pipefail

REPO="planet-port/planet-port-desktop"

die() { echo "错误: $*" >&2; exit 1; }

# ── 平台识别(产物命名: macos_aarch64 / macos_x86_64 / linux_amd64 / windows_x86_64) ──
OS="$(uname -s)"
case "$OS" in
    Darwin) PLATFORM="macos" ;;
    Linux)
        # ubuntu / mint 等 Debian 系共用 linux_amd64 产物
        PLATFORM="linux"
        ;;
    *) PLATFORM="unsupported" ;;
esac
case "$(uname -m)" in
    arm64|aarch64) ARCH_TAG="$( [[ "$PLATFORM" == "linux" ]] && echo arm64 || echo aarch64 )" ;;
    x86_64|amd64)  ARCH_TAG="$( [[ "$PLATFORM" == "linux" ]] && echo amd64 || echo x86_64 )" ;;
    *) ARCH_TAG="" ;;
esac

if [[ "$PLATFORM" == "unsupported" || -z "$ARCH_TAG" ]]; then
    die "当前平台(${OS} $(uname -m))的版本尚未发布,已支持: macOS(arm64/x86_64)、Linux(amd64/arm64,glibc≥2.31);Windows 在发布计划中"
fi

if [[ "$(id -u)" -ne 0 ]]; then
    exec sudo bash "$0" "$@"
fi

if [[ "$PLATFORM" == "macos" ]]; then
    APP_BIN="/Library/Application Support/planet-portd/bin"
    LINK="/usr/local/bin/planet-port"
else
    APP_BIN="/usr/local/bin"
    LINK="/usr/local/bin/planet-port"
fi

# ── 参数 ──────────────────────────────────────────────────────────────
NO_SERVICE=0
for arg in "$@"; do
    case "$arg" in
        --no-service) NO_SERVICE=1 ;;
        v[0-9]*|[0-9]*) export PLANET_PORT_VERSION="$arg" ;;
        *) die "未知参数: $arg" ;;
    esac
done

# ── 版本与下载 ─────────────────────────────────────────────────────────
if [[ -n "${PLANET_PORT_TARBALL:-}" ]]; then
    TARBALL="$PLANET_PORT_TARBALL"
    [[ -f "$TARBALL" ]] || die "本地包不存在: $TARBALL"
    VERSION="${PLANET_PORT_VERSION:-本地包}"
else
    command -v curl >/dev/null || die "缺少 curl"
    if [[ -n "${PLANET_PORT_VERSION:-}" ]]; then
        TAG="v${PLANET_PORT_VERSION#v}"
    else
        TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
        [[ -n "$TAG" ]] || die "获取最新版本失败(仓库可能还没有 Release)"
    fi
    VERSION="${TAG#v}"
    ASSET="planet-port_${VERSION}_${PLATFORM}_${ARCH_TAG}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
    echo "下载 ${URL}"
    curl -fSL --progress-bar -o "${TMP}/${ASSET}" "$URL" || die "下载失败(确认 Release 已附带 ${ASSET})"
    TARBALL="${TMP}/${ASSET}"
fi

# ── 解包并安装 ─────────────────────────────────────────────────────────
# 一键安装(二进制落盘 + PATH + launchd/systemd 服务)由 `planet-port install`
# 自带,含升级兜底(记录 running planet、装完后拉回);本脚本只负责
# 「下载 + 解压 + 调起」,不再重复实现拷贝/软链/恢复逻辑
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE" ${TMP:-}' EXIT
tar -xzf "$TARBALL" -C "$STAGE"
[[ -x "$STAGE/planet-port" && -x "$STAGE/planet-portd" ]] \
    || die "包内容不完整(需含可执行的 planet-port 与 planet-portd)"

if [[ "$NO_SERVICE" -eq 1 ]]; then
    # 仅落盘二进制,不装服务(软链仅 macOS 需要;Linux 直接落 /usr/local/bin)
    mkdir -p "$APP_BIN"
    for bin in planet-port planet-portd; do
        install -m 755 "${STAGE}/${bin}" "${APP_BIN}/${bin}.new"
        mv -f "${APP_BIN}/${bin}.new" "${APP_BIN}/${bin}"
    done
    if [[ "$PLATFORM" == "macos" ]]; then
        mkdir -p "$(dirname "$LINK")"
        ln -sfn "$APP_BIN/planet-port" "$LINK"
    fi
    echo "已跳过服务安装(--no-service),需要时执行: sudo planet-port install"
else
    "${STAGE}/planet-port" install
fi
rm -rf "$STAGE"

echo
echo "安装完成: ${VERSION} (${PLATFORM} ${ARCH_TAG})"
echo "  CLI:      ${LINK}"
echo "  二进制:   ${APP_BIN}/"
if [[ "$PLATFORM" == "macos" ]]; then
    echo "  卸载:     sudo ${LINK} service uninstall && sudo rm ${LINK}"
else
    echo "  服务:     /etc/systemd/system/planet-portd.service"
    echo "  卸载:     sudo ${LINK} service uninstall && sudo rm -f /usr/local/bin/planet-port /usr/local/bin/planet-portd"
fi
