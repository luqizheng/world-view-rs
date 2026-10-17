#!/usr/bin/env python3
"""把 AI 生成的地图符号图归一化成主题包可用的 RGBA 资产。

用法：
  python scripts/normalize_overlay_asset.py SRC DST [选项]

选项：
  --crop X0,X1,Y0,Y1   手动裁剪（源图像素）；省略则按内容包围盒自动裁
  --cut-bottom Y       只保留 y < Y 的内容；用于切掉地面阴影与水印（推荐先看行密度）
  --light R,G,B        输出浅墨版（给 Dusk 等暗色主题）；省略 = 保留原墨色（Daylight）
  --alpha-gain G       日间版 alpha 增益，默认 1.35
  --size N             画布边长，默认 128
  --margin M           内容与画布边缘的留白，默认 5

为什么需要浅墨版：Bevy 材质 tint 是乘法，深墨乘浅色仍是深墨。暗色主题必须换
浅墨资产，不能靠 symbol_color 提亮（spec §6.6）。

白底转 alpha 用 color-to-alpha：alpha = 1 - min(R,G,B)/bg，再反预乘恢复墨色；
浅墨版改用陡化曲线，压掉淡雾、保留线稿。

依赖：Pillow + numpy（仅开发期，不进 Rust 运行时）。
"""

import argparse
import os
import sys

import numpy as np
from PIL import Image

BG = 250.0  # 源图白底实测亮度（树 a：249–251）


def parse_crop(s: str):
    parts = [int(v) for v in s.split(",")]
    if len(parts) != 4:
        raise argparse.ArgumentTypeError("--crop 需要 X0,X1,Y0,Y1")
    return parts


def content_bbox(alpha: np.ndarray, cut_bottom: int) -> tuple[int, int, int, int]:
    ink = alpha[:cut_bottom] > 0.04
    ys, xs = np.where(ink)
    if len(xs) == 0:
        raise SystemExit("裁切后没有内容——检查 --crop / --cut-bottom")
    return int(xs.min()), int(xs.max()) + 1, int(ys.min()), int(ys.max()) + 1


def fit_canvas(rgba: np.ndarray, size: int, margin: int) -> Image.Image:
    img = Image.fromarray(rgba, "RGBA")
    cw, ch = img.size
    inner = size - margin * 2
    scale = inner / max(cw, ch)
    nw, nh = max(1, int(round(cw * scale))), max(1, int(round(ch * scale)))
    # 预乘后缩放，避免透明边缘出现暗边
    pre = np.asarray(img).astype(np.float32)
    pre[..., :3] *= pre[..., 3:4] / 255.0
    small = Image.fromarray(pre.astype(np.uint8), "RGBA").resize((nw, nh), Image.LANCZOS)
    pre = np.asarray(small).astype(np.float32)
    a = pre[..., 3:4] / 255.0
    pre[..., :3] = np.where(a > 0, pre[..., :3] / np.maximum(a, 1e-6), 0)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(
        Image.fromarray(np.clip(pre, 0, 255).astype(np.uint8), "RGBA"),
        ((size - nw) // 2, (size - nh) // 2),
    )
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("dst")
    ap.add_argument("--crop", type=parse_crop)
    ap.add_argument("--cut-bottom", type=int)
    ap.add_argument("--light")
    ap.add_argument("--alpha-gain", type=float, default=1.35)
    ap.add_argument("--size", type=int, default=128)
    ap.add_argument("--margin", type=int, default=5)
    args = ap.parse_args()

    rgb = np.asarray(Image.open(args.src).convert("RGB")).astype(np.float32)
    h = rgb.shape[0]
    cut = args.cut_bottom if args.cut_bottom is not None else h
    if not (0 < cut <= h):
        raise SystemExit(f"--cut-bottom 越界：{cut}（图高 {h}）")

    alpha = np.clip((BG - rgb.min(axis=2)) / BG, 0, 1)

    if args.crop:
        x0, x1, y0, y1 = args.crop
    else:
        x0, x1, y0, y1 = content_bbox(alpha, cut)
        y1 = min(y1, cut)

    crop_rgb = rgb[y0:y1, x0:x1]
    crop_a = alpha[y0:y1, x0:x1]

    if args.light:
        light = np.array([int(v) for v in args.light.split(",")], dtype=np.float32)
        if light.shape != (3,):
            raise SystemExit("--light 需要 R,G,B")
        a = np.clip((crop_a - 0.18) / 0.55, 0, 1) ** 0.85
        # 浅墨版的目标色本身就是最终 RGB（不经过白底反预乘，
        # 否则低 alpha 处会被算成比目标色更暗的颜色）。
        out_rgb = np.full_like(crop_rgb, 1.0) * light
    else:
        a = np.clip(crop_a * args.alpha_gain, 0, 1)
        with np.errstate(divide="ignore", invalid="ignore"):
            out_rgb = np.where(
                a[..., None] > 0,
                (crop_rgb - BG * (1 - a[..., None])) / np.maximum(a[..., None], 1e-6),
                BG,
            )
        out_rgb = np.clip(out_rgb, 0, 255)

    a[a < 0.04] = 0.0

    canvas = fit_canvas(
        np.dstack([out_rgb, a * 255]).astype(np.uint8), args.size, args.margin
    )
    os.makedirs(os.path.dirname(os.path.abspath(args.dst)), exist_ok=True)
    canvas.save(args.dst)
    print(f"saved {args.dst} ({args.size}x{args.size})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
