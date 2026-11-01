#!/bin/bash
# planet-port 发版打包脚本
#
# 用法: ./scripts/release.sh vX.Y.Z
#
# 前置(按仓库发版模型):
#   main ← merge --ff-only dev → git tag vX.Y.Z → git push origin main vX.Y.Z
#   然后在 main 检出状态下执行本脚本。
#
# 行为:
#   1. 校验当前 HEAD 即 tag 指向的提交(防止用 dev 代码发 main 的版)
#   2. 五目标构建(macOS×2 + Linux×2 + Windows×1)
#   3. 按 planet-port_<ver>_<platform>_<arch> 规范打包(unix 走 tar.gz,
#      Windows 走 zip,内含双 exe + wintun.dll + install-offline.ps1)
#   4. gh release create 挂资产(已存在则只补传缺失资产,幂等)

set -euo pipefail

REPO="planet-port/planet-port-desktop"
# 目标三元组;linux 目标带 .2.31 后缀(cargo-zigbuild 的 glibc 基线标记)
TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin" \
         "x86_64-unknown-linux-gnu.2.31" "aarch64-unknown-linux-gnu.2.31" \
         "x86_64-pc-windows-msvc")
# target 三元组 → 产物命名里的平台架构标记
arch_tag() {
    case "$1" in
        aarch64-apple-darwin)            echo "macos_aarch64" ;;
        x86_64-apple-darwin)              echo "macos_x86_64" ;;
        x86_64-unknown-linux-gnu.2.31)    echo "linux_amd64" ;;
        aarch64-unknown-linux-gnu.2.31)   echo "linux_arm64" ;;
        x86_64-pc-windows-msvc)           echo "windows_x86_64" ;;
        *) echo "" ;;
    esac
}
build_target() { # $1=target
    case "$1" in
        *-linux-*) cargo zigbuild --release --target "$1" -p planet-port -p daemon ;;
        x86_64-pc-windows-msvc)
            # Task 1 探针结论:xwin(msvc STL 头 + lld-link)在本机可链接;
            # llvm 的 clang/lld-link 不在默认 PATH,须显式前置
            export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
            cargo xwin build --release --target "$1" -p planet-port -p daemon ;;
        *)         cargo build --release --target "$1" -p planet-port -p daemon ;;
    esac
}
# windows 产物打包:双 exe + wintun.dll(SHA256 校验) + 离线安装脚本 → zip
pkg_windows() { # $1=BIN_DIR $2=输出 zip 路径
    local bin_dir="$1" out="$2"
    [[ -f "${bin_dir}/planet-port.exe" && -f "${bin_dir}/planet-portd.exe" ]] \
        || die "${bin_dir} 下 Windows 二进制缺失"
    local want="e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce"
    local got
    got="$(shasum -a 256 third_party/wintun/wintun.dll | awk '{print $1}')"
    [[ "$got" == "$want" ]] \
        || die "wintun.dll SHA256 不符(期望 ${want:0:16}…,实际 ${got:0:16}…),拒绝打包"
    local stage="${STAGE}/win-pkg"
    rm -rf "$stage"; mkdir -p "$stage"
    cp "${bin_dir}/planet-port.exe" "${bin_dir}/planet-portd.exe" "$stage/"
    cp third_party/wintun/wintun.dll "$stage/"
    cp scripts/install-offline.ps1 "$stage/"
    (cd "$stage" && zip -q -r "$out" .)
    rm -rf "$stage"
}

die() { echo "错误: $*" >&2; exit 1; }

TAG="${1:-}"
# 支持 vX.Y.Z 与 vX.Y.Z-beta.N(beta 版会以 GitHub prerelease 形式发布,
# 不会被 /releases/latest 解析到——install.sh 不带版本号时装不到 beta,
# 用户需显式指定版本,符合 beta 的按需试用语义)
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?$ ]] || die "用法: $0 vX.Y.Z [vX.Y.Z-beta.N]"
VERSION="${TAG#v}"
PRERELEASE=()
[[ "$TAG" =~ -beta\. ]] && PRERELEASE=(--prerelease)

