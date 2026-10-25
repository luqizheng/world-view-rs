# -*- coding: utf-8 -*-
"""把七类手绘符号注入地图预览模板 → 单文件 HTML。

**为什么内联 base64**：预览页要在 WorkBuddy 预览面板 / 任意浏览器里直接打开，
不能依赖相对路径（预览服务的根目录不确定）。七张 96px PNG 合计约 130 KB，
内联后单文件自包含。

用法：
    py assets/build_map_preview.py
    → preview/map_preview.html
"""
from __future__ import annotations

import base64
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TEMPLATE = REPO / "assets/preview_template.html"
OUT = REPO / "preview/map_preview.html"
SRC_DIR = REPO / "crates/wv-render/assets/symbols"

# 占位符 → 资产文件（统一用 96px 档：28px 屏幕上仍有 3.4x 超采样）
ASSETS = {
    "__TREE__": "树冠_96.png",
    "__PEAK__": "山峰_96.png",
    "__SNOWPEAK__": "雪峰_96.png",
    "__CASTLE__": "城堡_96.png",
    "__TOWN__": "城镇_96.png",
    "__SHIP__": "帆船_96.png",
    "__COMPASS__": "指南针_96.png",
}


def data_uri(path: Path) -> str:
    return "data:image/png;base64," + base64.b64encode(path.read_bytes()).decode("ascii")


def main() -> None:
    html = TEMPLATE.read_text(encoding="utf-8")
    for token, name in ASSETS.items():
        src = SRC_DIR / name
        if not src.exists():
            raise SystemExit(f"缺资产：{src}（先跑 assets/process_symbols.py）")
        html = html.replace(token, data_uri(src))
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(html, encoding="utf-8")
    print(f"{OUT.relative_to(REPO)}  {OUT.stat().st_size / 1024:.0f} KB")


if __name__ == "__main__":
    main()
