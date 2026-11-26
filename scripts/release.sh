#!/usr/bin/env bash
# 组件化发版：./scripts/release.sh <daemon|cli|gui> [beta] [--dry-run]
#   推导该组件下一版本(scope 归属表内置；cliff 的 no_increment_regex 仅作用
#   于 type、scope 过滤无法参与 bump，故推导不走 git-cliff)→ 渲染 CHANGELOG-<comp>.md
#   → bump crate 版本(gui 三源同步)→ 提交 + 斜杠 tag → dev 占位。
#   --dry-run 只读演练：打印基线/提交清单/推导结果，不动任何文件。
# tag 推送后由 cargo-dist 的 Release 工作流接管(斜杠 tag 原生绑定单 package)。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

usage() { echo "用法：$0 <daemon|cli|gui> [beta] [--dry-run]" >&2; exit 1; }

COMP="${1:-}"; shift || usage
case "$COMP" in daemon|cli|gui) ;; *) usage ;; esac
MODE="" DRY=0
for a in "$@"; do
    case "$a" in
        beta) MODE=beta ;;
        --dry-run) DRY=1 ;;
        *) usage ;;
    esac
done

command -v git-cliff >/dev/null 2>&1 || { echo "错误：未安装 git-cliff（brew install git-cliff）" >&2; exit 1; }
command -v cargo  >/dev/null 2>&1 || { echo "错误：未安装 Rust 工具链" >&2; exit 1; }

# ── 组件元数据 ─────────────────────────────────────────────
case "$COMP" in
    daemon) PKG="planet-portd";    CRATE="crates/daemon/Cargo.toml";          SCOPES="daemon zt-core zt-sys api" ;;
    cli)    PKG="planet-port";     CRATE="crates/cli/Cargo.toml";             SCOPES="cli zt-core zt-sys api" ;;
    gui)    PKG="planet-port-gui"; CRATE="crates/gui/src-tauri/Cargo.toml";   SCOPES="gui tray net settings api" ;;
esac
KNOWN_SCOPES="gui tray net settings cli daemon zt-core zt-sys api"
# 工程类 scope 不归属任何组件，fix 之不触发 bump 也不入日志（与 cliff
# 渲染层的 fix(工程 scope) skip 同源，否则两侧判定割裂：cliff 放行的
# 提交在推导侧报错，或推导侧放行的提交 cliff 不入日志）
ENGINEERING_SCOPES="ci docs build test release deps"

# scope 归属须整词精确相等：grep -w 的词界不含 '-'，'core' 会误中 'zt-core'
has_token() { case " $2 " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

git diff --quiet && git diff --cached --quiet || {
    echo "错误：已跟踪文件有未提交改动，请先提交或暂存" >&2; exit 1;
}

# ── 基线：本组件最新非 beta 稳定 tag；迁移首版回落 v0.5.0 ──
# 两模式统一取稳定基线：sort -V 下同名正式版排在 beta 之前，取「最新
# tag」会在有稳定 tag 时居尾选中旧 beta——区间错过其间已发布的正式
# 内容，推出的 beta 号虚高，后续正式版回落稳定基线即倒挂。稳定基线
# 的区间天然收齐 beta 线全部提交，档位单调不降，续号不会倒挂
# 首个组件版直接以 beta 起步时无非 beta tag 可选，grep 无选中行会以
# pipefail 中止整个脚本，|| true 放行空结果走下方 v0.5.0 回落
LATEST_TAG="$(git tag --list "${PKG}/v*" | grep -v -- '-beta\.' | sort -V | tail -1 || true)"
if [ -n "$LATEST_TAG" ]; then
    LATEST="${LATEST_TAG#${PKG}/v}"
    RANGE="${LATEST_TAG}..HEAD"
else
    LATEST="0.5.0"
    RANGE="v0.5.0..HEAD"
fi

# 渲染时忽略界内本组件 beta tag，两模式都需要：beta 模式区间含既有
# beta 线 tag、正式模式区间可能含 beta 线 tag，cliff 都会在其处分段，
# awk 只改首段标题会错冠、该段被整段前插成重复段
IGNORE_BETA_ARGS=(--ignore-tags "${PKG}/v[0-9].*-beta\.[0-9]+")

# ── 版本推导（内置；scope 归属表见 usage 区注释与 KNOWN_SCOPES）──
LEVEL="" BAD="" HIT=0
# 一条提交读成一个记录：%s 与 %b 一起取（subject 与 body 用 \x1f 分隔、
# 记录间用 \x00 分隔）——breaking 有两个合法声明位：subject 尾 ! 与 body
# 里的 BREAKING CHANGE footer，只取 %s 时后者永远不可见
while IFS= read -r -d '' rec; do
    # git 的 --format 按 tformat 语义在每条记录后再补一个换行，read 按记录
    # 分割后它会出现在下一条记录的头部；subject 不可能以换行开头，剥掉即可
    # （不剥则从第二条记录起整条记录的 subject 解析全部带前导换行而失真）
    rec="${rec#$'\n'}"
    subj="${rec%%$'\x1f'*}"
    body="${rec#*$'\x1f'}"
    [ -n "$subj" ] || continue
    # breaking 只认冒号前 type token 尾部的 !（fix!: / feat(x)!:），
    # subject 正文含 ! 不算；! 先归一掉再入 case，否则 feat!: 这类
    # 无 scope 破坏提交会因 type 不匹配被静默吞掉
    type="${subj%%:*}"
    breaking=0; [[ "$type" == *'!' ]] && { breaking=1; type="${type%!}"; }
    type="${type%%(*}"
    # Conventional Commits 的 footer 形式（body 行首 BREAKING CHANGE: /
    # BREAKING-CHANGE:）同样声明破坏变更；不认它会把这类提交推成 minor，
    # 而 cliff 侧 protect_breaking_commits 已按 breaking 渲染（major 保护），
    # 推导与组件日志两侧失真（口径对齐 git-cliff，两种 footer 拼写都认）
    [[ "$body" == "BREAKING CHANGE:"* || "$body" == *$'\n'"BREAKING CHANGE:"* \
        || "$body" == "BREAKING-CHANGE:"* || "$body" == *$'\n'"BREAKING-CHANGE:"* ]] && breaking=1
    scope=""; [[ "$subj" == *"("*")"* ]] && { scope="${subj#*(}"; scope="${scope%%)*}"; }
    case "$type" in
        feat|fix|perf|revert)
            if [ -z "$scope" ]; then
                BAD="${BAD}  无 scope   ${subj}\n"; continue
            fi
            if [ "$type" = fix ] && has_token "$scope" "$ENGINEERING_SCOPES"; then
                continue
            fi
            if ! has_token "$scope" "$KNOWN_SCOPES"; then
                BAD="${BAD}  未知 scope(${scope})   ${subj}\n"; continue
            fi
            if has_token "$scope" "$SCOPES"; then
                HIT=1
                if [ "$breaking" = 1 ]; then NEWL=major
                elif [ "$type" = feat ]; then
                    [ "${NEWL:-}" != major ] && NEWL=minor
                else
                    [ -z "${NEWL:-}" ] && NEWL=patch
                fi
            fi
            ;;
    esac
