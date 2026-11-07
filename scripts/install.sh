#!/bin/bash
# planet-port 安装脚本
#
# 用法:
#   curl -fsSL https://raw.githubusercontent.com/planet-port/planet-port-desktop/main/scripts/install.sh | sudo bash
#   或下载后执行: sudo ./install.sh [版本号] [--no-service]
#   GitHub 直连不通(国内常见)时显式指定代理前缀:
#   ... | sudo bash -s -- --proxy=<代理前缀>   (代理前缀形如 https://某代理站/)
#   (或环境变量 PLANET_PORT_PROXY,注意 sudo 会清洗环境,管道场景用 -s -- 传参)
#
# 行为:
#   1. 识别平台与架构,从 GitHub Releases 下载对应预编译包
#      (cargo-dist 规范命名: planet-port-<target>.tar.xz;Windows 为
#      planet-port-x86_64-pc-windows-msvc.zip;
#      target 如 aarch64-apple-darwin / x86_64-unknown-linux-gnu)
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
#   PLANET_PORT_TARBALL  用本地包代替下载(测试用)
#   PLANET_PORT_PREFIX   落盘根目录覆盖,仅供沙箱测试;正常安装不设
#   PLANET_PORT_PROXY    GitHub 代理前缀(同 --proxy;sudo 会清洗环境,慎用)
#
# --proxy 安全设计:不自动回退到代理——第三方代理站可同时替换包与校验和,
# 自动切换等于把安装链路交给不可信方。仅显式传入时代理 api/下载请求,
# 且下载后强制 sha256 校验:校验和优先直连获取,取不到才走代理,校验不过
# 一律中止(宁可不装)。代理站仍可能连同校验和一起替换,务必只使用可信代理。
#
# 资产命名兼容:cargo-dist 新命名(planet-port-<target>.tar.xz)404 时,
# 回退尝试旧命名 planet-port_<ver>_<platform>_<arch>.tar.gz(如 v0.3.0
# 手工 release.sh 产物,顶层直放布局,解包逻辑天然兼容)

set -euo pipefail

REPO="planet-port/planet-port-desktop"

die() { echo "错误: $*" >&2; exit 1; }

# ── 平台识别(产物命名: cargo-dist 规范 planet-port-<target>.tar.xz) ──
OS="$(uname -s)"
case "$OS" in
    Darwin) PLATFORM="macos" ;;
    Linux)  PLATFORM="linux" ;;
    *) PLATFORM="unsupported" ;;
esac
case "${PLATFORM}_$(uname -m)" in
    macos_arm64|macos_aarch64)  TRIPLE="aarch64-apple-darwin" ;;
    macos_x86_64)               TRIPLE="x86_64-apple-darwin" ;;
    linux_x86_64|linux_amd64)   TRIPLE="x86_64-unknown-linux-gnu" ;;
    linux_aarch64|linux_arm64)  TRIPLE="aarch64-unknown-linux-gnu" ;;
    *) TRIPLE="" ;;
esac

if [[ -z "$TRIPLE" ]]; then
    die "当前平台(${OS} $(uname -m))的版本尚未发布,已支持: macOS(arm64/x86_64)、Linux(amd64/arm64,glibc≥2.28);Windows 见 docs/install.md"
fi

if [[ "$(id -u)" -ne 0 ]]; then
    # 沙箱豁免:PLANET_PORT_PREFIX 指定落盘根目录时,落盘无需 root,跳过提权直接继续
    if [[ -n "${PLANET_PORT_PREFIX:-}" ]]; then
        :
    else
        exec sudo bash "$0" "$@"
    fi
fi

# 落盘根目录;PLANET_PORT_PREFIX 仅供沙箱测试,正常安装不设
if [[ "$PLATFORM" == "macos" ]]; then
    APP_BIN="${PLANET_PORT_PREFIX:-/Library/Application Support/planet-portd}/bin"
    LINK="${PLANET_PORT_PREFIX:-/usr/local/bin}/planet-port"
else
    APP_BIN="${PLANET_PORT_PREFIX:-/usr/local}/bin"
    LINK="${PLANET_PORT_PREFIX:-/usr/local}/bin/planet-port"
fi

