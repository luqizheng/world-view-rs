//! 街区图层文档（2026-11-06 城市街区工具设计，D2/D4）：模拟城市式「框选
//! 长建筑」的区域实体——参数 + seed 留种，建筑为派生数据可整区重摇。
//!
//! 存储形态与 [`super::building::BuildingLayer`] 完全同构：一个
//! `LayerKind::District` 图层经 [`crate::map::Layer::source`] 指向一个
//! YAML 文件，文件内容即本模块的 [`DistrictLayer`]；`World.city_blocks`
//! 持可改可撤销的工作副本。
//!
//! ## 重摇语义（设计 D3）
//!
//! 重摇 = 清空该区 ring 内**全部建筑（含作者手摆的）**后按 (参数, seed)
//! 重新生成——无出身标记、不追问，误重摇靠 undo 兜底（`wv-edit` 的
//! `UpsertDistrictCommand` 一个事务一条 undo）。
//!
//! ## 生成算法（设计 §5）
//!
//! [`generate_buildings`] 是**纯函数**：同 (ring, params, seed, streets)
//! 必同输出（splitmix64 家族，与 ADR-0031 群系装饰同源）。v1 肌理 =
//! 格网对齐：ring 包围盒切 block 网格，格心在环内的格按占地率掷骰放
//! 矩形建筑；有街道的格生成率上调（沿街肌理）、建筑长轴对齐最近街段。
//!
//! ## 边界（不做什么）
//!
//! - **不做带洞环**（与势力几何同限）；ring 按简单多边形假设，坏环
//!   最多少生成、不 panic；
//! - **不做逐栋旋转拖拽**等编辑 UI（README §12-31(c) 另轮）；
//! - **不感知 IO / Bevy**（AGENTS 分层：wv-core 零依赖）。

use crate::base::geometry::Point;

/// 街区生成参数（设计 D2）：全部有缺省——表单滑条调之前就是能看的默认值。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DistrictParams {
    /// 目标密度（0..1，占地率）；缺省 0.35。
    #[serde(default = "default_density")]
    pub density: f32,
    /// 楼层带下界（米）；缺省 6（约 2 层）。
    #[serde(default = "default_height_min_m")]
    pub height_min_m: f32,
    /// 楼层带上界（米）；缺省 18（约 6 层）。
    #[serde(default = "default_height_max_m")]
    pub height_max_m: f32,
    /// 街区肌理：格网边长下界（米）；缺省 12。
    #[serde(default = "default_block_min_m")]
    pub block_min_m: f32,
    /// 格网边长上界（米）；缺省 30。
    #[serde(default = "default_block_max_m")]
    pub block_max_m: f32,
}

fn default_density() -> f32 {
    0.35
}
fn default_height_min_m() -> f32 {
    6.0
}
fn default_height_max_m() -> f32 {
    18.0
}
fn default_block_min_m() -> f32 {
    12.0
}
fn default_block_max_m() -> f32 {
    30.0
}

impl Default for DistrictParams {
    fn default() -> Self {
        DistrictParams {
            density: default_density(),
            height_min_m: default_height_min_m(),
            height_max_m: default_height_max_m(),
            block_min_m: default_block_min_m(),
            block_max_m: default_block_max_m(),
        }
    }
}

impl DistrictParams {
    /// 把越界参数夹回安全带（UI 滑条之外的脏数据兜底）：density ∈ [0,1]、
    /// 楼层带 min ≤ max、肌理带 min ≤ max 且为正。
    pub fn sanitized(self) -> Self {
        let density = self.density.clamp(0.0, 1.0);
        let (h_lo, h_hi) = ordered(self.height_min_m, self.height_max_m);
        let (b_lo, b_hi) = ordered(self.block_min_m, self.block_max_m);
        let b_lo = b_lo.max(1.0);
        let b_hi = b_hi.max(b_lo);
        DistrictParams {
            density,
            height_min_m: h_lo,
            height_max_m: h_hi,
            block_min_m: b_lo,
            block_max_m: b_hi,
        }
    }
}

fn ordered(a: f32, b: f32) -> (f32, f32) {
    (a.min(b), a.max(b))
}

/// 一个街区：闭合轮廓 + 生成参数 + 种子。
///
/// `ring` 是**本图层所属地图的局部坐标**（与 [`super::building::Building::ring`]
/// 同一口径），首尾点不重复（闭合由消费侧连接）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct District {
    pub id: crate::Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 区域多边形（简单多边形假设；带洞不支持）。
    pub ring: Vec<Point>,
    #[serde(default)]
    pub params: DistrictParams,
    /// 生成种子：同 (参数, seed) ⇒ 同建筑群。
    #[serde(default)]
    pub seed: u64,
}