done < <(git log --format='%s%x1f%b%x00' "$RANGE")

if [ -n "$BAD" ]; then
    echo "错误：以下版本级提交的 scope 无法归属（合法值：${KNOWN_SCOPES} 中的 ${SCOPES} 归 ${COMP}），请拆分提交或修正 scope：" >&2
    printf '%b' "$BAD" >&2
    exit 1
fi
[ "$HIT" = 1 ] || { echo "错误：${RANGE} 内没有归属 ${COMP} 的 feat/fix/perf/revert 提交，无需发版" >&2; exit 1; }
LEVEL="${NEWL:-patch}"

# 基线号（恒为稳定号）按档位递增
BASE_VER="${LATEST%%-*}"
IFS='.' read -r MA MI PA <<<"$BASE_VER"
case "$LEVEL" in
    major) NEXT="$((MA + 1)).0.0" ;;
    minor) NEXT="${MA}.$((MI + 1)).0" ;;
    patch) NEXT="${MA}.${MI}.$((PA + 1))" ;;
esac

if [ "$MODE" = beta ]; then
    N=0
    for t in $(git tag --list "${PKG}/v${NEXT}-beta.*"); do
        n="${t##*-beta.}"; [ "$n" -gt "$N" ] 2>/dev/null && N=$n
    done
    NEXT="$NEXT-beta.$((N + 1))"
fi
[ "$NEXT" != "$LATEST" ] || { echo "错误：推导结果与当前版本相同（$LATEST），无需发版" >&2; exit 1; }

TAG="${PKG}/v${NEXT}"
echo "组件 $COMP ｜ 基线 ${LATEST_TAG:-v0.5.0(迁移首版)} ｜ 档位 $LEVEL ｜ 下一版本 $TAG"

if [ "$DRY" = 1 ]; then
    echo "── 归属 $COMP 的版本级提交："
    git log --format='  %s' "$RANGE" | grep -E '^(  )?(feat|fix|perf|revert)' | grep -E "\(($(echo "$SCOPES" | tr ' ' '|')\))" || true
    echo "（dry-run 结束，未做任何修改）"
    exit 0
fi

