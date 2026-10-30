#!/usr/bin/env bash
# scripts/clippy.sh —— 本地 clippy 内环（2026-10-30 用户裁定 A+B 方案）
#
# ## 为什么存在
#
# `cargo clippy --workspace --all-targets` 在本仓库是分钟级，而本地内环
# 根本不需要它的覆盖面：一次改动通常只碰一两个 crate。本脚本做两件事：
#
# - **B（profile）**：固定 `--profile clippy`（Cargo.toml 定义，全程 O0）。
#   clippy 的 lint 在 HIR/MIR 层，opt-level 不影响结论；而 dev profile 为了
#   运行时 FPS 给依赖 O2、热点 crate O2/O3，那笔账不该由静态分析付。
# - **A（改动面）**：无参数时从 git 工作区自动提取改动的 crate，只 lint
#   它们。全量 lint 交 CI（`.github/workflows/gates.yml`）。
#
# ## 用法
#
#   bash scripts/clippy.sh                  # 自动检测改动 crate（推荐日常）
#   bash scripts/clippy.sh wv-render wv-app # 显式指定
#   bash scripts/clippy.sh --all            # 全量（含 app feature 第二遍）
#
# ## app feature 的自动处理
#
# `wv-app` / `wv-render` 的渲染层挂在 `#[cfg(feature = "app")]` 之后，
# 不带 feature 时它们只 lint 数据侧（AGENTS §7.1 的已知盲区）。脚本把这两
# 个 crate 单独分组、自动加 `--features app`；其余 crate 不带 feature 跑
# （`--features app` 对没有该 feature 的包会直接报错，不能混在一组）。
#
# ## 首次使用
#
# `.target/clippy/` 是独立产物目录，**首次跑会全量重建一次**（O0 依赖树）。
# 之后就是标准增量速度。

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

# 渲染层两个 crate：必须带 app feature 才覆盖到渲染/UI 模块。
APP_CRATES=" wv-app wv-render "
# --all 时的全部 crate（= `cargo metadata` 的 workspace members，漏改会在
# run_group 里报「未知 crate」）。
ALL_CRATES=(
    wv-core wv-terrain wv-io wv-overture wv-edit wv-ui wv-default-theme
    wv-render wv-world-cell-map wv-app
)

usage() {
    echo "用法: bash scripts/clippy.sh [--all | <crate>...]" >&2
    echo "  无参数 = 自动检测改动 crate；--all = 全量（含 app feature 第二遍）" >&2
    exit 2
}

run_group() {
    local features="$1"
    shift
    local args=()
    for c in "$@"; do
        [ -d "crates/$c" ] || {
            echo "[clippy] 未知 crate：$c（crates/$c 不存在）" >&2
            exit 2
        }
        args+=(-p "$c")
    done
    local cmd=(cargo clippy --profile clippy "${args[@]}" --all-targets)
    if [ -n "$features" ]; then
        cmd+=(--features "$features")
    fi
    cmd+=(-- -D warnings)
    echo "[clippy] ${cmd[*]}"
    "${cmd[@]}"
}

all=false
crates=()
for arg in "$@"; do
    case "$arg" in
        --all) all=true ;;
        -h | --help) usage ;;
        -*) echo "[clippy] 未知选项：$arg" >&2; usage ;;
        *) crates+=("$arg") ;;
    esac
done

if $all; then
    run_group "" "${ALL_CRATES[@]}"
    run_group "app" wv-render wv-app
    echo "[clippy] PASS"
    exit 0
fi

if [ ${#crates[@]} -eq 0 ]; then
    changed_files="$(
        {
            git diff --name-only HEAD
            git diff --cached --name-only
            git ls-files --others --exclude-standard
        } | sort -u
    )"
    # 只认 `crates/<name>/...` 形态；docs / scripts / examples 不产生 clippy 目标。
    while IFS= read -r c; do
        [ -n "$c" ] && crates+=("$c")
    done < <(
        printf '%s\n' "$changed_files" \
            | sed -n 's#^crates/\([^/]*\)/.*#\1#p' \
            | sort -u
    )
    if [ ${#crates[@]} -eq 0 ]; then
        echo "[clippy] 无改动 crate（docs/scripts/examples 改动不产生 clippy 目标）——跳过。"
        echo "[clippy] 要全量请跑：bash scripts/clippy.sh --all"
        exit 0
    fi
    echo "[clippy] 检测到改动 crate：${crates[*]}"
fi

plain=()
app=()
for c in "${crates[@]}"; do
    if [[ "$APP_CRATES" == *" $c "* ]]; then
        app+=("$c")
    else
        plain+=("$c")
    fi
done

if [ ${#plain[@]} -gt 0 ]; then
    run_group "" "${plain[@]}"
fi
if [ ${#app[@]} -gt 0 ]; then
    # --features 对所有选中包生效，故 app 组单独成批。
    run_group "app" "${app[@]}"
fi

echo "[clippy] PASS"
