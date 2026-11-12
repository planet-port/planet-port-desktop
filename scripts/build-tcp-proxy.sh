#!/bin/bash
# 编译官方 tcp-proxy(ZeroTier vendor 树 1.12.0-dev,Apache-2.0)——TCP 兜底 relay 服务端。
# 源码取自本机 vendor 的 ZeroTier 完整仓库(zt-core 用的裁剪 Core/ 无
# service/tcp-proxy);产物用于部署到 planet 根服务器,见
# docs/deploy-tcp-proxy.md。
#
# 注:1.14 的官方 Makefile 已无 tcp-proxy 构建规则,故比"单文件直编"多两件
# 官方 zerotier-one 同款依赖(口径取自 vendor 自带的 tcp-proxy/Makefile):
#   1) prometheus-cpp-lite 头文件目录(Metrics.hpp 顶层 include,header-only)
#   2) node/Metrics.cpp(tcp-proxy.cpp 引用 Metrics::udp_send,定义在此)
#
# 用法:
#   ./scripts/build-tcp-proxy.sh                 # macos-native,本机协议验证
#   ./scripts/build-tcp-proxy.sh x86_64-linux-gnu   # zig 交叉(gozi25 部署)
#   ./scripts/build-tcp-proxy.sh aarch64-linux-gnu
#   ZT_ONE_SRC=/path/to/ZeroTierOne ./scripts/build-tcp-proxy.sh ...  # 源码覆盖
#   TCP_PROXY_PORT=8443 ./scripts/build-tcp-proxy.sh x86_64-linux-gnu  # 自定义监听口
#
# 监听口:官方源码宏写死 443 且二进制无命令行参数;本脚本默认改为 9443
# (避开常见 443 占用),由 TCP_PROXY_PORT 环境变量覆盖。非 443 口拷源码
# 副本替换宏后编译(不动 vendor 树);默认 9443 产物名无后缀,自定义口带
# -p<port> 后缀,443 为 vendor 原文件直编。
set -euo pipefail

TARGET="${1:-macos-native}"
SRC="${ZT_ONE_SRC:-/Users/zivyuan/GoziLib/Zerotier/libzt/ext/ZeroTierOne}"
CPP="$SRC/tcp-proxy/tcp-proxy.cpp"
METRICS="$SRC/node/Metrics.cpp"
INC=( -isystem "$SRC/ext/prometheus-cpp-lite-1.0/core/include" -isystem "$SRC/ext/prometheus-cpp-lite-1.0/simpleapi/include" )

for f in "$CPP" "$METRICS"; do
  [ -f "$f" ] || { echo "源码不存在: $f(用 ZT_ONE_SRC 覆盖路径)" >&2; exit 1; }
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/dist-tcp-proxy"
PORT="${TCP_PROXY_PORT:-9443}"
PORT_TAG=""
if [ "$PORT" != "443" ]; then
  case "$PORT" in
    ''|*[!0-9]*) echo "TCP_PROXY_PORT 须为数字: $PORT" >&2; exit 1 ;;
  esac
  mkdir -p "$ROOT/tmp"
  CPP="$ROOT/tmp/tcp-proxy-p$PORT.cpp"
  sed "s/^#define ZT_TCP_PROXY_TCP_PORT 443\$/#define ZT_TCP_PROXY_TCP_PORT $PORT/" \
    "$SRC/tcp-proxy/tcp-proxy.cpp" > "$CPP"
  # 替换必须真实生效,否则产物仍是 443(静默错误最难排查)
  grep -q "^#define ZT_TCP_PROXY_TCP_PORT $PORT\$" "$CPP" \
    || { echo "宏替换失败:vendor 源码变动?检查 tcp-proxy.cpp 的 ZT_TCP_PROXY_TCP_PORT 定义" >&2; exit 1; }
  # 副本不在 vendor 树内,源码的相对 include(../osdep 等)需指回原目录解析
  INC+=( -I "$SRC/tcp-proxy" )
fi
# 仅默认口不带后缀;显式 443 也带 -p443,避免与默认产物同名互相覆盖
[ "$PORT" != "9443" ] && PORT_TAG="-p$PORT"

case "$TARGET" in
  macos-native)
    mkdir -p "$OUT_DIR"
    c++ -O2 -std=c++11 "${INC[@]}" -o "$OUT_DIR/tcp-proxy$PORT_TAG" "$CPP" "$METRICS" -lpthread
    OUT="$OUT_DIR/tcp-proxy$PORT_TAG"
    ;;
  x86_64-linux-gnu|aarch64-linux-gnu)
    command -v zig >/dev/null 2>&1 || { echo "需要 zig: brew install zig" >&2; exit 1; }
    mkdir -p "$OUT_DIR"
    zig c++ -target "$TARGET" -O2 -std=c++11 "${INC[@]}" -o "$OUT_DIR/tcp-proxy-$TARGET$PORT_TAG" "$CPP" "$METRICS" -lpthread
    OUT="$OUT_DIR/tcp-proxy-$TARGET$PORT_TAG"
    ;;
  *)
    echo "未知 target: $TARGET(可选 macos-native | x86_64-linux-gnu | aarch64-linux-gnu)" >&2
    exit 1
    ;;
esac

echo "产物: $OUT"
echo "许可: Apache-2.0(ZeroTier BSL 1.1 已过 Change Date 2025-01-01,自动转 Change License),分发须附来源: https://github.com/zerotier/ZeroTierOne"