# ── CHANGELOG 渲染（cliff 只渲染不推导，版本号全由脚本内置推导）──
# 只渲染本次区间生成新段、既有段落原样保留：首枚组件 tag 之上没有更早的
# 本组件 tag，让 cliff 全量重生成会把 v0.5.0 前已发布的整段历史回填进新
# 文件；--bump 同因不可用——无本组件 tag 时 cliff 从 0.1.0 起算，又被
# tag_pattern 校验拒绝而报错退出（实测 2.14.2）
DATE=$(date "+%Y-%m-%d")
SEC_FILE="CHANGELOG-${COMP}.md"
NEW_FILE="${SEC_FILE}.new"
rm -f "$NEW_FILE"
# 空数组展开用 + 形式，兼容 macOS bash 3.2 在 set -u 下的行为
git-cliff --config "cliff/${COMP}.toml" ${IGNORE_BETA_ARGS[@]+"${IGNORE_BETA_ARGS[@]}"} --tag-pattern "${PKG}/v[0-9].*" "$RANGE" > "$NEW_FILE"
# 段标题统一改写为与本次 tag 同名（Release 工作流按 `## <tag>（` 提取发布说明）
awk -v new="## ${TAG}（${DATE}）" '
    !done && $0 ~ /^## / { $0 = new; done = 1 }
    { print }
' "$NEW_FILE" > "${NEW_FILE}.tmp" && mv "${NEW_FILE}.tmp" "$NEW_FILE"
# 新段插到文件头块之后（既有段落保持冻结态）；文件尚不存在则整文件即渲染结果
if [ -f "$SEC_FILE" ]; then
    awk -v secfile="$NEW_FILE" '
        BEGIN {
            p = 0
            while ((getline line < secfile) > 0) {
                if (!p && line ~ /^## /) p = 1
                if (p) sec = sec line "\n"
            }
        }
        !ins && /^## / { printf "%s", sec; ins = 1 }
        { print }
    ' "$SEC_FILE" > "${SEC_FILE}.tmp" && mv "${SEC_FILE}.tmp" "$SEC_FILE"
else
    mv "$NEW_FILE" "$SEC_FILE"
fi
rm -f "$NEW_FILE"
grep -q "^## ${TAG}（" "$SEC_FILE" || { echo "错误：${SEC_FILE} 未生成 ${TAG} 段落" >&2; exit 1; }

# ── 版本写入（crate 单源；gui 另同步 tauri.conf.json 与 package.json）──
NEXT="$NEXT" perl -0777 -pi -e 's/^version = "[^"]*"/version = "$ENV{NEXT}"/m' "$CRATE"
grep -q "^version = \"$NEXT\"" "$CRATE" || { echo "错误：$CRATE 版本同步失败" >&2; exit 1; }
if [ "$COMP" = gui ]; then
    NEXT="$NEXT" perl -0777 -pi -e 's/"version": "[^"]*"/"version": "$ENV{NEXT}"/m' \
        crates/gui/src-tauri/tauri.conf.json crates/gui/package.json
    grep -q "\"version\": \"$NEXT\"" crates/gui/src-tauri/tauri.conf.json || { echo "错误：tauri.conf.json 同步失败" >&2; exit 1; }
    grep -q "\"version\": \"$NEXT\"" crates/gui/package.json || { echo "错误：package.json 同步失败" >&2; exit 1; }
fi
cargo update -w

FILES=(CHANGELOG-${COMP}.md "$CRATE" Cargo.lock)
[ "$COMP" = gui ] && FILES+=(crates/gui/src-tauri/tauri.conf.json crates/gui/package.json)
git add "${FILES[@]}"
git commit -m "chore(release): ${COMP} v$NEXT" -m "由 scripts/release.sh 生成，${COMP} 组件版本与更新日志按 scope 归属独立推导。"
git tag -a "$TAG" -m "Release $PKG v$NEXT"

# ── dev 占位（不进发版产物；正式版后 patch+1，beta 后序号+1）──
case "$NEXT" in
    *-beta.*) DEV="${NEXT%-beta.*}-beta.$((${NEXT##*-beta.} + 1))-dev" ;;
    *) _rest="${NEXT#*.}"; DEV="${NEXT%%.*}.${_rest%%.*}.$((${_rest##*.} + 1))-dev" ;;
esac
DEV="$DEV" perl -0777 -pi -e 's/^version = "[^"]*"/version = "$ENV{DEV}"/m' "$CRATE"
if [ "$COMP" = gui ]; then
    DEV="$DEV" perl -0777 -pi -e 's/"version": "[^"]*"/"version": "$ENV{DEV}"/m' \
        crates/gui/src-tauri/tauri.conf.json crates/gui/package.json
fi
cargo update -w
git add "$CRATE" Cargo.lock
[ "$COMP" = gui ] && git add crates/gui/src-tauri/tauri.conf.json crates/gui/package.json
git commit -m "chore(release): ${COMP} 开发版本占位 v$DEV" -m "发版后 dev 分支保持领先一个开发版本号，避免开发构建与已发布正式版同号。"

echo "完成：$TAG 已提交并打 tag（附注），dev 占位 ${DEV} 。推送：git push --follow-tags"
