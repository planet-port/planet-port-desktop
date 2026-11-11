#!/bin/bash
# zerotier-one 官方服务启停脚本(macOS)
#
# 背景:本机 ZeroTier 已由 planet-port 接管,官方 zerotier-one 处于
# 停用 + 禁自启状态(launchctl bootout + disable),数据保留在
# /Library/Application Support/ZeroTier/One。
# 当 planet-port 出现连接异常时,用本脚本临时拉起官方客户端做对照实验:
# 同机、同 planet、同身份,唯一变量是客户端实现——若官方也连不上,
# 优先怀疑网络出口(家宽 UDP 过滤等),而非 planet-port 自身。
#
# 用法:
#   sudo ./scripts/zerotier-one-service.sh start    # enable + bootstrap,等待上线并展示状态
#   sudo ./scripts/zerotier-one-service.sh stop     # bootout + disable,还原关停状态
#   sudo ./scripts/zerotier-one-service.sh status   # 查看 launchd/进程/端口/连接状态
#
# 注意:
#   - start 前需确认 planet-port 已停(9993 端口独占),脚本会检查并拒绝
#   - 官方服务 KeepAlive=true,bootstrap 即自动拉起并保活
#   - 实验完成后记得 stop 关回,避免与 planet-port 争抢 9993 端口与身份

set -euo pipefail

LABEL="com.zerotier.one"
PLIST="/Library/LaunchDaemons/${LABEL}.plist"
ZT_DIR="/Library/Application Support/ZeroTier/One"
ZT_PORT=9993

# zerotier-cli 探测:官方安装脚本软链在 /usr/local/bin,兜底用数据目录内二进制
if command -v zerotier-cli >/dev/null 2>&1; then
  ZT_CLI="$(command -v zerotier-cli)"
else
  ZT_CLI="${ZT_DIR}/zerotier-cli"
fi

ensure_root() {
  if [[ ${EUID} -ne 0 ]]; then
    exec sudo "$0" "$@"
  fi
}

service_loaded() {
  launchctl print "system/${LABEL}" >/dev/null 2>&1
}

service_disabled() {
  launchctl print-disabled system 2>/dev/null | grep -q "\"${LABEL}\" => disabled"
}

port_busy() {
  lsof -nP -i ":${ZT_PORT}" 2>/dev/null | grep -v '^COMMAND'
}

show_conn() {
  echo "--- zerotier-cli info ---"
  "${ZT_CLI}" info
  echo "--- listnetworks ---"
  "${ZT_CLI}" listnetworks
}

cmd_start() {
  if [[ ! -f "${PLIST}" ]]; then
    echo "错误: 未找到 ${PLIST},官方 zerotier-one 似乎未安装" >&2
    exit 1
  fi
  if port_busy; then
    echo "错误: 端口 ${ZT_PORT} 已被占用(通常是 planet-port 还在运行)。" >&2
    echo "先停掉 planet-port 再拉起官方服务:" >&2
    echo "  sudo launchctl bootout system/cn.imgozi.d.planetport.daemon" >&2
    exit 1
  fi
  if service_loaded; then
    echo "官方服务已在 launchd 中加载,无需重复 start(如需重启先 stop)"
    show_conn
    exit 0
  fi

  # 逆向当初的关停操作:disable -> enable,bootout -> bootstrap
  launchctl enable "system/${LABEL}"
  launchctl bootstrap system "${PLIST}"
  echo "已加载官方服务(KeepAlive 保活),等待与 planet 根建立连接..."

  # ONLINE = 至少连上一个根;连不上会一直 OFFLINE,这本身就是要观察的实验结果,
  # 超时不报错,直接展示当前状态供判断
  for _ in $(seq 1 10); do
    sleep 2
    if "${ZT_CLI}" info 2>/dev/null | awk '{print $NF}' | grep -q ONLINE; then
      break
    fi
  done
  show_conn
}

cmd_stop() {
  if service_loaded; then
    launchctl bootout "system/${LABEL}"
    echo "已停止并移出 launchd"
  else
    echo "官方服务本就未加载,跳过 bootout"
  fi
  launchctl disable "system/${LABEL}"
  echo "已禁用自启(还原到关停状态,数据目录不受影响)"
}

cmd_status() {
  echo "--- launchd ---"
  if service_loaded; then
    launchctl print "system/${LABEL}" 2>/dev/null | grep -E '^\s*(state|pid)' || true
  else
    echo "未加载(launchctl bootout 过)"
  fi
  if service_disabled; then
    echo "自启: 已禁用(disable)"
  else
    echo "自启: 未禁用"
  fi

  echo "--- 进程 ---"
  if pgrep -f "${ZT_DIR}/zerotier-one" >/dev/null 2>&1; then
    ps aux | grep "[${ZT_DIR}/]zerotier-one" || true
  else
    echo "无 zerotier-one 进程"
  fi

  echo "--- 端口 ${ZT_PORT} ---"
  if port_busy; then
    port_busy
  else
    echo "空闲"
  fi

  if service_loaded; then
    show_conn
  fi
}

case "${1:-}" in
  start)  ensure_root "$@"; cmd_start ;;
  stop)   ensure_root "$@"; cmd_stop ;;
  status) ensure_root "$@"; cmd_status ;;
  *) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
