#!/bin/bash
# planet-port 安装脚本
#
# 用法:
#   curl -fsSL https://raw.githubusercontent.com/planet-port/planet-port-desktop/main/scripts/install.sh | sudo bash
#   或下载后执行: sudo ./install.sh [版本号] [--no-service]
#
# 行为:
#   1. 识别平台与架构,从 GitHub Releases 下载对应预编译包
#      (命名参考 zerotier 发布规范: planet-port_<ver>_<platform>_<arch>.tar.gz)
#   2. 二进制安装到 /Library/Application Support/planet-portd/bin/(仅 macOS)
#   3. /usr/local/bin/planet-port 建软链
#   4. 执行 planet-port service install(装 launchd 服务并启动;--no-service 跳过)
#   5. 升级场景:daemon 按 registry 期望状态自动恢复 planet;脚本另做
#      拉回兜底(旧版 daemon 升级时 SIGTERM 会落 stopped,需脚本恢复)
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
    arm64|aarch64) ARCH_TAG="aarch64" ;;
    x86_64|amd64) ARCH_TAG="$( [[ "$PLATFORM" == "linux" ]] && echo amd64 || echo x86_64 )" ;;
    *) ARCH_TAG="" ;;
esac

if [[ "$PLATFORM" != "macos" ]]; then
    die "当前平台(${OS} $(uname -m))的版本尚未发布,已支持: macOS(arm64/x86_64);Linux(ubuntu/mint)、Windows 在发布计划中"
fi

if [[ "$(id -u)" -ne 0 ]]; then
    exec sudo bash "$0" "$@"
fi

APP_BIN="/Library/Application Support/planet-portd/bin"
LINK="/usr/local/bin/planet-port"

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

# ── 记录运行中的 planet(升级兜底:旧版 daemon 的 SIGTERM 会落 stopped,
#    新版才保留期望状态;拉回对新版幂等、对旧版是唯一恢复路径) ────────────
RUNNING=""
if [[ -x "$LINK" ]]; then
    RUNNING="$("$LINK" info --json 2>/dev/null | awk -F'"' '
        /"id":/ { id = $4 }
        /"status": *"running"/ { if (id != "") print id; id = "" }
    ' || true)"
fi

# ── 停止在跑的服务(优雅退出,daemon 自会清理 feth;随后 install 拉起新版) ──
if [[ -x "$LINK" ]]; then
    "$LINK" service stop >/dev/null 2>&1 || true
fi

# ── 安装二进制 ─────────────────────────────────────────────────────────
STAGE="$(mktemp -d)"
tar -xzf "$TARBALL" -C "$STAGE"
[[ -x "$STAGE/planet-port" && -x "$STAGE/planet-portd" ]] \
    || die "包内容不完整(需含可执行的 planet-port 与 planet-portd)"

mkdir -p "$APP_BIN"
# 先落临时名再 mv:目标可能是正在运行的二进制,直接覆盖会 ETXTBSY
for bin in planet-port planet-portd; do
    install -m 755 "${STAGE}/${bin}" "${APP_BIN}/${bin}.new"
    mv -f "${APP_BIN}/${bin}.new" "${APP_BIN}/${bin}"
done
mkdir -p "$(dirname "$LINK")"
ln -sfn "$APP_BIN/planet-port" "$LINK"
rm -rf "$STAGE"

# ── 服务安装 ───────────────────────────────────────────────────────────
if [[ "$NO_SERVICE" -eq 1 ]]; then
    echo "已跳过服务安装(--no-service),需要时执行: sudo planet-port service install"
else
    # daemon 随 service install 拉起,并按 registry 期望状态自动恢复 planet;
    # RUNNING 拉回兜底旧版 daemon 升级(新版幂等,已恢复的 start 直接成功)
    "$LINK" service install
    for id in $RUNNING; do
        "$LINK" start "$id" >/dev/null 2>&1 \
            || echo "警告: planet ${id} 恢复失败,请手动: planet-port start ${id}"
    done
fi

echo
echo "安装完成: ${VERSION} (${PLATFORM} ${ARCH_TAG})"
echo "  CLI:      ${LINK}"
echo "  二进制:   ${APP_BIN}/"
echo "  卸载:     sudo ${LINK} service uninstall && sudo rm ${LINK}"
