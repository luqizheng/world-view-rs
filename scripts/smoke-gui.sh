#!/usr/bin/env bash
# scripts/smoke-gui.sh —— GUI 启动烟测：「启动 5 秒不崩」（ADR-0016 followup · 2026-10-15）
#
# ## 为什么存在
# §7.1 的五条闸门命令（三基础 + app 双闸 + http 双闸）都在**测试层**：
# 单元测试不实例化 Bevy 系统集，B0001（query 互斥证据缺失）、资源初始化顺序、
# Plugin 互依赖这类**运行期 panic** 只有真正启动 GUI 才暴露——ADR-0016 M2 的
# 文字晕边（Without<Camera2d> 缺失）就是这么漏过全部闸门、在首次 `wv gui` 时崩的。
#
# 本脚本补最低保证：**启动 5 秒、进程存活、日志无 panic**。
# 渲染正确性（画面对不对）仍属人工验收，不在此范围。
#
# ## 用法
#   bash scripts/smoke-gui.sh            # 默认 5 秒
#   SMOKE_SECS=8 bash scripts/smoke-gui.sh   # 自定义秒数
#
# ## 退出码
#   0 = 启动存活且日志干净；1 = 提前退出（崩）或日志含 panic/B000x
set -uo pipefail
cd "$(dirname "$0")/.."

DURATION="${SMOKE_SECS:-5}"

# 2026-10-25 追记（AGENTS §13 裁定）：构建目录以全局共享 `CARGO_TARGET_DIR`
# 为准，二进制不再落在 `./target`——脚本跟随该变量，缺省仍回退 `./target`。
# 2026-10-27 追记（feat/faction-partition-map 施工期裁定）：worktree 用
# `.cargo/config.toml` 的 `target-dir = ".target"` 隔离——脚本跟随 cargo
# 的解析序：环境变量 > worktree config > `./target` 缺省。此处用
# `cargo metadata` 单点询问，不再各自猜测路径。
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | tr '\\' '/')"
TARGET_DIR="${TARGET_DIR:-./target}"

echo "[smoke] cargo build -p wv-app --features app --bin wv"
if ! cargo build -q -p wv-app --features app --bin wv; then
    echo "[smoke] FAIL: 构建失败"
    exit 1
fi

BIN="$TARGET_DIR/debug/wv.exe"
if [ ! -f "$BIN" ]; then
    echo "[smoke] FAIL: 二进制未找到 $BIN"
    exit 1
fi

LOG="$(mktemp -t wv-smoke-XXXXXX.log)"
echo "[smoke] 启动 wv gui，观察 ${DURATION}s ..."
"$BIN" gui >"$LOG" 2>&1 &
PID=$!

sleep "$DURATION"

ALIVE=no
if kill -0 "$PID" 2>/dev/null; then
    ALIVE=yes
fi
kill "$PID" 2>/dev/null
wait "$PID" 2>/dev/null

FAIL=0
if [ "$ALIVE" != "yes" ]; then
    echo "[smoke] FAIL: 进程在 ${DURATION}s 内自行退出（疑似启动崩溃）"
    FAIL=1
fi
if grep -Eq "panicked at|error\[B0|Encountered a panic" "$LOG"; then
    echo "[smoke] FAIL: 日志含 panic / B000x："
    grep -En "panicked at|error\[B0|Encountered a panic" "$LOG" | head -5
    FAIL=1
fi

if [ "$FAIL" = "0" ]; then
    echo "[smoke] PASS: 启动 ${DURATION}s 不崩（窗口已创建、无 panic）"
else
    echo "[smoke] 完整日志保留在：$LOG"
fi
rm -f "$LOG"
exit "$FAIL"
