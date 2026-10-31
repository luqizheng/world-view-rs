#!/usr/bin/env python3
"""POI 图钉「含义图形」归一化生成器（ADR-0093）。

把 `crates/wv-ui/assets/ui/poi_glyphs/*.svg`（Lucide 直取 + AI 生成补缺）
统一成 24×24 空间、白线稿风格的折线集，生成
`crates/wv-render/src/poi_glyphs.rs`（生成文件，勿手改）。

归一化规则：
- viewBox 任意（24 或 48）→ 线性缩放到 24 空间；y 翻转（SVG y 向下 →
  符号局部 y 向上）；
- 所有元素（path/circle/rect）统一转**闭合/开放折线**：填充路径取其轮廓
  （线稿化，与 Lucide stroke 家族一致）；曲线（C/S/Q/T/A）采样成短线段；
- 伸缩：内容包围盒等比适配到 20×20 并居中于 (12,12)——各图标墨量一致；
- 笔画宽在渲染侧统一（生成数据只存折线，无宽语义）。

跑法：python scripts/normalize_poi_glyphs.py（仓库根；纯标准库）。
"""

from __future__ import annotations

import math
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ASSET_DIR = Path("crates/wv-ui/assets/ui/poi_glyphs")
OUT_FILE = Path("crates/wv-render/src/poi_glyphs.rs")

# ---------- 路径解析 ----------

TOKEN = re.compile(r"([MmLlHhVvCcSsQqTtAaZz])|(-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)")


ARITY = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "S": 4, "Q": 4, "T": 2, "A": 7, "Z": 0}


def parse_path(d: str) -> list[list[tuple[float, float]]]:
    """SVG path d → 折线列表（子路径；闭合子路径末点 = 首点）。

    两遍法：先按命令元数把数字流切成显式命令组（隐式重复按 SVG 规范展开，
    M 的隐式重复按 L 处理），再逐命令展平采样。
    """
    tokens: list[tuple[str | None, float]] = []
    for m in TOKEN.finditer(d):
        if m.group(1):
            tokens.append((m.group(1), 0.0))
        else:
            tokens.append((None, float(m.group(2))))

    # 第一遍：展开隐式重复（任意命令后的裸数字 = 重复该命令；M 的重复按 L）。
    groups: list[tuple[str, list[float]]] = []
    i, n = 0, len(tokens)
    cur_cmd = ""
    while i < n:
        if tokens[i][0] is not None:
            cur_cmd = tokens[i][0]
            i += 1
            cmd_eff = cur_cmd
        else:
            if not cur_cmd:
                raise ValueError(f"命令流以数字开头: {d[:20]}")
            cmd_eff = "L" if cur_cmd in "Mm" else cur_cmd
        if cmd_eff in "Zz":
            groups.append((cmd_eff, []))
            continue
        groups.append((cmd_eff, []))
        pending = ARITY[cmd_eff.upper()]
        while i < n and tokens[i][0] is None and len(groups[-1][1]) < pending:
            groups[-1][1].append(tokens[i][1])
            i += 1
        if len(groups[-1][1]) != pending:
            raise ValueError(f"{cmd_eff} 参数不足（{d[:30]}…）")

    # 第二遍：逐命令展平。
    subs: list[list[tuple[float, float]]] = []
    cur: list[tuple[float, float]] = []
    cx = cy = 0.0
    sx = sy = 0.0
    prev_c2 = (0.0, 0.0)
    prev_q = (0.0, 0.0)
    prev_type = ""

    def flush():
        nonlocal cur
        if len(cur) >= 2:
            subs.append(cur)
        cur = []

    for cmd, a in groups:
        c = cmd.upper()
        rel = cmd.islower()
        if c == "M":
            x, y = a
            if rel:
                x, y = cx + x, cy + y
            flush()
            cx, cy, sx, sy = x, y, x, y
            cur = [(x, y)]
        elif c == "L":
            x, y = a
            if rel:
                x, y = cx + x, cy + y
            cx, cy = x, y
            cur.append((x, y))
        elif c == "H":
            x = a[0] + (cx if rel else 0.0)
            cx = x
            cur.append((cx, cy))
        elif c == "V":
            y = a[0] + (cy if rel else 0.0)
            cy = y
            cur.append((cx, cy))
        elif c == "C":
            p = a
            if rel:
                p = [v + (cx if k % 2 == 0 else cy) for k, v in enumerate(p)]
            c1, c2, pt = (p[0], p[1]), (p[2], p[3]), (p[4], p[5])
            cur.extend(flat_cubic((cx, cy), c1, c2, pt))
            prev_c2, prev_type = c2, "C"
            cx, cy = pt
        elif c == "S":
            p = a
            if rel:
                p = [v + (cx if k % 2 == 0 else cy) for k, v in enumerate(p)]
            c1 = mirror(prev_c2, (cx, cy)) if prev_type in "CS" else (cx, cy)
            c2, pt = (p[0], p[1]), (p[2], p[3])
            cur.extend(flat_cubic((cx, cy), c1, c2, pt))
            prev_c2, prev_type = c2, "S"
            cx, cy = pt
        elif c == "Q":
            p = a
            if rel:
                p = [v + (cx if k % 2 == 0 else cy) for k, v in enumerate(p)]
            q, pt = (p[0], p[1]), (p[2], p[3])
            cur.extend(flat_quad((cx, cy), q, pt))
            prev_q, prev_type = q, "Q"
            cx, cy = pt
        elif c == "T":
            p = a
            if rel:
                p = [v + (cx if k % 2 == 0 else cy) for k, v in enumerate(p)]
            q = mirror(prev_q, (cx, cy)) if prev_type in "QT" else (cx, cy)
            pt = (p[0], p[1])
            cur.extend(flat_quad((cx, cy), q, pt))
            prev_q, prev_type = q, "T"
            cx, cy = pt
        elif c == "A":
            rx, ry, rot, large, sweep, x, y = a
            if rel:
                x, y = cx + x, cy + y
            cur.extend(flat_arc((cx, cy), rx, ry, rot, large, sweep, (x, y)))
            cx, cy = x, y
            prev_type = "A"
        elif c == "Z":
            if cur:
                cur.append(cur[0])
            flush()
            cx, cy = sx, sy
            prev_type = "Z"
    flush()
    return subs


