//! 零依赖（serde_json 之外）GeoJSON 工具（wv-gen-demo 专用）。
//!
//! 只支持本工具需要的子集：FeatureCollection + Polygon / MultiPolygon /
//! LineString / MultiLineString。另附栅格化小工具（扫描线填充 / 点到折线
//! 距离 / 折线刷写），与解析同放此处。

use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct FeatureCollection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    geometry: Option<Geometry>,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "coordinates", rename_all = "PascalCase")]
enum Geometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
    LineString(Vec<[f64; 2]>),
    MultiLineString(Vec<Vec<[f64; 2]>>),
}

pub struct F {
    pub geom: G,
}

pub enum G {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
    LineString(Vec<[f64; 2]>),
    MultiLineString(Vec<Vec<[f64; 2]>>),
}

/// 读 GeoJSON FeatureCollection → 几何列表（null geometry 的 feature 跳过）。
pub fn read(path: &Path) -> Result<Vec<F>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let fc: FeatureCollection =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    Ok(fc
        .features
        .into_iter()
        .filter_map(|f| {
            let geom = match f.geometry? {
                Geometry::Polygon(rs) => G::Polygon(rs),
                Geometry::MultiPolygon(rs) => G::MultiPolygon(rs),
                Geometry::LineString(ls) => G::LineString(ls),
                Geometry::MultiLineString(ls) => G::MultiLineString(ls),
            };
            Some(F { geom })
        })
        .collect())
}

/// 扫描线多边形填充（odd-even 规则）。
pub fn fill_polygon(buf: &mut [u8], val: u8, ring: &[[i32; 2]], w: i32, h: i32) {
    if ring.is_empty() {
        return;
    }
    let (y_min, y_max) = ring.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| {
        (lo.min(p[1]), hi.max(p[1]))
    });
    for y in y_min..=y_max {
        let mut xs: Vec<i32> = Vec::new();
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if (a[1] <= y && b[1] > y) || (b[1] <= y && a[1] > y) {
                let t = (y - a[1]) as f32 / (b[1] - a[1]) as f32;
                let x = (a[0] as f32 + (b[0] - a[0]) as f32 * t).round() as i32;
                xs.push(x);
            }
        }
        xs.sort_unstable();
        for pair in xs.chunks_exact(2) {
            for x in pair[0]..=pair[1] {
                if x >= 0 && x < w && y >= 0 && y < h {
                    let idx = (y as u32 * w as u32 + x as u32) as usize;
                    buf[idx] = val;
                }
            }
        }
    }
}

/// 格点到一组 ring（折线集合）的最近距离（格单位）。
pub fn min_dist_to_rings(x: i32, y: i32, rings: &[Vec<[i32; 2]>]) -> i32 {
    let mut best = i32::MAX;
    for ring in rings {
        for seg in ring.windows(2) {
            best = best.min(point_to_seg_dist(x, y, seg[0], seg[1]));
        }
    }
    best
}

fn point_to_seg_dist(px: i32, py: i32, a: [i32; 2], b: [i32; 2]) -> i32 {
    let (x0, y0) = (a[0] as f32, a[1] as f32);
    let (x1, y1) = (b[0] as f32, b[1] as f32);
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len2 = dx * dx + dy * dy;
    let (px, py) = (px as f32, py as f32);
    if len2 < 1.0 {
        return ((px - x0).powi(2) + (py - y0).powi(2)).sqrt() as i32;
    }
    let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
    let qx = x0 + t * dx;
    let qy = y0 + t * dy;
    ((px - qx).powi(2) + (py - qy).powi(2)).sqrt() as i32
}

/// 折线刷写（i16 通道：val < 0 时取更低值——河床语义）。
pub fn draw_line_i16(buf: &mut [i16], val: i16, a: [i32; 2], b: [i32; 2], w: i32, h: i32) {
    let (x0, y0) = (a[0], a[1]);
    let (x1, y1) = (b[0], b[1]);
    let radius = 1;
    let steps = (x0 - x1).abs().max((y0 - y1).abs());
    if steps == 0 {
        return;
    }
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let x = (x0 as f32 * (1.0 - t) + x1 as f32 * t).round() as i32;
        let y = (y0 as f32 * (1.0 - t) + y1 as f32 * t).round() as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || xx >= w || yy < 0 || yy >= h {
                    continue;
                }
                let idx = (yy as u32 * w as u32 + xx as u32) as usize;
                if val < 0 {
                    if buf[idx] > val {
                        buf[idx] = val;
                    }
                } else {
                    buf[idx] = val;
                }
            }
        }
    }
}

/// 折线刷写（u8 通道：biome 覆盖）。
pub fn draw_line_u8(buf: &mut [u8], val: u8, a: [i32; 2], b: [i32; 2], w: i32, h: i32) {
    let (x0, y0) = (a[0], a[1]);
    let (x1, y1) = (b[0], b[1]);
    let radius = 1;
    let steps = (x0 - x1).abs().max((y0 - y1).abs());
    if steps == 0 {
        return;
    }
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let x = (x0 as f32 * (1.0 - t) + x1 as f32 * t).round() as i32;
        let y = (y0 as f32 * (1.0 - t) + y1 as f32 * t).round() as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || xx >= w || yy < 0 || yy >= h {
                    continue;
                }
                let idx = (yy as u32 * w as u32 + xx as u32) as usize;
                buf[idx] = val;
            }
        }
    }
}
