#!/usr/bin/env python3
"""把 AI 生成的单棵树归一化并拼成 `terrain_decor` 用的 3 格横向图集（ADR-0049）。

用法：
  python scripts/build_decor_atlas.py --out crates/wv-render/assets/overlays/forest/decor_atlas_day.png
  python scripts/build_decor_atlas.py --out .../decor_atlas_dusk.png --light 242,234,216

输入：`JOBS` 表里的源图（AI 出图，纯白底，未提交到仓库——提示词见
`docs/theme/default/overlays.md`，重制时按键重新生成即可）。

处理链（与 `normalize_overlay_asset.py` 同族）：
  1. 涂白水印区（右下角固定方块，供应商水印）；
  2. 可选裁掉底部（`cut_bottom`）——生成图有时带草地底座，地图符号不要；
  3. 白底转 alpha：`alpha = 1 - min(R,G,B)/BG`，反预乘恢复墨色；
  4. 内容 bbox 居中放进 `cell × cell` 画布（底部居中，树根贴底）；
  5. 三格横向拼接成 `cells × cell` 图集。

格序契约（渲染层按此取格）：**针叶 / 阔叶 / 稀树**，与 `DecorVariant` 对齐。
依赖：Pillow + numpy（仅开发期，不进 Rust 运行时）。
"""

import argparse
import os

import numpy as np
from PIL import Image

BG = 250.0  # 源图白底实测亮度

# 源图 → (输出格, 裁掉底部 y, 说明)。
# 源图不随仓库提交：重制时按 docs/theme/default/overlays.md 的提示词重新生成，
# 改这里的文件名即可。`cut_bottom` 为 None 表示保留整图。
JOBS = [
    # 针叶：三株云杉一排，深绿，剪影最硬——放第 0 格。
    ("dsh-image-gen/image-4ebd85bc.jpg", 0, None, "conifer"),
    # 阔叶：单株圆冠，暖绿。
    ("dsh-image-gen/image-57f0ccb5.jpg", 1, None, "broadleaf"),
    # 稀树：单株伞形，生成图底部带草地底座——在草线以上（实测 y≈755）裁掉。
    ("dsh-image-gen/image-13a6590a.jpg", 2, 752, "sparse"),
]


def blank_watermark(rgb: np.ndarray) -> np.ndarray:
    """把右下角水印区涂成白底（供应商水印位置固定）。"""
    h, w, _ = rgb.shape
    x0, x1 = int(w * 0.76), w
    y0, y1 = int(h * 0.90), h
    rgb = rgb.copy()
    rgb[y0:y1, x0:x1] = BG
    return rgb


def keyed_rgba(rgb: np.ndarray, light: tuple[int, int, int] | None) -> np.ndarray:
    """白底 → straight-alpha RGBA。

    `light` 非空时输出**浅墨版**（暗色主题用）：直接用目标色，不走白底反预乘，
    否则低 alpha 处会被算成比目标色更暗的颜色。
    """
    alpha = np.clip((BG - rgb.min(axis=2)) / BG, 0, 1)
    if light is not None:
        a = np.clip((alpha - 0.18) / 0.55, 0, 1) ** 0.85
        out_rgb = np.full_like(rgb, 1.0) * np.array(light, dtype=np.float32)
    else:
        a = np.clip(alpha * 1.25, 0, 1)
        with np.errstate(divide="ignore", invalid="ignore"):
            out_rgb = np.where(
                a[..., None] > 0,
                (rgb - BG * (1 - a[..., None])) / np.maximum(a[..., None], 1e-6),
                BG,
            )
        out_rgb = np.clip(out_rgb, 0, 255)
    a[a < 0.04] = 0.0
    return np.dstack([out_rgb, a * 255]).astype(np.uint8)


def fit_cell(rgba: np.ndarray, cell: int, margin: int) -> Image.Image:
    """内容 bbox → 底部居中放进 cell 画布（树根贴底，向上生长）。"""
    ink = rgba[..., 3] > 10
    ys, xs = np.where(ink)
    if len(xs) == 0:
        raise SystemExit("裁切后没有内容——检查 cut_bottom / 水印区")
    rgba = rgba[ys.min() : ys.max() + 1, xs.min() : xs.max() + 1]

    img = Image.fromarray(rgba, "RGBA")
    cw, ch = img.size
    inner = cell - margin * 2
    scale = inner / max(cw, ch)
    nw, nh = max(1, round(cw * scale)), max(1, round(ch * scale))
    # 预乘后缩放，避免透明边缘出现暗边。
    pre = np.asarray(img).astype(np.float32)
    pre[..., :3] *= pre[..., 3:4] / 255.0
    small = Image.fromarray(pre.astype(np.uint8), "RGBA").resize((nw, nh), Image.LANCZOS)
    pre = np.asarray(small).astype(np.float32)
    a = pre[..., 3:4] / 255.0
    pre[..., :3] = np.where(a > 0, pre[..., :3] / np.maximum(a, 1e-6), 0)

    out = Image.new("RGBA", (cell, cell), (0, 0, 0, 0))
    out.paste(
        Image.fromarray(np.clip(pre, 0, 255).astype(np.uint8), "RGBA"),
        ((cell - nw) // 2, cell - margin - nh),
    )
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--cell", type=int, default=128)
    ap.add_argument("--margin", type=int, default=6)
    ap.add_argument("--light", help="输出浅墨版的目标色 R,G,B（暮色主题）")
    ap.add_argument("--no-watermark", action="store_true", help="源图没有供应商水印")
    args = ap.parse_args()

    light = None
    if args.light:
        light = tuple(int(v) for v in args.light.split(","))
        if len(light) != 3:
            raise SystemExit("--light 需要 R,G,B")

    cells = len(JOBS)
    atlas = Image.new("RGBA", (args.cell * cells, args.cell), (0, 0, 0, 0))
    for src, index, cut_bottom, name in JOBS:
        if not os.path.exists(src):
            raise SystemExit(f"源图不存在：{src}（提示词见 docs/theme/default/overlays.md）")
        rgb = np.asarray(Image.open(src).convert("RGB")).astype(np.float32)
        if not args.no_watermark:
            rgb = blank_watermark(rgb)
        if cut_bottom is not None:
            rgb = rgb[:cut_bottom]
        cell = fit_cell(keyed_rgba(rgb, light), args.cell, args.margin)
        atlas.paste(cell, (index * args.cell, 0))
        print(f"[{index}] {name} ← {src}")

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    atlas.save(args.out)
    print(f"saved {args.out} ({atlas.width}x{atlas.height})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