/// 一份街区图层文件（`Layer.source` 指向的 YAML）的文档根。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DistrictLayer {
    /// 全部街区；顺序即绘制顺序（先画的在下）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub districts: Vec<District>,
}

impl DistrictLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按 ID 找街区。
    pub fn find(&self, id: &crate::Id) -> Option<&District> {
        self.districts.iter().find(|d| &d.id == id)
    }

    /// 插入或替换（按 ID 对齐），返回被替换的旧值。
    pub fn upsert(&mut self, district: District) -> Option<District> {
        if let Some(slot) = self.districts.iter_mut().find(|d| d.id == district.id) {
            let old = slot.clone();
            *slot = district;
            Some(old)
        } else {
            self.districts.push(district);
            None
        }
    }

    /// 按 ID 删除，返回被删的街区。
    pub fn remove(&mut self, id: &crate::Id) -> Option<District> {
        let idx = self.districts.iter().position(|d| &d.id == id)?;
        Some(self.districts.remove(idx))
    }
}

// ---------------------------------------------------------------------------
// 生成算法（纯函数；确定性 = splitmix64 家族）
// ---------------------------------------------------------------------------

/// 单区建筑数上限（设计 §5 规模守卫）：手滑框全图也只长 500 栋。
pub const MAX_BUILDINGS_PER_DISTRICT: usize = 500;
/// 沿街密度提升半径系数：距街 < 此值 × 格边长的格，生成率 ×1.5（钳 0.9）。
const STREET_BOOST_RADIUS_CELLS: f32 = 1.5;
const STREET_BOOST_FACTOR: f32 = 1.5;
const STREET_BOOST_CAP: f32 = 0.9;
/// 建筑在格内的占地率区间（短/长两轴各自独立采样）。
const FOOTPRINT_MIN: f32 = 0.55;
const FOOTPRINT_MAX: f32 = 0.85;

/// splitmix64（与 wv-terrain::forest 的 ADR-0031 家族同一公式，本地内联
/// 避免 wv-core 反向依赖 wv-terrain）。
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn next_unit(state: &mut u64) -> f32 {
    // 取高 24 位归一化到 [0, 1)：24 位精度对布局/高度采样绰绰有余。
    (splitmix64(state) >> 40) as f32 / 16_777_216.0
}

/// 射线法点在环内判定（even-odd；ring 简单多边形假设）。
pub fn point_in_ring(x: f32, y: f32, ring: &[Point]) -> bool {
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[j].x, ring[j].y);
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 点到线段的最短距离（沿街判定用）。
fn dist_point_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (vx, vy) = (bx - ax, by - ay);
    let len2 = vx * vx + vy * vy;
    if len2 < 1e-12 {
        return ((px - ax) * (px - ax) + (py - ay) * (py - ay)).sqrt();
    }
    let t = (((px - ax) * vx + (py - ay) * vy) / len2).clamp(0.0, 1.0);
    let (qx, qy) = (ax + t * vx, ay + t * vy);
    ((px - qx) * (px - qx) + (py - qy) * (py - qy)).sqrt()
}

/// 点到折线集合的最短距离（`streets` = 本图局部坐标折线）。
fn dist_to_streets(px: f32, py: f32, streets: &[Vec<Point>]) -> f32 {
    let mut best = f32::INFINITY;
    for line in streets {
        for pair in line.windows(2) {
            let d = dist_point_segment(px, py, pair[0].x, pair[0].y, pair[1].x, pair[1].y);
            if d < best {
                best = d;
            }
        }
    }
    best
}

