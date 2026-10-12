# -*- coding: utf-8 -*-
"""生成 World View 瓦片图集 v2（方案 D：提升瓦片美术质量）。

输出 2048×2048 PNG，8 列 × 6 行、每格 256px，与 jrpg_v1.yaml 的网格规格一致
（代码零改动，只换 yaml 里的 atlas 路径）。

要点：
- 无缝平铺：所有噪声在「环面」上采样（格点坐标取模），瓦片四边天然连续，
  相邻格拼接不出缝——配合 terrain_view 的边界扰动，海岸线整段平滑。
- 风格基调对齐 palette.rs 的 CORE_TERRAIN 十色（美术 §1.4 色彩权威），
  每格在基色附近做明度/色相抖动，避免大色域平涂的「塑料感」。
- 山地/丘陵做法线光照（噪声梯度 · 光向），这是「Google 地图式立体感」
  的廉价近似。

运行：python scripts/gen_tile_atlas.py
依赖：numpy + Pillow（DSH 捆绑运行时自带）。
"""

import numpy as np
from PIL import Image, ImageDraw

TILE = 256          # 每格边长
COLS, ROWS = 8, 6   # 图集网格（必须与 jrpg_v1.yaml 一致）
W, H = TILE * COLS, TILE * ROWS


# ---------------------------------------------------------------------------
# 环面 value noise（周期 = period 个格点 → 无缝平铺）
# ---------------------------------------------------------------------------

def _hash(xi, yi, seed):
    h = (np.uint64(xi) * np.uint64(0x9E3779B97F4A7C15)
         ^ np.uint64(yi) * np.uint64(0xC2B2AE3D27D4EB4F)
         ^ np.uint64(seed))
    h = np.uint64(h ^ (h >> np.uint64(33)))
    h = h * np.uint64(0xFF51AFD7ED558CCD)
    h = np.uint64(h ^ (h >> np.uint64(33)))
    return (h & np.uint64(0xFFFFFFFF)).astype(np.float64) / np.float64(0xFFFFFFFF)


def vnoise(w, h, freq, seed, octaves=4):
    """周期性分形 value noise，输出 [0,1]，形状 (h, w)。freq = 基频（周期内格点数）。"""
    out = np.zeros((h, w))
    amp, total = 1.0, 0.0
    f = float(freq)
    for o in range(octaves):
        gx, gy = max(2, int(round(f))), max(2, int(round(f)))
        ys = np.arange(h) * (gy / h)
        xs = np.arange(w) * (gx / w)
        x0 = np.floor(xs).astype(np.int64)
        y0 = np.floor(ys).astype(np.int64)
        tx = xs - x0
        ty = ys - y0
        sx = tx * tx * (3 - 2 * tx)
        sy = ty * ty * (3 - 2 * ty)
        x0m, y0m = x0 % gx, y0 % gy
        x1m, y1m = (x0 + 1) % gx, (y0 + 1) % gy
        a = _hash(x0m[None, :], y0m[:, None], seed + o)
        b = _hash(x1m[None, :], y0m[:, None], seed + o)
        c = _hash(x0m[None, :], y1m[:, None], seed + o)
        d = _hash(x1m[None, :], y1m[:, None], seed + o)
        out += amp * (a + (b - a) * sx[None, :]
                      + (c - a) * sy[:, None]
                      + (a - b - c + d) * sx[None, :] * sy[:, None])
        total += amp
        amp *= 0.5
        f *= 2.0
    return out / total


def shade_by_gradient(height, strength=0.35, lx=-0.6, ly=-0.8):
    """噪声梯度的伪光照：返回乘法明度场 [1-strength, 1+strength]。"""
    gy, gx = np.gradient(height)
    g = np.stack([gx, gy, np.ones_like(gx)])
    g /= np.linalg.norm(g, axis=0)
    light = np.array([lx, ly, 0.7])
    light /= np.linalg.norm(light)
    dot = (g * light[:, None, None]).sum(axis=0)
    return 1.0 + np.clip(dot, -1, 1) * strength


