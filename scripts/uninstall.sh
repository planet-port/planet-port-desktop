#!/bin/bash
# planet-port 卸载脚本(macOS)
#
# 用法: sudo ./uninstall.sh [--purge]
#
# 行为: 卸载 launchd 服务、删 /usr/local/bin 软链与二进制目录;
# 数据目录(planet 注册表/身份/网络数据)默认保留。
# --purge 一并删除数据目录与日志。

set -euo pipefail

APP_DIR="/Library/Application Support/planet-portd"
LINK="/usr/local/bin/planet-port"

die() { echo "错误: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "仅支持 macOS"
if [[ "$(id -u)" -ne 0 ]]; then
    exec sudo bash "$0" "$@"
fi

PURGE=0
for arg in "$@"; do
    case "$arg" in
        --purge) PURGE=1 ;;
        *) die "未知参数: $arg" ;;
    esac
done

# 服务卸载(CLI 不在时跳过,直接清文件)
if [[ -x "$LINK" ]]; then
    "$LINK" service uninstall || true
else
    echo "未找到 ${LINK},跳过服务卸载"
fi

rm -f "$LINK"
rm -rf "${APP_DIR}/bin"

if [[ "$PURGE" -eq 1 ]]; then
    rm -rf "$APP_DIR" /Library/Logs/planet-portd
    echo "已删除数据目录与日志(--purge)"
else
    echo "数据目录保留: ${APP_DIR}(彻底清除: sudo $0 --purge)"
fi

echo "卸载完成"