def mirror(ctrl: tuple[float, float], p: tuple[float, float]) -> tuple[float, float]:
    return (2 * p[0] - ctrl[0], 2 * p[1] - ctrl[1])


def flat_cubic(
    p0: tuple[float, float],
    c1: tuple[float, float],
    c2: tuple[float, float],
    p1: tuple[float, float],
    seg: int = 16,
) -> list[tuple[float, float]]:
    out = []
    for k in range(1, seg + 1):
        t = k / seg
        mt = 1 - t
        x = mt**3 * p0[0] + 3 * mt**2 * t * c1[0] + 3 * mt * t**2 * c2[0] + t**3 * p1[0]
        y = mt**3 * p0[1] + 3 * mt**2 * t * c1[1] + 3 * mt * t**2 * c2[1] + t**3 * p1[1]
        out.append((x, y))
    return out


def flat_quad(
    p0: tuple[float, float], q: tuple[float, float], p1: tuple[float, float], seg: int = 12
) -> list[tuple[float, float]]:
    out = []
    for k in range(1, seg + 1):
        t = k / seg
        mt = 1 - t
        x = mt**2 * p0[0] + 2 * mt * t * q[0] + t**2 * p1[0]
        y = mt**2 * p0[1] + 2 * mt * t * q[1] + t**2 * p1[1]
        out.append((x, y))
    return out