def to_rgb(base, variation, luma):
    """base(Hz3) + variation(±) 再乘 luma → uint8 RGB。"""
    rgb = base[None, None, :] * (1.0 + variation[:, :, None])
    rgb *= luma[:, :, None]
    return np.clip(rgb * 255.0, 0, 255).astype(np.uint8)


# ---------------------------------------------------------------------------
# 各地形 painter（返回 256×256×3 uint8）
# ---------------------------------------------------------------------------

def p_water_deep():
    base = np.array([0.10, 0.30, 0.48])
    n = vnoise(TILE, TILE, 4, 11)
    # 长条浪痕：低频噪声的水平拉伸
    streak = vnoise(TILE, TILE // 8, 6, 12, 2)
    streak = np.repeat(streak, 8, axis=0)[:TILE]
    luma = 0.85 + 0.30 * n + 0.18 * (streak - 0.5)
    var = 0.10 * (n - 0.5)
    return to_rgb(base, var, np.clip(luma, 0.5, 1.3))


def p_water_shallow():
    base = np.array([0.26, 0.58, 0.76])
    n = vnoise(TILE, TILE, 6, 21)
    # 焦散网：高频噪声脊线提亮
    web = 1.0 - np.abs(vnoise(TILE, TILE, 24, 22, 3) * 2 - 1)
    luma = 0.92 + 0.16 * n + 0.22 * web ** 3
    return to_rgb(base, 0.06 * (n - 0.5), np.clip(luma, 0.6, 1.25))


def p_sand():
    base = np.array([0.85, 0.77, 0.56])
    grain = vnoise(TILE, TILE, 48, 31, 2) - 0.5
    ripple = vnoise(TILE, TILE // 4, 8, 32, 2)
    ripple = np.repeat(ripple, 4, axis=0)[:TILE]
    luma = 0.95 + 0.10 * (ripple - 0.5) + 0.06 * grain
    return to_rgb(base, 0.05 * grain, np.clip(luma, 0.7, 1.15))


def p_grass():
    base = np.array([0.50, 0.66, 0.36])
    mottle = vnoise(TILE, TILE, 8, 41)
    speck = vnoise(TILE, TILE, 64, 42, 2) - 0.5
    luma = 0.90 + 0.22 * (mottle - 0.5) + 0.08 * speck
    # 局部偏黄的干草斑
    dry = (vnoise(TILE, TILE, 5, 43) > 0.62).astype(np.float64) * 0.5
    rgb = to_rgb(base, 0.08 * (mottle - 0.5), np.clip(luma, 0.6, 1.25)).astype(np.float64)
    rgb[:, :, 0] += dry * 26
    rgb[:, :, 1] += dry * 8
    rgb[:, :, 2] -= dry * 10
    return np.clip(rgb, 0, 255).astype(np.uint8)


def p_forest():
    base = np.array([0.24, 0.42, 0.23])
    n = vnoise(TILE, TILE, 10, 51)
    luma = 0.88 + 0.24 * (n - 0.5)
    img = to_rgb(base, 0.10 * (n - 0.5), np.clip(luma, 0.6, 1.2))
    # 树冠：随机深绿圆 + 轻微亮边（俯视树冠的廉价画法）。
    # 圆要压暗、数量克制——太亮太密会像「波点」，不像俯视森林。
    pil = Image.fromarray(img)
    d = ImageDraw.Draw(pil)
    rng = np.random.default_rng(51)
    for _ in range(70):
        cx, cy = rng.uniform(0, TILE, 2)
        r = rng.uniform(6, 15)
        sh = rng.uniform(0.70, 0.88)
        col = tuple(int(255 * c) for c in (0.13 * sh * 2, 0.36 * sh * 2, 0.16 * sh * 2))
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=col)
        # 单侧月牙亮边：比整圈 outline 更像受光树冠
        lw = max(2, int(r * 0.25))
        d.arc([cx - r, cy - r, cx + r, cy + r], 200, 340,
              fill=(70, 110, 64), width=lw)
    return np.array(pil)


def p_hills():
    base = np.array([0.55, 0.58, 0.35])
    h = vnoise(TILE, TILE, 5, 61, 4)
    luma = shade_by_gradient(h, 0.30)
    return to_rgb(base, 0.08 * (h - 0.5), np.clip(luma, 0.55, 1.35))


def p_mountain():
    base = np.array([0.48, 0.42, 0.37])
    # 双尺度：低频定山体大势，高频加岩石碎褶——纯低频的光照是「云朵」感。
    h = 0.45 * vnoise(TILE, TILE, 4, 71, 4) + 0.55 * vnoise(TILE, TILE, 14, 73, 3)
    luma = shade_by_gradient(h, 0.75)
    img = to_rgb(base, 0.06 * (h - 0.5), np.clip(luma, 0.35, 1.4)).astype(np.float64)
    # 雪线：只落在最高处且按高频碎化（阈值抬到 0.74，避免大片「云盖」）
    snow = np.clip((h - 0.74) / 0.14, 0, 1) ** 1.2
    for c, target in enumerate((0.93, 0.95, 0.97)):
        img[:, :, c] = img[:, :, c] * (1 - snow) + target * 255 * snow
    return np.clip(img, 0, 255).astype(np.uint8)


def p_snow():
    base = np.array([0.90, 0.93, 0.95])
    n = vnoise(TILE, TILE, 8, 81)
    crunch = vnoise(TILE, TILE, 48, 82, 2) - 0.5
    luma = 0.93 + 0.10 * (n - 0.5) + 0.05 * crunch
    return to_rgb(base, 0.03 * (n - 0.5), np.clip(luma, 0.75, 1.1))


def p_marsh():
    base = np.array([0.35, 0.42, 0.28])
    n = vnoise(TILE, TILE, 6, 91)
    pool = (vnoise(TILE, TILE, 12, 92) > 0.60).astype(np.float64)
    luma = 0.85 + 0.22 * (n - 0.5)
    img = to_rgb(base, 0.10 * (n - 0.5), np.clip(luma, 0.6, 1.2)).astype(np.float64)
    # 死水斑：偏蓝的暗斑 + 亮反光点
    water = np.array([0.20, 0.34, 0.36]) * 255
    for c in range(3):
        img[:, :, c] = img[:, :, c] * (1 - pool[:, :]) + water[c] * pool[:, :]
    glint = (vnoise(TILE, TILE, 40, 93, 2) > 0.78).astype(np.float64) * pool
    img += glint[:, :, None] * np.array([80, 90, 90])[None, None, :]
    return np.clip(img, 0, 255).astype(np.uint8)


def p_road():
    base = np.array([0.72, 0.62, 0.45])
    grain = vnoise(TILE, TILE, 40, 101, 2) - 0.5
    ruts = vnoise(TILE, TILE // 6, 6, 102, 2)
    ruts = np.tile(np.repeat(ruts, 6, axis=0)[:TILE], (1, 1))[:TILE]
    if ruts.shape[0] < TILE:
        ruts = np.pad(ruts, ((0, TILE - ruts.shape[0]), (0, 0)), mode="wrap")
    luma = 0.93 + 0.10 * grain + 0.14 * (ruts - 0.5)
    return to_rgb(base, 0.06 * grain, np.clip(luma, 0.65, 1.2))


# kind → 图集格位（与 jrpg_v1.yaml 的 tiles 映射一致）
KIND_TILES = {
    0: ((0, 0), p_water_deep),
    1: ((3, 0), p_water_shallow),
    2: ((5, 0), p_sand),
    3: ((0, 2), p_grass),
    4: ((4, 1), p_forest),
    5: ((5, 2), p_hills),
    6: ((0, 3), p_mountain),
    7: ((6, 4), p_snow),
    8: ((2, 4), p_marsh),
    9: ((3, 5), p_road),
}


def main():
    atlas = np.zeros((H, W, 3), dtype=np.uint8)
    for kind, ((col, row), painter) in KIND_TILES.items():
        tile = painter()
        y0, x0 = row * TILE, col * TILE
        atlas[y0:y0 + TILE, x0:x0 + TILE] = tile
    out = Image.fromarray(atlas, "RGB")
    path = "crates/wv-app/assets/tilesets/jrpg_atlas_v2.png"
    out.save(path, optimize=True)
    print(f"written {path} ({W}x{H})")


if __name__ == "__main__":
    main()
