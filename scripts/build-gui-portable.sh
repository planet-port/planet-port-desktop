#!/usr/bin/env bash
# 组装 GUI 免安装形态:内嵌 daemon/cli release 资产到 resources → tauri build → 平台包
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
#   manifest.json 精确格式 {"daemon":"X.Y.Z","cli":"X.Y.Z","target":"<triple>"}
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# 锚定仓库根:cargo(靠 cwd 找 workspace)与 third_party/wintun 等相对路径都依赖调用目录,
# 跨目录绝对路径调用时若不切根,相对路径会落到错误位置
cd "$ROOT"
GUI="$ROOT/crates/gui"
HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
TRIPLE="${1:-${TARGET:-}}"
[ -n "$TRIPLE" ] || TRIPLE="$HOST"

case "$TRIPLE" in *windows*) WIN_TARGET=1 ;; *) WIN_TARGET=0 ;; esac
RES="$GUI/src-tauri/resources/$TRIPLE"

# 0. 内嵌二进制来源:默认取各自最新稳定 release 资产(daemon/cli 独立发版
#    后,GUI 只内嵌已发布版本,永不内嵌未发布的本地构建,保证版本如实);
#    PLANET_LOCAL_BINS=1 时回退本地构建(开发调试/迁移期无 release 可拉)。
#    GUI 自身不依赖 zt-core(仅 planet-port-api),无需 bootstrap ZT 源码。
REPO="planet-port/planet-port-desktop"

