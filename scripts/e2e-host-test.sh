#!/usr/bin/env bash
# planet-port 主机级部署 e2e 测试(Linux/systemd 主机)
#
# 用法:
#   ./scripts/e2e-host-test.sh [-e .env.host-test] [-v v0.3.0] [--keep]
#     -e  配置文件路径,缺省仓库根 .env.host-test(格式见 .env.host-test.example)
#     -v  指定 release 版本 tag,缺省取 gh 的 latest 正式版
#     --keep  保留推到主机的 /tmp/e2e-pp 产物目录(排查用;缺省清理)
#
# 前提:
#   - 本机可 ssh 密钥登录目标主机(密码登录不支持,sudo 密码在配置文件里给)
#   - 本机有 gh 并已登录(release 资产下载;家宽直连 github 不稳定,gh 走 API 通道)
#
# 每台主机执行:预检(版本/服务/planet 基线) → 产物下载(scp) → install →
# 版本/服务/恢复校验 → stop/start → 幂等重装 → (可选)数据面互 ping。
# 不做 uninstall(主机留运行态);不覆盖式改配置,planet 导入仅当主机
# 尚无 planet 时按 PLANET_FILE 补一发 add,NETWORK_ID 配置了再补一发 join。
# 结果:逐台打印 STEP:xx:OK/FAIL,末尾汇总表,任一 FAIL 退出码 1。

set -uo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_FILE="$REPO_ROOT/.env.host-test"
VERSION=""
KEEP=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -e) ENV_FILE="$2"; shift 2 ;;
    -v) VERSION="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "未知参数: $1" >&2; exit 2 ;;
  esac
done

[[ -f "$ENV_FILE" ]] || { echo "配置文件不存在: $ENV_FILE (参考 scripts/../.env.host-test.example)" >&2; exit 2; }
# shellcheck disable=SC1090
source "$ENV_FILE"
[[ -n "${TEST_HOSTS:-}" ]] || { echo "配置文件缺少 TEST_HOSTS" >&2; exit 2; }

command -v gh >/dev/null || { echo "本机需要 gh CLI(下载 release 资产)" >&2; exit 2; }
GH_REPO="${GH_REPO:-planet-port/planet-port-desktop}"
WORK="$(mktemp -d /tmp/pp-e2e.XXXXXX)"
trap '[[ $KEEP -eq 1 ]] || rm -rf "$WORK"' EXIT

# ---- 产物缓存:每架构只下载一次 ----
fetch_assets() { # $1=arch(amd64|arm64) → echo 解包目录
  local arch="$1" dir="$WORK/assets-$arch" tgz target
  [[ -d "$dir" ]] && { echo "$dir"; return 0; }
  mkdir -p "$dir"
  # v0.3.1 起产物为 dash 命名 tar.xz(如 planet-port-x86_64-unknown-linux-gnu)
  case "$arch" in
    amd64) target="x86_64-unknown-linux-gnu" ;;
    arm64) target="aarch64-unknown-linux-gnu" ;;
    *) echo "未知架构: $arch" >&2; return 1 ;;
  esac
  gh release download "$VERSION" --repo "$GH_REPO" --pattern "*${target}*" --dir "$dir" \
    || { echo "下载产物失败(${arch})" >&2; return 1; }
  tar -xf "$dir/planet-port-${target}.tar.xz" -C "$dir" || return 1
  [[ -x "$dir/planet-port" ]] || { echo "解包后缺 planet-port" >&2; return 1; }
  echo "$dir"
}

# ---- 远端步骤脚本:stdin 由本脚本注入,参数 1=sudo 喂法前缀 ----
REMOTE_STEPS='
set -u
SUDO_RUN() { # $1=sudo 密码(必填;root 直登时 sudo 不提示,喂了也无害),其余透传
  local pw="$1"; shift
  printf "%s" "$pw" | sudo -S -p "" "$@"
}
step() { printf "STEP:%s:%s\n" "$1" "${2:-OK}"; }
# 1. 安装(覆盖升级或净装共用路径)
SUDO_RUN "$PW" /tmp/e2e-pp/planet-port install --yes >/tmp/e2e-install.log 2>&1 \
  && step install || { step install FAIL; cat /tmp/e2e-install.log; exit 1; }
# 2. 版本与服务
planet-port --version 2>/dev/null | grep -q "${VER#v}" && step version || step version FAIL
systemctl is-active planet-portd >/dev/null && step service-active || step service-active FAIL
systemctl is-enabled planet-portd >/dev/null && step service-enabled || step service-enabled FAIL
# 3. planet 与网络恢复(FRESH=0 覆盖升级:升级后同地址恢复;净装机器只验导入)
sleep 6
INFO_AFTER="$(planet-port info 2>/dev/null || true)"
if [[ "$FRESH" != "1" ]]; then
  INFO_BEFORE="$(cat /tmp/e2e-info-before 2>/dev/null || true)"
  [[ -n "$INFO_AFTER" ]] && step info || step info FAIL
  ADDR_B="$(printf "%s" "$INFO_BEFORE" | awk "NR>1{print \$3}" | sort | tr "\n" ",")"
  ADDR_A="$(printf "%s" "$INFO_AFTER" | awk "NR>1{print \$3}" | sort | tr "\n" ",")"
  [[ -n "$ADDR_B" && "$ADDR_B" = "$ADDR_A" ]] && step addr-same || step addr-same FAIL
  planet-port listnetworks 2>/dev/null | grep -qE "OK|REQUESTING" && step networks || step networks SKIP
