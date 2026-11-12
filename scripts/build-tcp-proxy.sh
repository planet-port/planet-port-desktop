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

case "$TARGET" in
  macos-native)
    mkdir -p "$OUT_DIR"
    c++ -O2 -std=c++11 "${INC[@]}" -o "$OUT_DIR/tcp-proxy" "$CPP" "$METRICS" -lpthread
    OUT="$OUT_DIR/tcp-proxy"
    ;;
  x86_64-linux-gnu|aarch64-linux-gnu)
    command -v zig >/dev/null 2>&1 || { echo "需要 zig: brew install zig" >&2; exit 1; }
    mkdir -p "$OUT_DIR"
    zig c++ -target "$TARGET" -O2 -std=c++11 "${INC[@]}" -o "$OUT_DIR/tcp-proxy-$TARGET" "$CPP" "$METRICS" -lpthread
    OUT="$OUT_DIR/tcp-proxy-$TARGET"
    ;;
  *)
    echo "未知 target: $TARGET(可选 macos-native | x86_64-linux-gnu | aarch64-linux-gnu)" >&2
    exit 1
    ;;
esac

echo "产物: $OUT"
echo "许可: Apache-2.0(ZeroTier BSL 1.1 已过 Change Date 2025-01-01,自动转 Change License),分发须附来源: https://github.com/zerotier/ZeroTierOne"