# ── 参数 ──────────────────────────────────────────────────────────────
NO_SERVICE=0
for arg in "$@"; do
    case "$arg" in
        --no-service) NO_SERVICE=1 ;;
        --proxy=*) export PLANET_PORT_PROXY="${arg#--proxy=}" ;;
        v[0-9]*|[0-9]*) export PLANET_PORT_VERSION="$arg" ;;
        *) die "未知参数: $arg" ;;
    esac
done

# 代理前缀拼接(空代理 = 原样直连)
dl_url() { echo "${PLANET_PORT_PROXY:-}${1}"; }

# sha256 摘要(Linux: sha256sum;macOS: shasum)
sha256_of() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null; then shasum -a 256 "$1" | awk '{print $1}'
    else die "缺少 sha256sum/shasum,无法校验下载完整性"; fi
}

# 带重试的 curl(公共代理镜像常被瞬时重置;curl --retry 不覆盖 connection
# reset,--retry-all-errors 需 7.71+ 而 Ubuntu 20.04 仍是 7.68,只能 shell 层重试)
fetch() { # fetch <尝试次数> <curl 参数...>
    local tries=$1 n=0; shift
    while (( ++n <= tries )); do
        curl "$@" && return 0
        (( n < tries )) && { echo "  第 ${n}/${tries} 次请求失败,2s 后重试..." >&2; sleep 2; }
    done
    return 1
}

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
        TAG="$(fetch 3 -fsSL "$(dl_url "https://api.github.com/repos/${REPO}/releases/latest")" \
            | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
        [[ -n "$TAG" ]] || die "获取最新版本失败(仓库可能还没有 Release;GitHub 直连不通时可使用 --proxy 加代理站点下载)"
    fi
    VERSION="${TAG#v}"
    ASSET="planet-port-${TRIPLE}.tar.xz"
    URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
    # 旧命名回退:triple → 手工发版时代的 platform_arch(仅用于 404 后重试)
    case "$TRIPLE" in
        aarch64-apple-darwin)      OLD_ASSET="planet-port_${VERSION}_macos_aarch64.tar.gz" ;;
        x86_64-apple-darwin)       OLD_ASSET="planet-port_${VERSION}_macos_x86_64.tar.gz" ;;
        x86_64-unknown-linux-gnu)  OLD_ASSET="planet-port_${VERSION}_linux_amd64.tar.gz" ;;
        aarch64-unknown-linux-gnu) OLD_ASSET="planet-port_${VERSION}_linux_arm64.tar.gz" ;;
        *)                         OLD_ASSET="" ;;
    esac

    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
    TARBALL="${TMP}/${ASSET}"
    USED_URL="$URL"
    echo "下载 ${URL}${PLANET_PORT_PROXY:+(经代理 ${PLANET_PORT_PROXY})}"
    # curl -f:HTTP ≥400 直接失败且不落错误页,据此判定 404 并回退旧命名;
    # 不加 -s(静默会压掉 --progress-bar,用户看不到下载进度)
    if ! fetch 3 -fL --connect-timeout 15 --progress-bar -o "$TARBALL" "$(dl_url "$URL")"; then
        [[ -n "$OLD_ASSET" ]] || die "下载失败(确认 Release 已附带 ${ASSET};GitHub 直连不通时可使用 --proxy 加代理站点下载)"
        OLD_URL="https://github.com/${REPO}/releases/download/${TAG}/${OLD_ASSET}"
        echo "未找到 ${ASSET},回退尝试旧命名 ${OLD_ASSET}"
        # 旧命名是 gzip 内容,落盘文件名必须同步换成 .tar.gz,
        # 否则解包按扩展名误判走 -xJf(GNU tar 直接报 xz 格式错误)
        TARBALL="${TMP}/${OLD_ASSET}"
        USED_URL="$OLD_URL"
        fetch 3 -fL --connect-timeout 15 --progress-bar -o "$TARBALL" "$(dl_url "$OLD_URL")" \
            || die "下载失败(确认 Release 已附带 ${ASSET} 或 ${OLD_ASSET};GitHub 直连不通时可使用 --proxy 加代理站点下载)"
    fi
    [[ -s "$TARBALL" ]] || die "下载内容为空(确认 Release 已附带 ${ASSET})"
    # 完整性校验:校验和优先直连获取(直连能取到就不经代理),取不到再走
    # 代理;校验不过一律中止。经由代理安装但拿不到校验和时也中止——宁可不装。
    SHA_FILE="${TARBALL}.sha256"
    fetch 2 -fsSL --connect-timeout 15 -o "$SHA_FILE" "${USED_URL}.sha256" \
        || fetch 2 -fsSL --connect-timeout 15 -o "$SHA_FILE" "$(dl_url "${USED_URL}.sha256")" || true
    if [[ -s "$SHA_FILE" ]]; then
        WANT="$(awk '{print $1}' "$SHA_FILE")"
        GOT="$(sha256_of "$TARBALL")"
        [[ "$WANT" == "$GOT" ]] \
            || die "sha256 校验失败,包可能被篡改或损坏(期望 $WANT,实际 $GOT)"
        echo "sha256 校验通过"
    elif [[ -n "${PLANET_PORT_PROXY:-}" ]]; then
        die "无法获取 .sha256 校验和(经代理安装必须可校验,已中止;可改用 PLANET_PORT_TARBALL 本地包安装)"
    else
        echo "警告: 无法获取 .sha256,跳过完整性校验(旧版 Release 可能未附带)"
    fi
