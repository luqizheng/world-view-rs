# -*- coding: utf-8 -*-
# TEMP(资产处理工具·一次性)：豆包生成图 → 透明背景 256² 符号。
# 白底键控用「边缘 flood fill」而不是全局色距——城堡的浅色石墙、市镇的
# 白墙都在距白 30 以内，全局键控会把墙挖穿；从图边缘灌入的 flood 只会
# 吃掉与外部连通的背景，内部浅色因轮廓线阻隔而保留。
import numpy as np
from collections import deque
from PIL import Image

JOBS = [
    (r'C:/Users/leo/Downloads/（市镇）.png',
     'crates/wv-app/assets/symbols/town.png'),
]
TARGET = 256          # 入库边长
BG_DIST = 38.0        # 判定「背景白」的色距（欧氏，0-441）
SOFT_BAND = 22.0      # alpha 渐变带宽度（抗锯齿软边）
WM_X, WM_Y = 0.80, 0.88  # 水印区域右下角阈值（比例）

for src, dst in JOBS:
    im = Image.open(src).convert('RGB')
    a = np.asarray(im).astype(np.float32)
    h, w = a.shape[:2]

    # 1) 水印区域涂白（右下角，豆包 AI 生成 文字）。
    a[int(h * WM_Y):, int(w * WM_X):, :] = 255.0

    # 2) 背景色距 → 「确定背景」掩码（仅用于 flood 种子扩展）。
    dist = np.sqrt(((a - 255.0) ** 2).sum(axis=2))
    bg_like = dist < BG_DIST

    # 3) 从四边 flood：与边缘连通的 bg_like 才是背景。
    flood = np.zeros((h, w), dtype=bool)
    dq = deque()
    for x in range(w):
        for y in (0, h - 1):
            if bg_like[y, x] and not flood[y, x]:
                flood[y, x] = True
                dq.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if bg_like[y, x] and not flood[y, x]:
                flood[y, x] = True
                dq.append((y, x))
    while dq:
        y, x = dq.popleft()
        if y > 0 and bg_like[y - 1, x] and not flood[y - 1, x]:
            flood[y - 1, x] = True; dq.append((y - 1, x))
        if y < h - 1 and bg_like[y + 1, x] and not flood[y + 1, x]:
            flood[y + 1, x] = True; dq.append((y + 1, x))
        if x > 0 and bg_like[y, x - 1] and not flood[y, x - 1]:
            flood[y, x - 1] = True; dq.append((y, x - 1))
        if x < w - 1 and bg_like[y, x + 1] and not flood[y, x + 1]:
            flood[y, x + 1] = True; dq.append((y, x + 1))

    # 4) alpha：flood 区按色距做软边（距主体越近越不透明），非 flood 区全不透明。
    alpha = np.where(
        flood,
        np.clip((dist - (BG_DIST - SOFT_BAND)) / SOFT_BAND, 0.0, 1.0),
        1.0,
    )
    # flood 但色距已超软带（不该发生，保险）→ 不透明。
    rgba = np.dstack([a, alpha * 255.0]).astype(np.uint8)

    # 5) 内容 bbox（alpha > 8）。
    ys, xs = np.where(rgba[:, :, 3] > 8)
    x0, x1, y0, y1 = xs.min(), xs.max(), ys.min(), ys.max()

    # 6) pad 成正方形、内容底部居中。
    cw, ch = x1 - x0 + 1, y1 - y0 + 1
    side = max(cw, ch)
    canvas = np.zeros((side, side, 4), dtype=np.uint8)
    sub = rgba[y0:y1 + 1, x0:x1 + 1]
    ox = (side - cw) // 2
    oy = side - ch  # 底对齐
    canvas[oy:oy + ch, ox:ox + cw] = sub

    out = Image.fromarray(canvas).resize((TARGET, TARGET), Image.LANCZOS)
    out.save(dst)
    print(f'{dst}: content {cw}x{ch} -> {TARGET}x{TARGET} (opaque {np.mean(canvas[:,:,3] > 8) * 100:.0f}%)')