/// 生成一个街区的建筑群（设计 §5；纯函数、确定性）。
///
/// 入参：
/// - `ring`：街区多边形（本图局部坐标）；
/// - `params`：生成参数（内部先 `sanitized`，脏数据兜底）；
/// - `seed`：随机种子；
/// - `streets`：本图全部街道折线（局部坐标；沿街判定与长轴对齐用），
///   无街道时退化为纯格网肌理。
///
/// 输出的 `Building.ring` 与输入同一坐标系；栋数 ≤
/// [`MAX_BUILDINGS_PER_DISTRICT`]。
pub fn generate_buildings(
    ring: &[Point],
    params: &DistrictParams,
    seed: u64,
    streets: &[Vec<Point>],
) -> Vec<super::building::Building> {
    use super::building::Building;
    let params = params.clone().sanitized();
    if ring.len() < 3 {
        return Vec::new();
    }
    let Some(bbox) = ring_bbox(ring) else {
        return Vec::new();
    };
    if bbox.2 <= bbox.0 || bbox.3 <= bbox.1 {
        return Vec::new();
    }

    let mut state = seed ^ 0x632B_E596_1A62_3D1D;
    // 肌理：格边长在参数带内各轴独立抽一次——整区统一（城市肌理的一致性）。
    let cell_w =
        params.block_min_m + (params.block_max_m - params.block_min_m) * next_unit(&mut state);
    let cell_h =
        params.block_min_m + (params.block_max_m - params.block_min_m) * next_unit(&mut state);
    if cell_w <= 0.0 || cell_h <= 0.0 {
        return Vec::new();
    }
    // 沿街判定半径：以格边长均值计。
    let street_radius = STREET_BOOST_RADIUS_CELLS * (cell_w + cell_h) * 0.5;

    let cols = (((bbox.2 - bbox.0) / cell_w).ceil() as usize).min(MAX_BUILDINGS_PER_DISTRICT * 4);
    let rows = (((bbox.3 - bbox.1) / cell_h).ceil() as usize).min(MAX_BUILDINGS_PER_DISTRICT * 4);

    let mut out = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            if out.len() >= MAX_BUILDINGS_PER_DISTRICT {
                return out;
            }
            let cx = bbox.0 + (col as f32 + 0.5) * cell_w;
            let cy = bbox.1 + (row as f32 + 0.5) * cell_h;
            if !point_in_ring(cx, cy, ring) {
                continue;
            }
            // 沿街提升：距街近的格生成率 ×1.5（钳 0.9）——沿街商铺肌理。
            let near_street =
                !streets.is_empty() && dist_to_streets(cx, cy, streets) <= street_radius;
            let density = if near_street {
                (params.density * STREET_BOOST_FACTOR).min(STREET_BOOST_CAP)
            } else {
                params.density
            };
            if next_unit(&mut state) >= density {
                continue; // 空地
            }
            // 占地率两轴独立采样 + 轻微位置抖动。
            let fw = FOOTPRINT_MIN + (FOOTPRINT_MAX - FOOTPRINT_MIN) * next_unit(&mut state);
            let fd = FOOTPRINT_MIN + (FOOTPRINT_MAX - FOOTPRINT_MIN) * next_unit(&mut state);
            let mut w = cell_w * fw;
            let mut d = cell_h * fd;
            let jx = (next_unit(&mut state) - 0.5) * cell_w * 0.2;
            let jy = (next_unit(&mut state) - 0.5) * cell_h * 0.2;
            let px = cx + jx;
            let py = cy + jy;
            // 高度在楼层带内采样。
            let height = params.height_min_m
                + (params.height_max_m - params.height_min_m) * next_unit(&mut state);
            // 长轴对齐最近街段（沿街对齐）；无街 / 未命中 → 长边朝 x。
            let mut angle = if w >= d {
                0.0
            } else {
                std::f32::consts::FRAC_PI_2
            };
            if !streets.is_empty() {
                if let Some(a) = nearest_street_angle(px, py, streets, street_radius) {
                    // 建筑长轴贴街向。保证 w 是长边。
                    if w < d {
                        std::mem::swap(&mut w, &mut d);
                    }
                    angle = a;
                }
            }
            out.push(rect_building(px, py, w, d, angle, height));
        }
    }
    out
}

/// 轴向矩形（可旋转）的建筑轮廓：中心 + 两边长 + 旋转角 + 高度。
fn rect_building(
    cx: f32,
    cy: f32,
    w: f32,
    d: f32,
    angle: f32,
    height_m: f32,
) -> super::building::Building {
    use super::building::Building;
    let (sin, cos) = angle.sin_cos();
    // 局部四角（未旋转）：长边 w 沿局部 x、短边 d 沿局部 y。
    let corners = [
        (-w / 2.0, -d / 2.0),
        (w / 2.0, -d / 2.0),
        (w / 2.0, d / 2.0),
        (-w / 2.0, d / 2.0),
    ];
    let ring = corners
        .iter()
        .map(|(lx, ly)| Point::new(cx + lx * cos - ly * sin, cy + lx * sin + ly * cos))
        .collect();
    Building {
        name: None,
        height_m,
        ring,
    }
}

/// ring 包围盒 `(min_x, min_y, max_x, max_y)`；空环回 `None`。
fn ring_bbox(ring: &[Point]) -> Option<(f32, f32, f32, f32)> {
    let first = ring.first()?;
    let mut bbox = (first.x, first.y, first.x, first.y);
    for p in &ring[1..] {
        bbox.0 = bbox.0.min(p.x);
        bbox.1 = bbox.1.min(p.y);
        bbox.2 = bbox.2.max(p.x);
        bbox.3 = bbox.3.max(p.y);
    }
    Some(bbox)
}

