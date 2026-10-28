#!/usr/bin/env bash
# scripts/smoke-http.sh —— MCP HTTP server 真机烟测（ADR-0014 v0.2 followup · 2026-10-15）
#
# ## 为什么存在
# `cargo http-test` 里的 2 条 end-to-end 用例走 `tower::ServiceExt::oneshot`——
# **内存内**跑 handler，不经过真实 TCP 监听 / 端口绑定 / HTTP 解析。
# ADR-0014 §2.3 的承诺形态是「LLM 客户端通过 127.0.0.1:8732 访问运行中的
# 编辑器」，这条链路只有真机 smoke 才验证得到：HttpServerPlugin 是否真的
# 在 Startup 起了 tokio 线程、axum 是否真的绑上端口、curl 是否真能调通。
#
# 本脚本补最低保证：**启动 GUI → 真实 curl POST /mcp → tools/list 返全部
# skill（现 15 个，ADR-0078 十三 + ADR-0082 二）→ tools/call list_maps /
# set_view_mode / screenshot 走通**。
#
# ## 用法
#   bash scripts/smoke-http.sh              # 默认端口 18732（避开默认 8732，
#                                           # 顺带验证 --http-port flag）
#   SMOKE_PORT=9000 bash scripts/smoke-http.sh
#
# ## 退出码
#   0 = 全部 skill schema + list_maps / set_view_mode / screenshot 调用通过；
#   1 = 任一环节失败
set -uo pipefail
cd "$(dirname "$0")/.."

PORT="${SMOKE_PORT:-18732}"
URL="http://127.0.0.1:${PORT}/mcp"
TARGET_DIR="${CARGO_TARGET_DIR:-.target}"

echo "[smoke] cargo build -p wv-app --features http --bin wv"
if ! cargo build -q -p wv-app --features http --bin wv; then
    echo "[smoke] FAIL: 构建失败"
    exit 1
fi

BIN="$TARGET_DIR/debug/wv.exe"
if [ ! -f "$BIN" ]; then
    echo "[smoke] FAIL: 二进制未找到 $BIN"
    exit 1
fi

LOG="$(mktemp -t wv-http-smoke-XXXXXX.log)"
echo "[smoke] 启动 wv gui --http-port ${PORT} ..."
"$BIN" gui --http-port "$PORT" >"$LOG" 2>&1 &
PID=$!

cleanup() {
    kill "$PID" 2>/dev/null
    wait "$PID" 2>/dev/null
    if [ "$FAIL" != "0" ]; then
        echo "[smoke] 完整日志保留在：$LOG"
    else
        rm -f "$LOG"
    fi
}
trap cleanup EXIT

FAIL=0

# 等端口就绪（最多 20s；GUI 启动含 Bevy/wgpu 初始化，比纯 server 慢）。
READY=no
for _ in $(seq 1 40); do
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "[smoke] FAIL: 进程提前退出："
        tail -5 "$LOG"
        FAIL=1
        exit 1
    fi
    if curl -s -o /dev/null --max-time 1 "$URL" -X POST \
        -H 'content-type: application/json' \
        -d '{"jsonrpc":"2.0","method":"tools/list","id":0}' 2>/dev/null; then
        READY=yes
        break
    fi
    sleep 0.5
done
if [ "$READY" != "yes" ]; then
    echo "[smoke] FAIL: ${PORT} 端口 20s 内未就绪"
    tail -5 "$LOG"
    FAIL=1
    exit 1
fi
echo "[smoke] 端口 ${PORT} 已就绪"