def flat_arc(
    p0: tuple[float, float],
    rx: float,
    ry: float,
    rot_deg: float,
    large: float,
    sweep: float,
    p1: tuple[float, float],
    seg: int = 16,
) -> list[tuple[float, float]]:
    """端点圆弧 → 圆心参数化 → 采样（SVG 规范 F.6.5 的标准换算）。"""
    if rx <= 0 or ry <= 0:
        return [p1]
    rx, ry = abs(rx), abs(ry)
    phi = math.radians(rot_deg % 360)
    cos_p, sin_p = math.cos(phi), math.sin(phi)
    dx, dy = (p0[0] - p1[0]) / 2, (p0[1] - p1[1]) / 2
    x1p = cos_p * dx + sin_p * dy
    y1p = -sin_p * dx + cos_p * dy
    lam = x1p**2 / rx**2 + y1p**2 / ry**2
    if lam > 1:
        s = math.sqrt(lam)
        rx, ry = rx * s, ry * s
    num = rx**2 * ry**2 - rx**2 * y1p**2 - ry**2 * x1p**2
    den = rx**2 * y1p**2 + ry**2 * x1p**2
    co = math.sqrt(max(0.0, num / den)) if den else 0.0
    if large == sweep:
        co = -co
    cxp = co * rx * y1p / ry
    cyp = -co * ry * x1p / rx
    cx = cos_p * cxp - sin_p * cyp + (p0[0] + p1[0]) / 2
    cy = sin_p * cxp + cos_p * cyp + (p0[1] + p1[1]) / 2

    def ang(ux: float, uy: float, vx: float, vy: float) -> float:
        dot = ux * vx + uy * vy
        n = math.hypot(ux, uy) * math.hypot(vx, vy)
        a = math.acos(max(-1.0, min(1.0, dot / n)))
        if ux * vy - uy * vx < 0:
            a = -a
        return a

    th1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dth = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not sweep and dth > 0:
        dth -= 2 * math.pi
    elif sweep and dth < 0:
        dth += 2 * math.pi
    out = []
    for k in range(1, seg + 1):
        t = th1 + dth * k / seg
        px = cx + rx * math.cos(t) * cos_p - ry * math.sin(t) * sin_p
        py = cy + rx * math.cos(t) * sin_p + ry * math.sin(t) * cos_p
        out.append((px, py))
    return out


# ---------- 元素 → 折线 ----------

SUBDIV = 24  # 圆 / 圆弧的近似段数（24 空间下足够圆滑）


def elem_polylines(el: ET.Element, tf) -> list[list[tuple[float, float]]]:
    """单个 SVG 元素 → 24 空间折线列表（已 y 翻转）。tf = (scale, 视图 w/h)。"""
    tag = el.tag.rsplit("}", 1)[-1]
    a = el.attrib
    out: list[list[tuple[float, float]]] = []

    def T(x: float, y: float) -> tuple[float, float]:
        # viewBox → 24 空间 + y 翻转。
        X, Y = tf[0] * x, tf[0] * y
        return (X, 24.0 - Y)

    if tag == "path":
        for sub in parse_path(a.get("d", "")):
            out.append([T(x, y) for x, y in sub])
    elif tag == "circle":
        cx, cy = float(a.get("cx", 0)), float(a.get("cy", 0))
        r = float(a.get("r", 0))
        pts = [(cx + r * math.cos(2 * math.pi * k / SUBDIV), cy + r * math.sin(2 * math.pi * k / SUBDIV)) for k in range(SUBDIV + 1)]
        out.append([T(x, y) for x, y in pts])
    elif tag == "rect":
        x, y = float(a.get("x", 0)), float(a.get("y", 0))
        w, h = float(a.get("width", 0)), float(a.get("height", 0))
        corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)]
        out.append([T(*p) for p in corners])
    elif tag in ("line", "polyline", "polygon"):
        pts_attr = a.get("points")
        if tag == "line":
            pts = [(float(a["x1"]), float(a["y1"])), (float(a["x2"]), float(a["y2"]))]
        elif pts_attr:
            vals = [float(v) for v in re.findall(r"-?(?:\d+\.?\d*|\.\d+)", pts_attr)]
            pts = list(zip(vals[::2], vals[1::2]))
        else:
            pts = []
        if len(pts) >= 2:
            if tag == "polygon":
                pts = pts + [pts[0]]
            out.append([T(*p) for p in pts])
    return out


def fit(subs: list[list[tuple[float, float]]]) -> list[list[tuple[float, float]]]:
    """内容包围盒 → 等比适配 20×20、居中 (12,12)。"""
    xs = [x for s in subs for x, _ in s]
    ys = [y for s in subs for _, y in s]
    if not xs:
        return subs
    minx, maxx, miny, maxy = min(xs), max(xs), min(ys), max(ys)
    w, h = maxx - minx, maxy - miny
    if w <= 1e-6 or h <= 1e-6:
        return subs
    scale = 20.0 / max(w, h)
    cx0, cy0 = (minx + maxx) / 2, (miny + maxy) / 2
    return [
        [(12 + (x - cx0) * scale, 12 + (y - cy0) * scale) for x, y in s] for s in subs
    ]


