#!/usr/bin/env bash
# Windows POC 评估机代码同步：tar 打包工作树(含不入库的 Core/ 源码)，
# 经 ssh 推送到 Win 实测机并解压，规避在 Windows 上跑 bash bootstrap。
# 连接参数读项目根 .env.win10(含密码，已 gitignore)。
set -euo pipefail

cd "$(dirname "$0")/.."

ENV_FILE=".env.win10"
[[ -f "$ENV_FILE" ]] || { echo "sync-win10: 缺少 ${ENV_FILE}(含 WIN10_HOST/WIN10_USER/WIN10_PASS)" >&2; exit 1; }
# shellcheck disable=SC1094
source <(grep -E '^WIN10_' "$ENV_FILE")

TAR="tmp/pp-win.tar.gz"
mkdir -p tmp

echo "==> 打包工作树 → ${TAR}"
tar czf "$TAR" \
  --exclude target --exclude dist --exclude .git \
  --exclude claude-session --exclude tmp --exclude .superpowers \
  --exclude .env.win10 \
  Cargo.toml Cargo.lock crates docs scripts Core

echo "==> 推送 ${WIN10_USER}@${WIN10_HOST}:C:/planet-port/"
sshpass -p "$WIN10_PASS" scp "$TAR" "${WIN10_USER}@${WIN10_HOST}:C:/planet-port/pp.tar.gz"

echo "==> 远端解压"
sshpass -p "$WIN10_PASS" ssh "${WIN10_USER}@${WIN10_HOST}" \
  'cmd /c "cd /d C:\planet-port && tar xzf pp.tar.gz && del pp.tar.gz && echo SYNC_OK"'

echo "==> 同步完成"