resolve_latest_tag() { # $1=tag前缀(如 planet-portd/v) $2=allow_prerelease(true/false)
  gh api "repos/${REPO}/releases?per_page=50" --jq \
    "[.[] | select(.tag_name | startswith(\"$1\"))] |
     (if \"$2\" == \"true\" then .[0] else ([.[] | select(.draft == false) | select(.prerelease == false)][0]) end) |
     .tag_name // empty"
}

fetch_assets() { # $1=tag $2=triple $3=bin名(无扩展) → 解出二进制到 $RES。
  # find 定位不依赖归档内部布局(顶层目录有无均可)
  gh release download "$1" --repo "$REPO" \
    --pattern "planet-port*-${2}.tar.xz" --pattern "planet-port*-${2}.zip" \
    --dir "$RES.dl" --clobber
  # gh release download 只落盘不解包;pattern 可同时命中两种归档,须全部就地展开
  local archive
  for archive in "$RES.dl"/*.tar.xz "$RES.dl"/*.zip; do
    [ -e "$archive" ] || continue
    case "$archive" in
      *.tar.xz) tar -xJf "$archive" -C "$RES.dl" ;;
      *.zip) unzip -oq "$archive" -d "$RES.dl" ;;
    esac
  done
  local bin
  # -type f 排除与二进制同名的顶层目录(否则 find 先命中目录,mv 会搬走整个目录);
  # -print -quit 取首个命中,避免管道 + pipefail 下 head 提前退出误伤
  bin="$(find "$RES.dl" -type f \( -name "$3" -o -name "$3.exe" \) -print -quit)"
  [ -n "$bin" ] || { echo "资产中未找到 $3（$1 $2）" >&2; exit 1; }
  mv "$bin" "$RES/"
  chmod +x "$RES/$3" "$RES/$3.exe" 2>/dev/null || true
  rm -rf "$RES.dl"
}

# resources 组装:目录整体重建,保证无上一轮残留(本地拷贝与 release 解包都落满目录)
rm -rf "$RES"
mkdir -p "$RES"
if [ "${PLANET_LOCAL_BINS:-}" = "1" ]; then
  if [ "$TRIPLE" = "$HOST" ]; then
    cargo build --release -p planet-port -p planet-portd --bins
    BIN="$ROOT/target/release"
  else
    rustup target add "$TRIPLE"
    cargo build --release -p planet-port -p planet-portd --bins --target "$TRIPLE"
    BIN="$ROOT/target/$TRIPLE/release"
  fi
  DAEMON_VER="$(awk '/^version = /{gsub(/"/, "", $3); print $3; exit}' "$ROOT/crates/daemon/Cargo.toml")"
  CLI_VER="$(awk '/^version = /{gsub(/"/, "", $3); print $3; exit}' "$ROOT/crates/cli/Cargo.toml")"
  [ -n "$DAEMON_VER" ] || { echo "无法提取 daemon 版本" >&2; exit 1; }
  [ -n "$CLI_VER" ] || { echo "无法提取 cli 版本" >&2; exit 1; }
  if [ "$WIN_TARGET" = 1 ]; then
    cp "$BIN/planet-portd.exe" "$BIN/planet-port.exe" "$RES/"
  else
    cp "$BIN/planet-portd" "$BIN/planet-port" "$RES/"
    chmod +x "$RES/planet-portd" "$RES/planet-port"
  fi
else
  command -v gh >/dev/null || { echo "缺 gh CLI(拉取 release 资产需登录态)" >&2; exit 1; }
  # 本次构建为 beta 发版(ref 带 -beta.)时放宽到最新含 prerelease,与
  # suite job 口径一致——首个稳定版发布前 beta 链路否则无资产可嵌;
  # 正式版与本地构建始终取最新稳定
  ALLOW=false
  case "${GITHUB_REF_NAME:-}" in *-beta.*) ALLOW=true ;; esac
  DAEMON_TAG="$(resolve_latest_tag 'planet-portd/v' "$ALLOW")"
  CLI_TAG="$(resolve_latest_tag 'planet-port/v' "$ALLOW")"
  [ -n "$DAEMON_TAG" ] || { echo "未找到 planet-portd release,先发 daemon" >&2; exit 1; }
  [ -n "$CLI_TAG" ] || { echo "未找到 planet-port release,先发 cli" >&2; exit 1; }
  fetch_assets "$DAEMON_TAG" "$TRIPLE" planet-portd
  fetch_assets "$CLI_TAG" "$TRIPLE" planet-port
  DAEMON_VER="${DAEMON_TAG#planet-portd/v}"
  CLI_VER="${CLI_TAG#planet-port/v}"
fi

# 2. 版本与平台杂项:manifest 记 daemon/cli 各自版本;wintun 随 win 包分发
# GUI 自身版本:便携 zip 命名用(发版时三源已由 release.sh 同步)
GUI_VER="$(awk '/^version = /{gsub(/"/, "", $3); print $3; exit}' "$ROOT/crates/gui/src-tauri/Cargo.toml")"
[ -n "$GUI_VER" ] || { echo "无法提取 gui 版本" >&2; exit 1; }

if [ "$WIN_TARGET" = 1 ]; then
  # wintun 随便携包分发;SHA256 与 dist 管线同款口径,不符即中止
  cp "$ROOT/third_party/wintun/wintun.dll" "$RES/"
  want="e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce"
  # SHA256 工具探测:macOS 无 sha256sum,优先 shasum,Linux/CI 用 sha256sum(与 bootstrap.sh 同款口径)
  SHA256_TOOL=(shasum -a 256)
  command -v shasum >/dev/null 2>&1 || SHA256_TOOL=(sha256sum)
  got="$("${SHA256_TOOL[@]}" "$RES/wintun.dll" | awk '{print $1}')"
  [ "$got" = "$want" ] || { echo "wintun.dll SHA256 不符(实际 ${got:0:16}…),拒绝组包" >&2; exit 1; }
else
  # bundle 资源拷贝后执行位不确定,安装走提权 exec,显式补 +x
  chmod +x "$RES/planet-portd" "$RES/planet-port"
fi
printf '{"daemon":"%s","cli":"%s","target":"%s"}\n' "$DAEMON_VER" "$CLI_VER" "$TRIPLE" > "$RES/manifest.json"

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
  (cd "$STAGE" && powershell -NoProfile -Command "Compress-Archive -Path * -DestinationPath 'planet-port-${GUI_VER}-x64-portable.zip' -Force")
  mkdir -p "$ROOT/target/release/bundle"
  mv "$STAGE/planet-port-${GUI_VER}-x64-portable.zip" "$ROOT/target/release/bundle/"
  rm -rf "$STAGE"
elif [ "$TRIPLE" = "$HOST" ]; then
  pnpm tauri build
else
  # 交叉构建:GUI 与 daemon/cli 同 target,bundle 落 target/<triple>/release/bundle
  pnpm tauri build --target "$TRIPLE"
fi

echo "portable build done: $TRIPLE daemon v$DAEMON_VER + cli v$CLI_VER"
