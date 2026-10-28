#!/usr/bin/env bash
# 拉取 ZeroTier 核心源码子集(不入库)。与 iOS/Android 端同一套模板：
# 钉 tag + SHA256 拉官方 tarball，解压裁剪到 Core/，写 .fetched-tag 版本戳。
set -euo pipefail

ZT_VERSION=1.16.2
ZT_SHA256=2c607f573c6e38815433af289d364a689a203b18b51125f06c4472014d0657f0
ZT_URL="https://codeload.github.com/zerotier/ZeroTierOne/tar.gz/${ZT_VERSION}"

CACHE_DIR="${ZT_CACHE_DIR:-$HOME/Library/Caches/PlanetPort}"
TARBALL="${CACHE_DIR}/ZeroTierOne-${ZT_VERSION}.tar.gz"
CORE_DIR=Core
STAMP="${CORE_DIR}/.fetched-tag"

err() { echo "bootstrap: $*" >&2; exit 1; }

if [[ -f "$STAMP" && "$(cat "$STAMP")" == "$ZT_VERSION" ]]; then
  echo "ZeroTier 核心 ${ZT_VERSION} 已就绪，跳过拉取"
  exit 0
fi

verify_tarball() { echo "${ZT_SHA256}  ${TARBALL}" | shasum -a 256 --check - >/dev/null 2>&1; }

mkdir -p "$CACHE_DIR"
if [[ -s "$TARBALL" ]] && ! verify_tarball; then
  rm -f "$TARBALL"; err "缓存损坏，已删除：${TARBALL}"
fi
if [[ ! -s "$TARBALL" ]]; then
  echo "下载 ${ZT_TARBALL_URL:-$ZT_URL}"
  curl -fL --retry 3 -o "$TARBALL" "${ZT_TARBALL_URL:-$ZT_URL}" || err "下载失败"
fi
verify_tarball || err "SHA256 校验失败（可能被镜像篡改或下载不完整）：${TARBALL}"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
tar -xzf "$TARBALL" -C "$TMP"
SRC="${TMP}/ZeroTierOne-${ZT_VERSION}"

# 只取 POC 所需子集：node 整目录、C API 头、OSUtils、ext 头与汇编。
# 另拷 osdep 纯头 Binder/Phy/BlockingQueue/ExtOsdep:node/Bond.hpp 与
# PacketMultiplexer.hpp 无条件 include 它们,缺了编译不过(均为模板/条件编译,
# 不需对应 .cpp,不会引入 JNI/service 层实现)。
rm -rf "$CORE_DIR"; mkdir -p "$CORE_DIR"
cp -R "$SRC/node" "$CORE_DIR/"
mkdir -p "$CORE_DIR/include" "$CORE_DIR/osdep"
cp "$SRC/include/ZeroTierOne.h" "$SRC/include/ZeroTierDebug.h" "$CORE_DIR/include/"
cp "$SRC/osdep/OSUtils.cpp" "$SRC/osdep/OSUtils.hpp" "$CORE_DIR/osdep/"
cp "$SRC/osdep/Binder.hpp" "$SRC/osdep/Phy.hpp" \
  "$SRC/osdep/BlockingQueue.hpp" "$SRC/osdep/ExtOsdep.hpp" "$CORE_DIR/osdep/"
cp "$SRC/version.h" "$CORE_DIR/"
# 上游许可全文随 Core/ 分发(MPL-2.0;crates/zt-core/LICENSE-MPL.txt 指向此处)
cp "$SRC/LICENSE.txt" "$CORE_DIR/"
mkdir -p "$CORE_DIR/ext/nlohmann" \
  "$CORE_DIR/ext/prometheus-cpp-lite-1.0/core/include" \
  "$CORE_DIR/ext/prometheus-cpp-lite-1.0/simpleapi/include"
cp "$SRC/ext/nlohmann/json.hpp" "$CORE_DIR/ext/nlohmann/"
cp -R "$SRC/ext/prometheus-cpp-lite-1.0/core/include/prometheus" "$CORE_DIR/ext/prometheus-cpp-lite-1.0/core/include/"
cp -R "$SRC/ext/prometheus-cpp-lite-1.0/simpleapi/include/prometheus" "$CORE_DIR/ext/prometheus-cpp-lite-1.0/simpleapi/include/"
cp -R "$SRC/ext/arm32-neon-salsa2012-asm" "$SRC/ext/x64-salsa2012-asm" "$CORE_DIR/ext/"

echo "$ZT_VERSION" > "$STAMP"
echo "ZeroTier 核心 ${ZT_VERSION} 就绪 → ${CORE_DIR}/"