# ① tools/list：应返 15 个 skill，顺序与 ADR-0014 §2.5 + ADR-0078 + ADR-0082
# （自然区命名两接口 2026-10-28 追加；当时单测已更 15、本脚本漏更致假红）一致。
TOOLS_JSON=$(curl -s --max-time 5 "$URL" -X POST \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/list","id":1}')
NAMES=$(echo "$TOOLS_JSON" | python3 -c "
import json,sys
r = json.load(sys.stdin)
tools = r['result']['tools']
names = [t['name'] for t in tools]
expected = ['list_maps','get_map','list_places','get_place','get_state_at_time',
            'get_naming_context',
            'create_map','create_place','undo','save_project',
            'set_nature_names',
            'get_ui_state','set_view_mode','set_panel_visible','screenshot']
assert names == expected, f'skill 列表/顺序错: {names}'
assert all(t['inputSchema']['type'] == 'object' for t in tools), 'inputSchema.type != object'
print(len(names))
")
if [ "$NAMES" = "15" ]; then
    echo "[smoke] PASS ①: tools/list 返 15 个 skill，顺序正确"
else
    echo "[smoke] FAIL ①: tools/list 异常："
    echo "$TOOLS_JSON" | head -3
    FAIL=1
fi

# ② tools/call list_maps：读路径走通，isError=false。
CALL_JSON=$(curl -s --max-time 5 "$URL" -X POST \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"list_maps","arguments":{}},"id":2}')
IS_ERR=$(echo "$CALL_JSON" | python3 -c "
import json,sys
r = json.load(sys.stdin)
print(r['result']['isError'])
")
if [ "$IS_ERR" = "False" ]; then
    echo "[smoke] PASS ②: tools/call list_maps 走通（isError=false）"
else
    echo "[smoke] FAIL ②: tools/call list_maps 异常："
    echo "$CALL_JSON" | head -3
    FAIL=1
fi

# ③ 未知方法：应回 -32601。
BAD_JSON=$(curl -s --max-time 5 "$URL" -X POST \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/frobnicate","id":3}')
CODE=$(echo "$BAD_JSON" | python3 -c "
import json,sys
r = json.load(sys.stdin)
print(r['error']['code'])
")
if [ "$CODE" = "-32601" ]; then
    echo "[smoke] PASS ③: 未知方法回 -32601"
else
    echo "[smoke] FAIL ③: 未知方法应回 -32601，实际 {"
    echo "$BAD_JSON" | head -3
    FAIL=1
fi

# ④ set_view_mode（ADR-0078）：UI 操控经真实 TCP 链路走通，echo 快照带新模式。
MODE_JSON=$(curl -s --max-time 5 "$URL" -X POST \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"set_view_mode","arguments":{"mode":"cellmap"}},"id":4}')
MODE=$(echo "$MODE_JSON" | python3 -c "
import json,sys
r = json.load(sys.stdin)
assert r['result']['isError'] == False, f'isError=true: {r}'
payload = json.loads(r['result']['content'][0]['text'])
assert payload['view_mode'] == 'cellmap', f'echo 快照不带新模式: {payload}'
print(payload['view_mode'])
")
if [ "$MODE" = "cellmap" ]; then
    echo "[smoke] PASS ④: set_view_mode(cellmap) 走通并 echo 新快照"
else
    echo "[smoke] FAIL ④: set_view_mode 异常："
    echo "$MODE_JSON" | head -3
    FAIL=1
fi

# ⑤ get_ui_state：面板快照列全量用户可见面板。
STATE_JSON=$(curl -s --max-time 5 "$URL" -X POST \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"get_ui_state","arguments":{}},"id":5}')
PN=$(echo "$STATE_JSON" | python3 -c "
import json,sys
r = json.load(sys.stdin)
assert r['result']['isError'] == False, f'isError=true: {r}'
payload = json.loads(r['result']['content'][0]['text'])
panels = payload['panels']
assert any(p['panel'] == 'brush' for p in panels), f'快照缺 brush: {payload}'
print(len(panels))
")
if [ -n "$PN" ] && [ "$PN" -ge 10 ] 2>/dev/null; then
    echo "[smoke] PASS ⑤: get_ui_state 返 $PN 个面板快照"
else
    echo "[smoke] FAIL ⑤: get_ui_state 异常："
    echo "$STATE_JSON" | head -3
    FAIL=1
fi

# ⑥ screenshot（ADR-0078）：image content 回传 + 落盘文件真实存在。
SHOT_JSON=$(curl -s --max-time 15 "$URL" -X POST \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"screenshot","arguments":{}},"id":6}')
SHOT=$(echo "$SHOT_JSON" | python3 -c "
import json,sys,base64,os
r = json.load(sys.stdin)
assert r['result']['isError'] == False, f'isError=true: {str(r)[:300]}'
img = r['result']['content'][0]
assert img['type'] == 'image', f'content[0] 不是 image: {img[\"type\"]}'
assert img['mimeType'] == 'image/png', 'mimeType != image/png'
raw = base64.b64decode(img['data'])
assert raw[:4] == b'\x89PNG', '解码后不是 PNG'
assert len(raw) > 1000, f'PNG 小得可疑: {len(raw)}B'
meta = json.loads(r['result']['content'][1]['text'])
p = meta['saved_path']
assert os.path.exists(p), f'落盘文件不存在: {p}'
print(f'{meta[\"width\"]}x{meta[\"height\"]} {len(raw)}B -> {p}')
")
if [ -n "$SHOT" ]; then
    echo "[smoke] PASS ⑥: screenshot 回传 image content：$SHOT"
else
    echo "[smoke] FAIL ⑥: screenshot 异常："
    echo "$SHOT_JSON" | head -3
    FAIL=1
fi

if [ "$FAIL" = "0" ]; then
    echo "[smoke] PASS: MCP server 真机链路（TCP 绑定 + HTTP 解析 + 13 skill + 读路径 + UI 操控 + 截图回传 + 错误路径）全通"
fi
exit "$FAIL"
