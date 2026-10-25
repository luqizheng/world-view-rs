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
# 本脚本把 `TEMP` / `TMP` 指向工作区内的 `.target/testtmp`，一次性解决。
#
# ## 用法
#   bash scripts/cargo-test.sh                  # 等价 cargo test --workspace
#   bash scripts/cargo-test.sh -p wv-theme      # 透传参数
#   bash scripts/cargo-test.sh -- --nocapture   # 透传 cargo test 的其余参数
#
# ## 退出码
#   透传 cargo test 的退出码（0 = 全绿）。
set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$repo_root/.target/testtmp"
mkdir -p "$tmp_dir"

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
  exec cargo test --workspace
fi
exec cargo test "$@"