fi
# 4. 全新机器补一发 planet 导入(add)+ 加入网络(NETWORK_ID 配置,留空跳过)
if [[ "$FRESH" = "1" && -n "${PLANET_SRC:-}" && -f "${PLANET_SRC:-}" ]]; then
  cp "$PLANET_SRC" /tmp/e2e-pp/gozi25.planet
  planet-port add /tmp/e2e-pp/gozi25.planet --name e2e-test >/dev/null 2>&1 \
    && step planet-add || step planet-add FAIL
  if [[ -n "${NWID:-}" ]]; then
    planet-port join "$NWID" >/dev/null 2>&1 && step join || step join FAIL
  fi
fi
# 5. stop/start
SUDO_RUN "$PW" planet-port service stop >/dev/null 2>&1
sleep 2
systemctl is-active planet-portd >/dev/null 2>&1 && step stop || step stop OK
SUDO_RUN "$PW" planet-port service start >/dev/null 2>&1 && step start || step start FAIL
sleep 8
systemctl is-active planet-portd >/dev/null && step restart-active || step restart-active FAIL
# 6. 幂等重装
SUDO_RUN "$PW" /tmp/e2e-pp/planet-port install --yes >/dev/null 2>&1 && step reinstall || step reinstall FAIL
sleep 8
if [[ "$FRESH" != "1" ]]; then
  planet-port info 2>/dev/null | grep -qiE "connected|已连接" && step final-online || step final-online FAIL
fi
# 7. 数据面互 ping(可选)
if [[ -n "${PING_TARGET:-}" ]]; then
  ping -c3 -W2 "$PING_TARGET" >/dev/null 2>&1 && step ping || step ping FAIL
fi
'

# ---- 逐台执行 ----
OS_LINUX="linux"
declare -a SUMMARY
FAIL_TOTAL=0
for entry in "${TEST_HOSTS[@]}"; do
  IFS='|' read -r host arch pw fresh ping <<<"$entry"
  [[ -n "${host:-}" && -n "${arch:-}" && -n "${pw:-}" ]] || {
    echo "TEST_HOSTS 条目字段不全(需 user@ip|arch|sudo密码|fresh|ping): $entry" >&2
    exit 2
  }
  [[ "${ping:-}" = "-" ]] && ping=""
  name="${host//@/}"
  echo "================================================================"
  echo "==> $host (arch=$arch fresh=${fresh:-0} ping=${ping:-无})"
  echo "================================================================"

  ssh -o BatchMode=yes -o ConnectTimeout=6 "$host" true 2>/dev/null || {
    SUMMARY+=("$host|ssh|FAIL"); FAIL_TOTAL=1; continue; }

  # 版本缺省取 latest 正式版(排除 prerelease)
  if [[ -z "$VERSION" ]]; then
    VERSION="$(gh release view --repo "$GH_REPO" --json tagName,isPrerelease -q 'select(.isPrerelease==false) | .tagName' 2>/dev/null)" \
      || VERSION=""
    [[ -n "$VERSION" ]] || { echo "无法确定版本(gh latest)"; exit 2; }
  fi

  assets="$(fetch_assets "$arch")" || { SUMMARY+=("$host|assets|FAIL"); FAIL_TOTAL=1; continue; }

  # 预检基线:记录升级前 planet 地址(推到远端侧行内比对)
  ssh -o BatchMode=yes "$host" 'planet-port info 2>/dev/null || true' >"$WORK/info-before-$name" 2>/dev/null
  HAS_PLANET="$(ssh -o BatchMode=yes "$host" 'command -v planet-port >/dev/null && planet-port planet list 2>/dev/null | grep -q "🪐" && echo 1 || echo 0')"

  # 推产物
  ssh -o BatchMode=yes "$host" 'rm -rf /tmp/e2e-pp && mkdir -p /tmp/e2e-pp' || { SUMMARY+=("$host|stage|FAIL"); FAIL_TOTAL=1; continue; }
  scp -q "$assets/planet-port" "$assets/planet-portd" "$host:/tmp/e2e-pp/" || { SUMMARY+=("$host|stage|FAIL"); FAIL_TOTAL=1; continue; }
  scp -q "$WORK/info-before-$name" "$host:/tmp/e2e-info-before" 2>/dev/null || ssh -o BatchMode=yes "$host" 'true > /tmp/e2e-info-before'
  ssh -o BatchMode=yes "$host" 'chmod +x /tmp/e2e-pp/planet-port*'

  OUT="$(ssh -o BatchMode=yes "$host" "VER='$VERSION' PW='$pw' FRESH='$HAS_PLANET' PLANET_SRC='${PLANET_FILE:-}' NWID='${NETWORK_ID:-}' PING_TARGET='$ping' bash -s" \
        < <(printf '%s' "$REMOTE_STEPS") 2>&1)"
  echo "$OUT" | grep -E '^STEP:' || echo "$OUT" | tail -5
  FAILS="$(echo "$OUT" | grep -cE '^STEP:.*:(FAIL|SKIP-or-FAIL)')" || true
  OKS="$(echo "$OUT" | grep -cE '^STEP:.*:OK')" || true
  if [[ "${FAILS:-0}" -gt 0 ]]; then
    SUMMARY+=("$host|$OKS ok / $FAILS fail|FAIL"); FAIL_TOTAL=1
  else
    SUMMARY+=("$host|$OKS ok|PASS")
  fi
  [[ "$KEEP" = "1" ]] || ssh -o BatchMode=yes "$host" 'rm -rf /tmp/e2e-pp /tmp/e2e-install.log' 2>/dev/null
done

echo "================================================================"
echo "汇总(版本 $VERSION):"
for s in "${SUMMARY[@]}"; do IFS='|' read -r h r v <<<"$s"; printf '  %-32s %-16s %s\n' "$h" "$v" "$r"; done
exit $FAIL_TOTAL