fi

# ── 解包并安装 ─────────────────────────────────────────────────────────
# 一键安装(二进制落盘 + PATH + launchd/systemd 服务)由 `planet-port install`
# 自带,含升级兜底(记录 running planet、装完后拉回);本脚本只负责
# 「下载 + 解压 + 调起」,不再重复实现拷贝/软链/恢复逻辑
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE" ${TMP:-}' EXIT
# 新命名 .tar.xz / 旧命名 .tar.gz 两种压缩都要能解(按扩展名选参数,避免依赖 tar 自动识别)
case "$TARBALL" in
    *.tar.gz|*.tgz) TAR_FLAG="-xzf" ;;
    *)              TAR_FLAG="-xJf" ;;
esac
tar $TAR_FLAG "$TARBALL" -C "$STAGE"
# dist 归档可能带顶层目录(planet-port-<target>/),也可能顶层直放,两者都兼容
BIN_DIR="$STAGE"
if [[ ! -x "$STAGE/planet-port" ]]; then
    BIN_DIR="$(find "$STAGE" -maxdepth 2 -type f -name planet-port -perm -u+x -exec dirname {} \; | head -1)"
    [[ -n "$BIN_DIR" ]] || die "包内容不完整(需含可执行的 planet-port 与 planet-portd)"
fi
[[ -x "$BIN_DIR/planet-port" && -x "$BIN_DIR/planet-portd" ]] \
    || die "包内容不完整(需含可执行的 planet-port 与 planet-portd)"

if [[ "$NO_SERVICE" -eq 1 ]]; then
    # 仅落盘二进制,不装服务(软链仅 macOS 需要;Linux 直接落 /usr/local/bin)
    mkdir -p "$APP_BIN"
    for bin in planet-port planet-portd; do
        install -m 755 "${BIN_DIR}/${bin}" "${APP_BIN}/${bin}.new"
        mv -f "${APP_BIN}/${bin}.new" "${APP_BIN}/${bin}"
    done
    if [[ "$PLATFORM" == "macos" ]]; then
        mkdir -p "$(dirname "$LINK")"
        ln -sfn "$APP_BIN/planet-port" "$LINK"
    fi
    echo "已跳过服务安装(--no-service),需要时执行: sudo planet-port install"
else
    "${BIN_DIR}/planet-port" install
fi
rm -rf "$STAGE"

echo
echo "安装完成: ${VERSION} (${TRIPLE})"
echo "  CLI:      ${LINK}"
echo "  二进制:   ${APP_BIN}/"
if [[ "$PLATFORM" == "macos" ]]; then
    echo "  卸载:     sudo ${LINK} service uninstall && sudo rm ${LINK}"
else
    echo "  服务:     /etc/systemd/system/planet-portd.service"
    echo "  卸载:     sudo ${LINK} service uninstall && sudo rm -f /usr/local/bin/planet-port /usr/local/bin/planet-portd"
fi