command -v gh >/dev/null || die "缺少 gh(CLI 登录态用于创建 Release)"

# HEAD 必须就是 tag 提交:保证制品与 tag 严格一致
TAG_COMMIT="$(git rev-parse "${TAG}^{commit}")"
HEAD_COMMIT="$(git rev-parse HEAD)"
[[ "$TAG_COMMIT" == "$HEAD_COMMIT" ]] || die "当前 HEAD(${HEAD_COMMIT:0:7})不是 ${TAG} 指向的提交(${TAG_COMMIT:0:7});先 checkout main 并确认已合并"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

for target in "${TARGETS[@]}"; do
    tag="$(arch_tag "$target")"
    [[ -n "$tag" ]] || die "未配置 ${target} 的产物命名"
    echo "构建 ${target} ..."
    build_target "$target"
    # zigbuild 产物目录是不带 glibc 后缀的三元组;%%.* 删最长后缀,去掉整个 .2.31
    BIN_DIR="target/${target%%.*}/release"
    if [[ "$target" == "x86_64-pc-windows-msvc" ]]; then
        ASSET="planet-port_${VERSION}_${tag}.zip"
        pkg_windows "$BIN_DIR" "${STAGE}/${ASSET}"
    else
        [[ -x "${BIN_DIR}/planet-port" && -x "${BIN_DIR}/planet-portd" ]] \
            || die "${BIN_DIR} 下二进制缺失"
        ASSET="planet-port_${VERSION}_${tag}.tar.gz"
        tar -czf "${STAGE}/${ASSET}" -C "$BIN_DIR" planet-port planet-portd
    fi
    echo "打包 ${ASSET}: $(du -h "${STAGE}/${ASSET}" | cut -f1)"
done

NOTES="${STAGE}/notes.md"
cat > "$NOTES" <<EOF
## 产物

| 资产 | 平台 |
|---|---|
| planet-port_${VERSION}_macos_aarch64.tar.gz | macOS Apple Silicon |
| planet-port_${VERSION}_macos_x86_64.tar.gz | macOS Intel |
| planet-port_${VERSION}_linux_amd64.tar.gz | Linux x86_64 (glibc≥2.31) |
| planet-port_${VERSION}_linux_arm64.tar.gz | Linux arm64 (glibc≥2.31) |
| planet-port_${VERSION}_windows_x86_64.zip | Windows x86_64 |

## 安装

\`\`\`bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | sudo bash
\`\`\`

\`\`\`powershell
# Windows:下载 zip 解压后,在管理员 PowerShell 中
cd <解压目录>
powershell -ExecutionPolicy Bypass -File .\install-offline.ps1
\`\`\`
EOF

# Release 已存在则补传缺失资产(幂等),否则创建
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    echo "Release ${TAG} 已存在,补传缺失资产"
    ASSETS=()
    for target in "${TARGETS[@]}"; do
        ext="tar.gz"; [[ "$target" == "x86_64-pc-windows-msvc" ]] && ext="zip"
        f="${STAGE}/planet-port_${VERSION}_$(arch_tag "$target").${ext}"
        name="$(basename "$f")"
        gh release view "$TAG" --repo "$REPO" --json assets --jq ".assets[].name" \
            | grep -qx "$name" || ASSETS+=("$f")
    done
    [[ ${#ASSETS[@]} -gt 0 ]] && gh release upload "$TAG" --repo "$REPO" "${ASSETS[@]}"
else
    gh release create "$TAG" --repo "$REPO" --title "$TAG" "${PRERELEASE[@]}" --notes-file "$NOTES" \
        "${STAGE}"/planet-port_${VERSION}_*.tar.gz "${STAGE}"/planet-port_${VERSION}_*.zip
fi
echo "完成: https://github.com/${REPO}/releases/tag/${TAG}"
