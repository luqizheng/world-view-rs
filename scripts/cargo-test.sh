#!/usr/bin/env bash
# scripts/cargo-test.sh —— 测试运行器：把临时目录指向工作区（2026-10-25 增）
#
# ## 为什么存在
# 在**受限执行环境**（CI 沙箱 / 受管终端 / agent 派生进程）下，cargo 拉起的
# 测试子进程**无权写 `%TEMP%`**（`std::env::temp_dir()` 解析到
# `C:\Users\<user>\AppData\Local\Temp`），凡是往临时目录建目录写盘的单测
# 会报 `PermissionDenied (os error 5)` 假失败——典型如 wv-io 的
# `faction_layer_io_tests`（3 条）、`terrain_bin`（2 条）、`create_project`、
# `example_project`、`label_layer_yaml` 等集成测试。
#
# 关键判据：**同一环境里 bash 自己 `mkdir` 同一路径能成功**——受限的只是
# 派生的测试进程，不是系统权限本身；且这些测试在**正常桌面环境直接
# `cargo test` 全绿**。故这是环境约束，不是代码缺陷，**不应**用 `#[ignore]`
# 掩盖（那会永久损失集成覆盖）。
#
# 本脚本把 `TEMP` / `TMP` 指向工作区 `.target/testtmp` 下的**每次运行独享的
# 随机子目录**（mktemp），跑完即清——2026-10-28 用户裁定：多个 worktree
# 并行跑任务时固定同名 temp 会互相踩（与共享 target 同类撞车），随机子目录
# 让并行运行天然互不相干。
#
# ## 用法
#   bash scripts/cargo-test.sh                  # 等价 cargo nextest run --workspace
#   bash scripts/cargo-test.sh -p wv-theme      # 透传 nextest/cargo 参数
#
# ## 2026-10-29 追记：执行器 cargo test → cargo nextest
# cargo test 逐个测试二进制**串行**执行；nextest 用全局并行池跨测试二进制
# 调度，对本仓库多测试目标形态（wv-io 一家 50+ 条集成测试）运行期提速
# 明显。注意：nextest 0.9.x **默认遇失败提前收工**（2026-10-29 实测 2 条
# 失败即跳过 829 条未跑），全量模式显式带 --no-fail-fast 才能一次报全
# 所有失败——门禁要的是完整失败清单，不是最快失败。
# 与 cargo test 的两处偏差：
#   1. nextest 不跑 doctest——全量模式（无参数）会在 nextest 之后自动补
#      `cargo test --workspace --doc`；带过滤参数的内环用法不跑 doc。
#      当前全工作区仅 1 条可执行 doctest（wv-ui widgets/color_picker 的
#      no_run 用例示例，2026-10-29 围栏 grep 核查），补这条成本可忽略。
#   2. `-- --nocapture` 不再适用——nextest 默认失败即打印捕获输出，
#      想成功用例也即时输出用 `--success-output immediate`。
#
# ## 退出码
#   透传 cargo nextest 的退出码（0 = 全绿）。
set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_root="$repo_root/.target/testtmp"
mkdir -p "$tmp_root"
# 每次运行一个独享随机子目录；mktemp -d 即权限安全（0700）。
tmp_dir="$(mktemp -d "$tmp_root/run-XXXXXXXX")"
# 跑完清场（exit 透传 cargo 退出码，清理只兜底）。
trap 'rm -rf "$tmp_dir"' EXIT

# Windows API（GetTempPathW）只认 Win32 路径，MSYS 路径需转换。
if command -v cygpath >/dev/null 2>&1; then
  tmp_win="$(cygpath -w "$tmp_dir")"
else
  tmp_win="$tmp_dir"
fi
export TEMP="$tmp_win"
export TMP="$tmp_win"

cd "$repo_root" || exit 1
if [ "$#" -eq 0 ]; then
  # 全量模式：nextest 跑全部测试二进制（--no-fail-fast 报全失败清单），
  # 随后补 doctest（nextest 不覆盖）。
  cargo nextest run --workspace --no-fail-fast && cargo test --workspace --doc
  exit $?
fi
exec cargo nextest run "$@"
