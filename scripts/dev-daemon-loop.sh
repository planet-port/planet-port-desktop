#!/bin/bash
# 真机验证辅助:循环拉起 planet-portd,进程退出(含被 kill)后 10s 自动重启。
# 重启触发:非 root 的辅助进程(如 Claude)无法直接 kill root daemon,
# touch target/.restart-daemon 即可让本脚本杀掉当前这轮并走重启流程。
# Ctrl+C 结束脚本(当前这轮 daemon 收到 SIGTERM 优雅退出后不再重启)。
# 用法:sudo ./scripts/dev-daemon-loop.sh
set -u
cd "$(dirname "$0")/.." || exit 1

FLAG=target/.restart-daemon

# sudo 环境下取原用户家目录,保证 --config-dir 与日常 CLI 指向同一处
if [ -n "${SUDO_USER:-}" ]; then
    HOME_DIR=$(eval echo "~$SUDO_USER")
else
    HOME_DIR="$HOME"
fi
CFG_DIR="${PLANET_PORT_CONFIG_DIR:-$HOME_DIR/Library/Application Support/planet-portd}"

trap 'echo "[loop] 收到退出信号,结束循环"; kill "$PID" 2>/dev/null; exit 0' INT TERM
while true; do
    echo "[loop] $(date '+%H:%M:%S') 启动 planet-portd (config: $CFG_DIR)"
    ./target/release/planet-portd --config-dir "$CFG_DIR" &
    PID=$!
    # daemon 在后台跑;退出或出现重启标记时结束本轮
    while kill -0 "$PID" 2>/dev/null; do
        if [ -f "$FLAG" ]; then
            rm -f "$FLAG"
            kill "$PID"
            break
        fi
        sleep 1
    done
    wait "$PID"
    code=$?
    echo "[loop] $(date '+%H:%M:%S') planet-portd 退出 (code=$code),10s 后重启 (Ctrl+C 结束)"
    sleep 10
done