def collect(svg_path: Path) -> list[list[tuple[float, float]]]:
    root = ET.parse(svg_path).getroot()

    def strip(t: str) -> str:
        return t.rsplit("}", 1)[-1]

    vb = root.get("viewBox", "0 0 24 24").replace(",", " ").split()
    vx, vy, vw, vh = (float(v) for v in vb)
    scale = 24.0 / max(vw, vh)
    subs: list[list[tuple[float, float]]] = []

    def walk(el: ET.Element):
        tag = strip(el.tag)
        if tag in ("path", "circle", "rect", "line", "polyline", "polygon"):
            # 元素自己的 fill/stroke 不区分——线稿化后统一渲染。
            subs.extend(elem_polylines(el, (scale, (vw, vh))))
        for ch in el:
            walk(ch)

    walk(root)
    # 融合成单列表再 fit（包围盒要全局算）。
    return fit(subs)


def emit(subs: list[list[tuple[float, float]]]) -> str:
    lines = []
    for s in subs:
        pts = ", ".join(f"({x:.2f},{y:.2f})" for x, y in s)
        lines.append(f"        &[{pts}],")
    return "\n".join(lines)


def main() -> None:
    if not ASSET_DIR.is_dir():
        print(f"找不到 {ASSET_DIR}", file=sys.stderr)
        sys.exit(1)
    names = sorted(p.stem for p in ASSET_DIR.glob("*.svg"))
    body = []
    for name in names:
        subs = collect(ASSET_DIR / f"{name}.svg")
        body.append(f'    GlyphData {{\n        name: "{name}",\n        subpaths: &[\n{emit(subs)}\n        ],\n    }},')
    rs = (
        "//!\n"
        "//！本文件为**生成文件**——由 `scripts/normalize_poi_glyphs.py` 从\n"
        "//! `crates/wv-ui/assets/ui/poi_glyphs/*.svg` 归一化生成，勿手改；\n"
        "//! 改图形请改 SVG 源后重跑生成器。\n"
        "//!\n"
        "//! 归一化口径：24×24 空间（y 向上）、内容适配 20×20 居中、全元素\n"
        "//! 线稿化（填充取轮廓）——渲染侧统一按折线描边成白色三角形。\n"
        "//! 坐标采样值会撞上 π/τ 的近似值常量 lint——生成数据豁免。\n\n"
        "#![allow(clippy::approx_constant)]\n\n"
        "/// 一个含义图形：若干条折线（24 空间，y 向上，适配 20×20 居中）。\n"
        "pub struct GlyphData {\n"
        "    /// 稳定键（= SVG 文件名 stem；`PoiKind` → 图形映射用）。\n"
        "    pub name: &'static str,\n"
        "    /// 折线集（开放/闭合由末点是否重合判断，渲染侧不区分）。\n"
        "    pub subpaths: &'static [&'static [(f32, f32)]],\n"
        "}\n\n"
        "pub const GLYPHS: &[GlyphData] = &[\n"
        + "\n".join(body)
        + "\n];\n\n"
        "/// 按名取图形（缺省 `None`——调用方回退白芯圆点）。\n"
        "pub fn glyph(name: &str) -> Option<&'static GlyphData> {\n"
        "    GLYPHS.iter().find(|g| g.name == name)\n"
        "}\n"
    )
    # 生成文件的 //! 头里不能夹半角注释符，重写文件头（保持简单：全 //! 行）。
    rs = rs.replace("//!\n//！本文件", "//!\n//! 本文件")
    OUT_FILE.write_text(rs, encoding="utf-8", newline="\n")
    total = sum(len(s) for n in names for s in collect(ASSET_DIR / f"{n}.svg"))
    print(f"生成 {OUT_FILE}：{len(names)} 个图形，共 {total} 个采样点")


if __name__ == "__main__":
    main()
