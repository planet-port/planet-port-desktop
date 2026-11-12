#!/usr/bin/env bash
# 发版脚本：git-cliff 计算版本 → 生成 CHANGELOG → 同步 Cargo.toml/Cargo.lock → 提交并打 tag。
# 用法：./scripts/release.sh [beta]   （beta：发布 vX.Y.Z-beta.N 验证版）
# tag 推送后由 cargo-dist 的 Release 工作流接管构建与发布。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

command -v git-cliff >/dev/null 2>&1 || {
    echo "错误：未安装 git-cliff，请先执行 brew install git-cliff" >&2
    exit 1
}
command -v cargo >/dev/null 2>&1 || {
    echo "错误：未安装 Rust 工具链（cargo update 需要它）" >&2
    exit 1
}

MODE="${1:-}"
case "$MODE" in
    "" | beta) ;;
    *) echo "错误：用法 $0 [beta]" >&2; exit 1 ;;
esac

# 已跟踪文件必须无改动，避免把无关改动卷进发版提交（未跟踪文件不阻塞）
git diff --quiet && git diff --cached --quiet || {
    echo "错误：已跟踪文件有未提交改动，请先提交或暂存" >&2
    exit 1
}

# 无新提交时 git-cliff 会原样返回当前版本，防重复发版
LATEST=$(git describe --tags --abbrev=0 | tr -d '[:space:]' | sed 's/^v//')

# 发正式版且上个 tag 是 beta 时，忽略该版本的 beta tag 推导版本与生成日志：
# 否则 cliff 以 beta.N 为基线——beta 后仅有 chore 提交时直接中止，
# 有 feat/fix 时推出的段落只含 beta.N 之后的提交，正式版发布说明会丢失 beta 内容
IGNORE_FLAGS=()
case "$MODE/$LATEST" in
    /*-beta.*) IGNORE_FLAGS=(--ignore-tags "v${LATEST%%-*}-beta.*") ;;
esac

# 从上次 tag 之后的 Conventional Commits 推导下一版本
# （空数组展开需用 + 形式，兼容 macOS 自带 bash 3.2 在 set -u 下的行为）
RAW=$(git-cliff ${IGNORE_FLAGS[@]+"${IGNORE_FLAGS[@]}"} --bumped-version | tr -d '[:space:]')
case "$RAW" in
    v*.*) ;;
    *) echo "错误：无法计算下一版本（自上次 tag 后可能没有新提交）" >&2; exit 1 ;;
esac

# 推导结果与当前版本相同时给出指引而非裸中止
if [ "$RAW" = "v$LATEST" ]; then
    if [ "$MODE" = beta ]; then
        echo "错误：自 v${LATEST} 后没有新提交，无法递增 beta 序号" >&2
    else
        echo "错误：当前已是最新版本 v${LATEST}，自该版本后没有 feat/fix 等可触发版本变动的提交，无需发正式版；如需发布验证版请执行 $0 beta" >&2
    fi
    exit 1
fi

# beta 号一律基于正式版本号推导：cliff 在上个 tag 为 beta 时会返回 beta 版本，
# 直接在其上追加会产出 v0.3.6-beta.2-beta.1 这类畸形号
BASE=${RAW%%-*}
BASE=${BASE#v}
NEXT="$BASE"

# beta 模式：追加递增的 -beta.N（N 取同名 beta tag 的最大序号 +1）
if [ "$MODE" = beta ]; then
    N=0
    for t in $(git tag --list "v$BASE-beta.*"); do
        n=${t##*-beta.}
        [ "$n" -gt "$N" ] 2>/dev/null && N=$n
    done
    NEXT="$BASE-beta.$((N + 1))"
fi

if [ "$NEXT" = "$LATEST" ]; then
    echo "错误：当前已是最新版本 v$LATEST，没有可触发版本变动的提交，无需重复发版；如需发布验证版请执行 $0 beta" >&2
    exit 1
fi

echo "下一版本：v$NEXT"

# 生成中文更新日志（chore 类提交不入日志；ignore-tags 与版本推导保持同一基线）。
# CHANGELOG.md 不存在时 git-cliff 会基于全部历史 tag 生成完整日志
git-cliff ${IGNORE_FLAGS[@]+"${IGNORE_FLAGS[@]}"} --bump -o CHANGELOG.md
# cliff 生成的段落标题可能带 beta 尾巴，统一改写为与本次 tag 同名
# （Release 工作流靠 `## <tag>（` 精确匹配版本 token 提取发布说明）
DATE=$(date "+%Y-%m-%d")
awk -v new="## v${NEXT}（${DATE}）" '
    !done && $0 ~ /^## v/ { $0 = new; done = 1 }
    { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md
grep -q "^## v${NEXT}（" CHANGELOG.md || {
    echo "错误：CHANGELOG.md 未生成 v$NEXT 段落" >&2
    exit 1
}

# 同步 workspace 版本（Cargo.toml 首个 version 赋值行即 [workspace.package].version，
# 依赖的版本声明都在其后；perl 做首处替换以兼容 BSD/GNU sed 差异）
NEXT="$NEXT" perl -0777 -pi -e 's/^version = "[^"]*"/version = "$ENV{NEXT}"/m' Cargo.toml
grep -q "^version = \"$NEXT\"" Cargo.toml || {
    echo "错误：Cargo.toml 版本同步失败" >&2
    exit 1
}
# 仅更新 lockfile 中 workspace 成员的版本记录，不动第三方依赖
cargo update -w

git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "chore(release): v$NEXT" -m "由 scripts/release.sh 生成，版本与更新日志依据 Conventional Commits 自动推导。"
# 附注标签：git push --follow-tags 只推附注标签，轻量标签会被静默漏推
# （tag 不上去则 cargo-dist 的 Release 工作流不触发）
git tag -a "v$NEXT" -m "Release v$NEXT"

# 发版后将开发版本占位到「下一版本-dev」：dev 构建永不与已发布正式版同号
# （否则 dev 升级安装时与线上同名版本无法区分，方向判定也失真）。占位号
# 只是标识，下次发版时会被上面 cliff 算出的正式号覆盖——正式版后保守取
# patch+1（cargo-release 的 dev-version 同款惯例），beta 后取 beta 序号 +1。
# 提交位于 tag 之后，不进本次发版产物
case "$NEXT" in
    *-beta.*)
        DEV="${NEXT%-beta.*}-beta.$((${NEXT##*-beta.} + 1))-dev" ;;
    *)
        _rest="${NEXT#*.}"
        DEV="${NEXT%%.*}.${_rest%%.*}.$((${_rest##*.} + 1))-dev" ;;
esac
DEV="$DEV" perl -0777 -pi -e 's/^version = "[^"]*"/version = "$ENV{DEV}"/m' Cargo.toml
grep -q "^version = \"$DEV\"" Cargo.toml || {
    echo "错误：Cargo.toml 开发版本占位失败" >&2
    exit 1
}
cargo update -w
git add Cargo.toml Cargo.lock
git commit -m "chore(release): 开发版本占位 v$DEV" -m "发版后 dev 分支保持领先一个开发版本号，避免开发构建与已发布正式版同号。"

echo "完成：v$NEXT 已提交并打 tag（附注），开发版本已占位 ${DEV} 。推送到远端时记得：git push --follow-tags"