/// 距点最近街段的方向角（长轴对齐用）；超出 `radius` 回 `None`。
///
/// 方向取「与 x 轴夹角 ∈ (-π/2, π/2]」的规范向——矩形长轴对称，±π 等价。
fn nearest_street_angle(px: f32, py: f32, streets: &[Vec<Point>], radius: f32) -> Option<f32> {
    let mut best = (f32::INFINITY, 0.0_f32);
    for line in streets {
        for pair in line.windows(2) {
            let d = dist_point_segment(px, py, pair[0].x, pair[0].y, pair[1].x, pair[1].y);
            if d < best.0 {
                let angle = (pair[1].y - pair[0].y).atan2(pair[1].x - pair[0].x);
                best = (d, angle);
            }
        }
    }
    if best.0 > radius {
        return None;
    }
    // 规范化到 (-π/2, π/2]：长轴 ±π 对称。
    let mut a = best.1;
    while a > std::f32::consts::FRAC_PI_2 {
        a -= std::f32::consts::PI;
    }
    while a <= -std::f32::consts::FRAC_PI_2 {
        a += std::f32::consts::PI;
    }
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::geometry::Point;

    fn square_ring(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Point> {
        vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]
    }

    fn hstreet(y: f32) -> Vec<Point> {
        vec![Point::new(-1_000.0, y), Point::new(1_000.0, y)]
    }

    /// 同 (ring, params, seed, streets) 两次调用逐位一致（确定性锚点）。
    #[test]
    fn generate_is_deterministic_for_the_same_seed() {
        let ring = square_ring(0.0, 0.0, 200.0, 200.0);
        let params = DistrictParams::default();
        let streets = vec![hstreet(100.0)];
        let a = generate_buildings(&ring, &params, 42, &streets);
        let b = generate_buildings(&ring, &params, 42, &streets);
        assert_eq!(a, b, "同 seed 必同输出");
        assert!(!a.is_empty(), "默认密度下 200² 区应有产出");
    }

    /// 不同 seed → 高度序列不同（非严格证明，但能抓「种子没接进采样」的
    /// 低级错误）。
    #[test]
    fn different_seeds_produce_different_layouts() {
        let ring = square_ring(0.0, 0.0, 200.0, 200.0);
        let params = DistrictParams::default();
        let a = generate_buildings(&ring, &params, 1, &[]);
        let b = generate_buildings(&ring, &params, 2, &[]);
        let ha: Vec<u8> = a.iter().map(|b| b.height_m.to_bits() as u8).collect();
        let hb: Vec<u8> = b.iter().map(|b| b.height_m.to_bits() as u8).collect();
        assert_ne!(ha, hb, "不同 seed 的高度序列应不同");
    }

    /// 全部建筑质心落在环内（凹环也成立——用 L 形环验证射线法）。
    #[test]
    fn all_buildings_lie_inside_the_ring() {
        // L 形：右下角被切掉。
        let ring = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 50.0),
            Point::new(50.0, 50.0),
            Point::new(50.0, 100.0),
            Point::new(0.0, 100.0),
        ];
        let out = generate_buildings(&ring, &DistrictParams::default(), 7, &[]);
        assert!(!out.is_empty());
        for b in &out {
            let c = b.centroid().expect("矩形建筑必有质心");
            assert!(
                point_in_ring(c.x, c.y, &ring),
                "质心 ({}, {}) 出环",
                c.x,
                c.y
            );
        }
    }

    /// density = 0 → 空输出；density = 1 → 大产出（占地率掷骰全过）。
    #[test]
    fn density_zero_yields_nothing_and_one_yields_plenty() {
        let ring = square_ring(0.0, 0.0, 300.0, 300.0);
        let empty = DistrictParams {
            density: 0.0,
            ..DistrictParams::default()
        };
        assert!(generate_buildings(&ring, &empty, 9, &[]).is_empty());
        let full = DistrictParams {
            density: 1.0,
            ..DistrictParams::default()
        };
        let out = generate_buildings(&ring, &full, 9, &[]);
        assert!(
            out.len() > 20,
            "满密度 300² 区应长出大量建筑，得 {}",
            out.len()
        );
    }

    /// 规模守卫：极小 block + 大 ring → 栋数被 [`MAX_BUILDINGS_PER_DISTRICT`]
    /// 截断，不爆炸。
    #[test]
    fn building_count_is_capped() {
        let ring = square_ring(0.0, 0.0, 2_000.0, 2_000.0);
        let dense = DistrictParams {
            density: 1.0,
            block_min_m: 4.0,
            block_max_m: 4.0,
            ..DistrictParams::default()
        };
        let out = generate_buildings(&ring, &dense, 3, &[]);
        assert_eq!(out.len(), MAX_BUILDINGS_PER_DISTRICT, "应恰在上限截断");
    }

    /// 沿街对齐：贴横街的建筑长轴应近似水平（角度 ≈ 0 或 ±π）。
    #[test]
    fn buildings_near_a_street_align_with_it() {
        let ring = square_ring(0.0, 0.0, 200.0, 200.0);
        let streets = vec![hstreet(10.0)]; // 贴环北缘的横街
        let params = DistrictParams {
            density: 1.0,
            block_min_m: 20.0,
            block_max_m: 20.0,
            ..DistrictParams::default()
        };
        let out = generate_buildings(&ring, &params, 11, &streets);
        assert!(!out.is_empty());
        // 对所有距街 < radius 的建筑：长轴与 x 轴夹角 < 5°（横街水平）。
        let mut checked = 0;
        for b in &out {
            let c = b.centroid().unwrap();
            if (c.y - 10.0).abs() > STREET_BOOST_RADIUS_CELLS * 20.0 {
                continue;
            }
            let dx = b.ring[1].x - b.ring[0].x;
            let dy = b.ring[1].y - b.ring[0].y;
            let angle = dy.atan2(dx).abs();
            assert!(
                angle < 0.09 || (angle - std::f32::consts::PI).abs() < 0.09,
                "贴街建筑长轴应水平，得 {angle} rad"
            );
            checked += 1;
        }
        assert!(checked > 0, "fixture 应至少验到一栋贴街建筑");
    }

    /// 平图退化：环 < 3 点 → 空；参数越界 → sanitized 兜底不 panic。
    #[test]
    fn degenerate_inputs_yield_empty_not_panic() {
        assert!(generate_buildings(&[], &DistrictParams::default(), 1, &[]).is_empty());
        let two = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)];
        assert!(generate_buildings(&two, &DistrictParams::default(), 1, &[]).is_empty());
        let dirty = DistrictParams {
            density: 7.0,
            height_min_m: 50.0,
            height_max_m: 3.0,
            block_min_m: 0.0,
            block_max_m: -4.0,
        };
        let ring = square_ring(0.0, 0.0, 100.0, 100.0);
        let out = generate_buildings(&ring, &dirty, 1, &[]);
        for b in &out {
            assert!(b.height_m.is_finite(), "sanitized 后高度必有限");
        }
    }

    /// DistrictLayer 的 upsert / remove / find 基本语义（ID 对齐）。
    #[test]
    fn district_layer_upsert_and_remove_by_id() {
        let mut doc = DistrictLayer::new();
        let d1 = District {
            id: crate::Id::new_unchecked("d1"),
            name: None,
            ring: square_ring(0.0, 0.0, 10.0, 10.0),
            params: DistrictParams::default(),
            seed: 1,
        };
        assert_eq!(doc.upsert(d1.clone()), None, "首次插入无旧值");
        let mut d1b = d1.clone();
        d1b.seed = 2;
        assert_eq!(
            doc.upsert(d1b.clone()).map(|d| d.seed),
            Some(1),
            "替换回旧值"
        );
        assert_eq!(doc.districts.len(), 1, "ID 对齐不追加");
        assert_eq!(doc.find(&crate::Id::new_unchecked("d1")).unwrap().seed, 2);
        assert_eq!(doc.remove(&crate::Id::new_unchecked("d1")).unwrap().seed, 2);
        assert!(doc.districts.is_empty());
    }

    /// YAML round-trip：District / DistrictLayer 过 `load(save(w)) == w`
    /// （AGENTS §7.4 序列化契约）。
    #[test]
    fn district_layer_yaml_round_trip() {
        let doc = DistrictLayer {
            districts: vec![District {
                id: crate::Id::new_unchecked("blk_1"),
                name: Some("老城".into()),
                ring: square_ring(0.0, 0.0, 80.0, 60.0),
                params: DistrictParams {
                    density: 0.5,
                    ..DistrictParams::default()
                },
                seed: 0xDEAD_BEEF,
            }],
        };
        let text = serde_yaml::to_string(&doc).expect("序列化");
        let back: DistrictLayer = serde_yaml::from_str(&text).expect("反序列化");
        assert_eq!(back, doc);
    }
}
