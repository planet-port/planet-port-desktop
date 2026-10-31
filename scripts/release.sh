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
#   2. 四目标构建(macOS×2 + Linux×2)
#   3. 按 planet-port_<ver>_<platform>_<arch>.tar.gz 规范打包
#   4. gh release create 挂资产(已存在则只补传缺失资产,幂等)

set -euo pipefail

REPO="planet-port/planet-port-desktop"
# 目标三元组;linux 目标带 .2.31 后缀(cargo-zigbuild 的 glibc 基线标记)
TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin" \
         "x86_64-unknown-linux-gnu.2.31" "aarch64-unknown-linux-gnu.2.31")
# target 三元组 → 产物命名里的平台架构标记
arch_tag() {
    case "$1" in
        aarch64-apple-darwin)            echo "macos_aarch64" ;;
        x86_64-apple-darwin)              echo "macos_x86_64" ;;
        x86_64-unknown-linux-gnu.2.31)    echo "linux_amd64" ;;
        aarch64-unknown-linux-gnu.2.31)   echo "linux_arm64" ;;
        *) echo "" ;;
    esac
}
build_target() { # $1=target
    case "$1" in
        *-linux-*) cargo zigbuild --release --target "$1" -p planet-port -p daemon ;;
        *)         cargo build --release --target "$1" -p planet-port -p daemon ;;
    esac
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
    [[ -x "${BIN_DIR}/planet-port" && -x "${BIN_DIR}/planet-portd" ]] \
        || die "${BIN_DIR} 下二进制缺失"
    ASSET="planet-port_${VERSION}_${tag}.tar.gz"
    tar -czf "${STAGE}/${ASSET}" -C "$BIN_DIR" planet-port planet-portd
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

## 安装

\`\`\`bash
curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | sudo bash
\`\`\`
EOF

# Release 已存在则补传缺失资产(幂等),否则创建
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    echo "Release ${TAG} 已存在,补传缺失资产"
    ASSETS=()
    for target in "${TARGETS[@]}"; do
        f="${STAGE}/planet-port_${VERSION}_$(arch_tag "$target").tar.gz"
        name="$(basename "$f")"
        gh release view "$TAG" --repo "$REPO" --json assets --jq ".assets[].name" \
            | grep -qx "$name" || ASSETS+=("$f")
    done
    [[ ${#ASSETS[@]} -gt 0 ]] && gh release upload "$TAG" --repo "$REPO" "${ASSETS[@]}"
else
    gh release create "$TAG" --repo "$REPO" --title "$TAG" "${PRERELEASE[@]}" --notes-file "$NOTES" \
        "${STAGE}"/planet-port_${VERSION}_*.tar.gz
fi
echo "完成: https://github.com/${REPO}/releases/tag/${TAG}"
