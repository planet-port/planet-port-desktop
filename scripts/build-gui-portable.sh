#!/usr/bin/env bash
# 组装 GUI 免安装形态:daemon/cli 构建并内嵌 resources → tauri build → 平台包
#
# 用法: scripts/build-gui-portable.sh [target-triple]
#   交叉构建传 triple(与 cargo --target 同款);也兼容 release.yml 的 TARGET 环境变量
#   (位置参数优先于环境变量,两者皆空时取 rustc host triple)
#
# 产物:
#   mac/linux   → target[/triple]/release/bundle/{dmg,appimage}(bundle 内嵌 resources/<triple>/)
#   windows     → target/release/bundle/planet-port-<ver>-x64-portable.zip
#                 (tauri 2 无 zip bundle target,以 no-bundle 产物自组,命名不可变)
#
# resources 布局(消费端 crates/gui/src-tauri/src/bootstrap.rs):
#   resources/<triple>/{planet-portd[, .exe], planet-port[, .exe][, wintun.dll], manifest.json}
#   manifest.json 精确格式 {"version":"X.Y.Z","target":"<triple>"}
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GUI="$ROOT/crates/gui"
HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
TRIPLE="${1:-${TARGET:-}}"
[ -n "$TRIPLE" ] || TRIPLE="$HOST"

case "$TRIPLE" in *windows*) WIN_TARGET=1 ;; *) WIN_TARGET=0 ;; esac
RES="$GUI/src-tauri/resources/$TRIPLE"

# 0. ZeroTier 核心源码(幂等,已就绪即跳过);daemon/cli 经 zt-core 依赖它
bash "$ROOT/scripts/bootstrap.sh"

# 1. 本平台或交叉目标的 daemon/cli;GUI 随后由 tauri 按同一 target 构建
if [ "$TRIPLE" = "$HOST" ]; then
  cargo build --release -p planet-port --bins
  BIN="$ROOT/target/release"
else
  rustup target add "$TRIPLE"
  cargo build --release -p planet-port --bins --target "$TRIPLE"
  BIN="$ROOT/target/$TRIPLE/release"
fi

# 2. resources 组装:目录整体重建,保证无上一轮残留
rm -rf "$RES"
mkdir -p "$RES"

# 版本取 workspace.package(发版脚本会以正式版本号覆盖该行,dev 构建永不与正式版同号)
VERSION="$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0} f && /^version[[:space:]]*=/{gsub(/"/, "", $3); print $3; exit}' "$ROOT/Cargo.toml")"
[ -n "$VERSION" ] || { echo "无法从 workspace Cargo.toml 的 [workspace.package] 提取 version" >&2; exit 1; }

if [ "$WIN_TARGET" = 1 ]; then
  cp "$BIN/planet-portd.exe" "$BIN/planet-port.exe" "$RES/"
  # wintun 随便携包分发;SHA256 与 dist 管线同款口径,不符即中止
  cp "$ROOT/third_party/wintun/wintun.dll" "$RES/"
  want="e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce"
  got="$(sha256sum "$RES/wintun.dll" | awk '{print $1}')"
  [ "$got" = "$want" ] || { echo "wintun.dll SHA256 不符(实际 ${got:0:16}…),拒绝组包" >&2; exit 1; }
else
  cp "$BIN/planet-portd" "$BIN/planet-port" "$RES/"
  # bundle 资源拷贝后执行位不确定,安装走提权 exec,显式补 +x
  chmod +x "$RES/planet-portd" "$RES/planet-port"
fi
printf '{"version":"%s","target":"%s"}\n' "$VERSION" "$TRIPLE" > "$RES/manifest.json"

# 3. GUI 构建:win 走 no-bundle 自组 zip,其余 bundle(dmg/appimage)。
#    tauri 2 以 CLI 调用目录(crates/gui)为 beforeBuildCommand 的 cwd
cd "$GUI"
pnpm install --frozen-lockfile
if [ "$WIN_TARGET" = 1 ]; then
  pnpm tauri build --no-bundle
  STAGE="$ROOT/target/portable-stage"
  rm -rf "$STAGE"
  mkdir -p "$STAGE/resources"
  cp "$ROOT/target/release/planet-port-gui.exe" "$STAGE/"
  # 只收当前 triple,避免把 host 上其它目标的资源带进便携包
  cp -R "$RES" "$STAGE/resources/"
  (cd "$STAGE" && powershell -NoProfile -Command "Compress-Archive -Path * -DestinationPath 'planet-port-${VERSION}-x64-portable.zip' -Force")
  mkdir -p "$ROOT/target/release/bundle"
  mv "$STAGE/planet-port-${VERSION}-x64-portable.zip" "$ROOT/target/release/bundle/"
  rm -rf "$STAGE"
elif [ "$TRIPLE" = "$HOST" ]; then
  pnpm tauri build
else
  # 交叉构建:GUI 与 daemon/cli 同 target,bundle 落 target/<triple>/release/bundle
  pnpm tauri build --target "$TRIPLE"
fi

echo "portable build done: $TRIPLE v$VERSION"
