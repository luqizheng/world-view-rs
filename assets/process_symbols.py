# -*- coding: utf-8 -*-
"""处理手绘水彩符号贴图：去白底、裁切、缩放到 §3.4 档位。"""
import sys
from pathlib import Path
from PIL import Image
import numpy as np

SRC_DIR = Path(r"D:\kudi-projects\worldview-rs\assets\水彩主题")
DST_DIR = Path(r"D:\kudi-projects\worldview-rs\crates\wv-render\assets\symbols")
DST_DIR.mkdir(parents=True, exist_ok=True)

# 档位：1×1 / 2×2 / 3×3 格（@ base 32px）
SIZES = [32, 64, 96]

# 白底检测阈值：RGB 都 > 240 且 alpha > 200 视为"白底像素"
WHITE_THRESHOLD = 240
ALPHA_THRESHOLD = 200

def remove_white_bg(img: Image.Image) -> Image.Image:
    """把纯白背景变透明。"""
    img = img.convert("RGBA")
    arr = np.array(img)
    r, g, b, a = arr[:, :, 0], arr[:, :, 1], arr[:, :, 2], arr[:, :, 3]
    # 白底判定：RGB 都 > 阈值 且 alpha 足够高
    white_mask = (r > WHITE_THRESHOLD) & (g > WHITE_THRESHOLD) & (b > WHITE_THRESHOLD) & (a > ALPHA_THRESHOLD)
    arr[white_mask, 3] = 0  # alpha = 0
    return Image.fromarray(arr, "RGBA")

def crop_to_content(img: Image.Image, block: int = 32, fill_ratio: float = 0.15) -> Image.Image:
    """裁切到**主体连通块**的最小外接矩形（块聚合 + 网格连通域）。

    **两轮修正教训**（2026-10-25）：
    1. 原版用「`alpha > 0` 的行列范围」——去白底后仍有 14% 的浅灰纸纹散落
       全画布，一个残留点就撑到约 1700²，树在 96px 里只剩 1/3。
    2. 「每行 2% 计数」仍不够（稀疏灰点每行恰好过线）。
    3. **本版**：先按 32px 块聚合成 64×64 布尔网格（块内 >=15% 才算有内容），
       再在网格上做 4-邻接 BFS 取**最大连通块**——树 / 城堡是最大块，
       右下角 AI 水印（「豆包AI生成」）与远处泼溅是孤立小块，自动排除。
       网格只有 64²，纯 Python BFS 开销可忽略。
    """
    arr = np.array(img)
    mask = (arr[:, :, 3] > 20).astype(np.float32)
    if not mask.any():
        return img
    h, w = mask.shape
    bh, bw = h // block, w // block
    if bh == 0 or bw == 0:  # 图比块还小，退回整幅
        return img
    grid = mask[: bh * block, : bw * block].reshape(bh, block, bw, block).mean(axis=(1, 3))
    solid = grid >= fill_ratio

    # 网格级连通域：取面积最大的块团（主体），排除水印 / 泼溅。
    best: list[tuple[int, int]] = []
    seen = np.zeros_like(solid, dtype=bool)
    for sy in range(bh):
        for sx in range(bw):
            if not solid[sy, sx] or seen[sy, sx]:
                continue
            stack = [(sy, sx)]
            seen[sy, sx] = True
            comp: list[tuple[int, int]] = []
            while stack:
                cy, cx = stack.pop()
                comp.append((cy, cx))
                for ny, nx in ((cy - 1, cx), (cy + 1, cx), (cy, cx - 1), (cy, cx + 1)):
                    if 0 <= ny < bh and 0 <= nx < bw and solid[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        stack.append((ny, nx))
            if len(comp) > len(best):
                best = comp
    if not best:
        return img
    ys = [c[0] for c in best]
    xs = [c[1] for c in best]
    top, bottom = min(ys) * block, (max(ys) + 1) * block
    left, right = min(xs) * block, (max(xs) + 1) * block
    return img.crop((int(left), int(top), int(right), int(bottom)))

def fit_square(img: Image.Image, size: int) -> Image.Image:
    """把内容缩放到正方形画布，保持宽高比，居中放置。

    **必须 `copy()` 后再 `thumbnail`**（2026-10-25 修）：`thumbnail` 是
    **原地**缩放且只缩不放。原版在 32/64/96 三档循环里复用同一个 `img`，
    第一轮就把源图缩到 32px——后两档只是在 64/96 画布上居中贴一张 32px 的
    图，三档输出内容尺寸完全相同（实测全为 32×29）。
    """
    work = img.copy()
    work.thumbnail((size, size), Image.LANCZOS)
    square = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    x = (size - work.width) // 2
    y = (size - work.height) // 2
    square.paste(work, (x, y), work)
    return square

def process_file(path: Path):
    print(f"处理: {path.name}")
    img = Image.open(path)
    print(f"  原始: {img.size} {img.mode}")
    
    # 1. 去白底
    img = remove_white_bg(img)
    
    # 2. 裁切到内容
    img = crop_to_content(img)
    print(f"  裁切后: {img.size}")
    
    # 3. 生成三个档位
    for size in SIZES:
        out = fit_square(img, size)
        stem = path.stem
        out_path = DST_DIR / f"{stem}_{size}.png"
        out.save(out_path, "PNG")
        print(f"  -> {out_path.name} ({out.size})")

def main():
    files = sorted(SRC_DIR.glob("*.png"))
    if not files:
        print("没有找到 PNG 文件")
        return
    for f in files:
        process_file(f)
    print("\n完成！")

if __name__ == "__main__":
    main()