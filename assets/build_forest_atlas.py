# -*- coding: utf-8 -*-
"""把单张手绘树冠拼成 3 变体横向图集（覆盖物层符号）。

**为什么需要这一步**：`FeatureRule::symbol_texture` 走的是 3 变体图集
（`wv-render::feature_overlays::SYMBOL_ATLAS_VARIANTS = 3`），UV 按横向三等分
切分。直接塞单张方图 → 只显示左边 1/3 且缩放错位。

**当前策略**：3 格放同一张图（链路验证用）。后续美术给出针叶 / 阔叶 / 团块
三种形态时，把 THIRD / SECOND 换成对应文件即可，契约与代码零改动。

用法：
    py assets/build_forest_atlas.py
    py assets/build_forest_atlas.py --src assets/symbols_src/树冠_96.png --dst ...
"""
from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image

REPO = Path(__file__).resolve().parent.parent
DEFAULT_SRC = REPO / "crates/wv-render/assets/symbols/树冠_96.png"
DEFAULT_DST = REPO / "crates/wv-render/assets/overlays/forest/tree_atlas_canopy.png"

# 3 变体：值 = 源文件路径（None 表示复用 SRC）。占位期全用同一张。
SECOND = None
THIRD = None


def build(src: Path, dst: Path) -> None:
    tiles = [src, SECOND or src, THIRD or src]
    images = [Image.open(p).convert("RGBA") for p in tiles]
    cell_w = max(im.width for im in images)
    cell_h = max(im.height for im in images)

    atlas = Image.new("RGBA", (cell_w * len(images), cell_h), (0, 0, 0, 0))
    for i, im in enumerate(images):
        # 内容居中贴入等分格——UV 等分要求每格几何一致。
        x = i * cell_w + (cell_w - im.width) // 2
        y = (cell_h - im.height) // 2
        atlas.paste(im, (x, y), im)

    dst.parent.mkdir(parents=True, exist_ok=True)
    atlas.save(dst, "PNG")
    print(f"{dst.relative_to(REPO)}  {atlas.width}x{atlas.height}（{len(images)} 变体，每格 {cell_w}x{cell_h}）")


def main() -> None:
    parser = argparse.ArgumentParser(description="树冠 → 3 变体横向图集")
    parser.add_argument("--src", type=Path, default=DEFAULT_SRC)
    parser.add_argument("--dst", type=Path, default=DEFAULT_DST)
    args = parser.parse_args()
    build(args.src, args.dst)


if __name__ == "__main__":
    main()
