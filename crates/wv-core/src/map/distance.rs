//! 距离维度：地点图、边、交通工具、行程时间计算、路径搜索。
//!
//! 核心决策（§5.3 距离不存结论）：只存**参数**（距离、地形、速度、条件），
//! 行程时间永远由计算得出。交通工具或地形一改，所有结论自动更新。

use std::collections::HashMap;

use crate::{base::error::CoreError, base::id::Id};

/// 交通工具。对应 `distances/vehicles.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vehicle {
    pub id: Id,
    pub name: String,
    /// 瞬时交通工具（如传送阵）。`true` 时不参与速度公式，按零耗时边处理。
    #[serde(default)]
    pub instant: bool,
    /// 基础速度 km/h。`instant` 为 `true` 时可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kmh: Option<f32>,
    /// 每日有效行驶时长（小时）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_hours: Option<f32>,
    /// 地形速度修正系数。
    #[serde(default)]
    pub terrain_modifiers: HashMap<String, f32>,
    /// 瞬时交通工具是否要求两端都有对应设施。
    #[serde(default)]
    pub requires_endpoints: bool,
    /// 前置条件（条件表达式引擎待定，见 §13）。此处先存裸字符串。
    #[serde(default)]
    pub requirements: Vec<String>,
}

impl Vehicle {
    /// 在给定地形上的有效日速度（km/天）。
    ///
    /// 公式（ADR-0024，单位修正）：
    /// ```text
    /// 有效日速度 = speed_kmh × terrain_mult × daily_hours   [km/日]
    /// ```
    /// 旧式 `× daily_hours / 24` 得到的是 km/h 量纲，`segment_days` 因之
    /// 放大 24 倍（2026-10-17 评审发现；ADR-0024 记录修正与影响面）。
    pub fn effective_daily_speed(&self, terrain: &str) -> Option<f32> {
        if self.instant {
            return None;
        }
        let speed = self.speed_kmh?;
        let hours = self.daily_hours?;
        let mult = self.terrain_modifiers.get(terrain).copied().unwrap_or(1.0);
        Some(speed * mult * hours)
    }

    /// 单段耗时（天）。返回 `None` 表示该工具无法使用（缺参数或瞬时）。
    pub fn segment_days(&self, distance_km: f32, terrain: &str) -> Option<f32> {
        if self.instant {
            return Some(0.0);
        }
        let daily = self.effective_daily_speed(terrain)?;
        if daily <= 0.0 {
            return None;
        }
        Some(distance_km / daily)
    }
}

/// 地点边。对应 `distances/edges.yaml`。
///
/// 同一对地点可有多条边（官道 vs 山道），路径搜索按时间最优选择。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaceEdge {
    pub id: Id,
    pub a: Id,
    pub b: Id,
    pub distance_km: f32,
    pub terrain: String,
    #[serde(default = "default_true")]
    pub bidirectional: bool,
    /// 呈现级别（ADR-0047 D1，与 [`Place::rank`] 同约定）：数值越大越重要，
    /// 渲染层按「视野档位 × 最低级别」决定这条边画不画
    /// （Nation ≥ 3 / Region ≥ 2 / Edit 全见）。
    ///
    /// 词汇（GLOSSARY「呈现级别」，与 `Place::rank` 同一张表）：
    /// `3` 国家级干线、`2` 区域级、`1` 局部道路、`0` 仅编辑级可见。
    ///
    /// `None`（缺省）= 按 `distance_km` 折算（[`Self::effective_rank`]）：
    /// ≥ 100 km → 3、≥ 30 km → 2、否则 → 1——长边是城际干线的天然代理，
    /// 导入工程（Overture 路口网）不改 YAML 即获得主干优先的呈现。折算规则
    /// 是冻结契约（测试 `place_edge_effective_rank_folds_by_distance` 钉死），
    /// 改动它等于改旧工程的可视行为。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u8>,
}

impl PlaceEdge {
    /// 有效呈现级别（ADR-0047 D1 的折算入口，渲染层唯一可用的级别读法）。
    ///
    /// 显式 `rank` 原样返回；`None` 按距离折算：≥ 100 km → 3（城际干线，
    /// Nation 档即可见）、≥ 30 km → 2（城镇间）、否则 → 1（局部，仅
    /// Region 及更近档可见）。
    pub fn effective_rank(&self) -> u8 {
        match self.rank {
            Some(r) => r,
            None if self.distance_km >= 100.0 => 3,
            None if self.distance_km >= 30.0 => 2,
            None => 1,
        }
    }
}

fn default_true() -> bool {
    true
}

/// 地点在地图上的**符号类别**（ADR-0064）。
///
/// 与 [`Place::biome`] 正交：`biome` 说「这块地是什么地形」（森林 / 山地 /
/// 水域），`kind` 说「这个点在地图上代表什么」（餐饮 / 医疗 / 车站）。
/// 专业地图的 POI 图标按后者分色——`biome` 表达不了「同是平原上的饭店和医院」。
///
/// **封闭枚举而非自由字符串**：图标集是有限集，自由串只会带来拼错与静默
/// 回退；新增类别必须同时给出图标/配色，这正是封闭枚举要逼出的决策。
///
/// 缺省（旧 YAML 无此字段）由渲染层折算：有 `w/h` 的聚落 → `Settlement`，
/// 其余 → `Other`（回退既有红点白环）——零迁移。
///
/// **注意**：带 `biome` 的地点不生成 POI 点标（它们是覆盖物 / 标签的表现对象，
/// 见 `wv-render::world_projection::place_dot_for`），所以山峰 / 港口这类
/// 「biome 型地物」要出现在 POI 层，必须显式写 `kind`——这是 ADR-0060 的遗留
/// （山峰 → POI 符号尚未接通），不是本枚举的职责。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PoiKind {
    /// 未分类（缺省）：通用点标。
    #[default]
    Other,
    /// 聚落：城市 / 村镇。
    Settlement,
    /// 山峰（地标，ADR-0060）。
    Peak,
    /// 港口 / 码头 / 渡口。
    Port,
    /// 交通站点：火车站 / 客运站 / 驿站。
    Station,
    /// 景点 / 公园 / 古迹。
    Scenic,
    /// 餐饮。
    Food,
    /// 住宿。
    Lodging,
    /// 医疗。
    Medical,
    /// 教育 / 科研。
    Education,
    /// 行政 / 政府机构。
    Government,
    /// 商业 / 购物。
    Shop,
}

impl PoiKind {
    /// 全部类别（声明序）。渲染层的材质 / 批次表按 [`Self::index`] 索引，
    /// 新增类别只需在这里追加 + 给配色。
    pub const ALL: [Self; 12] = [
        Self::Other,
        Self::Settlement,
        Self::Peak,
        Self::Port,
        Self::Station,
        Self::Scenic,
        Self::Food,
        Self::Lodging,
        Self::Medical,
        Self::Education,
        Self::Government,
        Self::Shop,
    ];

    /// 类别总数（= [`Self::ALL`] 长度，编译期一致）。
    pub const COUNT: usize = Self::ALL.len();

    /// 声明序下标（0..[`Self::COUNT`]）。渲染层用它索引平面数组。
    pub const fn index(self) -> usize {
        // 单元枚举的判别值 = 声明序（与 ALL 由测试钉死一致）。
        self as usize
    }

    /// 稳定字符串键（主题配色表 / 图标表用它做键，`serde` 同一拼写）。
    ///
    /// 为什么不用 `serde` 的序列化结果：渲染层不该为了取一个键而走一遍
    /// 序列化（且会产生分配）。这里手写常量，与 `#[serde(rename_all)]`
    /// 由测试钉死一致。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Other => "other",
            Self::Settlement => "settlement",
            Self::Peak => "peak",
            Self::Port => "port",
            Self::Station => "station",
            Self::Scenic => "scenic",
            Self::Food => "food",
            Self::Lodging => "lodging",
            Self::Medical => "medical",
            Self::Education => "education",
            Self::Government => "government",
            Self::Shop => "shop",
        }
    }
}

/// 地点节点。对应 `distances/places.yaml`。
///
/// ## 双重身份（2026-01 修订）
///
/// 1. **距离图节点**——`PlaceEdge.a/b` 与路径搜索用它做稳定 ID，不需要几何；
/// 2. **地图上的 icon + 范围**——用 `(map, x, y, w, h)` 定位。
///
/// `w/h` 可选：`None` = 地图上就是一个点 icon（如关隘），`Some` = 有矩形范围（如城市）。
/// 这正对应用户原话：「有些在世界地图上它可能就是一个 icon，但是他是有范围的」。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub id: Id,
    pub name: String,
    /// 地理事实类型的**旧词汇**（TerrainKind 0–9；ADR-0032 D1 已收口）。
    ///
    /// 唯一词汇是 [`Self::biome`]（冻结群系表）。本字段降级为 serde 只读
    /// 迁移脚手架：旧 YAML 照常读入（迁移窗口，一个版本周期），序列化
    /// **不再输出**；wv-io 加载时把它迁入 `biome`（见
    /// `place_feature_to_biome`）并发诊断提示。下版本删除本字段。
    #[serde(default, skip_serializing)]
    pub feature: Option<u8>,
    /// 若该地点对应某张地图上的锚点，则引用其 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<Id>,
    /// 中心点 X（地图局部坐标）。`None` 表示不锚定到地图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// 中心点 Y（地图局部坐标）。`None` 表示不锚定到地图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// 范围宽度（须与 `h` 成对出现）。`None` = 点 icon。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f32>,
    /// 范围高度（须与 `w` 成对出现）。`None` = 点 icon。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
    /// 生物群系编号（ADR-0029 D1；冻结表在 `wv_terrain::biome`）。
    /// `None` = 普通地点（或旧工程只有 `feature`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub biome: Option<u8>,
    /// 标记包络（意图多边形，图内局部 wu 扁平点列；ADR-0029 D1）。
    /// 最终边界在 `place_geometry`（生成快照），包络只用于重圈重生成。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Vec<f32>>,
    /// 呈现级别（ADR-0030 D1）：数值越大越重要，渲染层按「视野档位 × 最低级别」
    /// 决定点标/标签是否出现（Nation ≥ 3 / Region ≥ 2 / Edit 全见）。
    ///
    /// 词汇表（GLOSSARY「呈现级别」）：`3` 国家级、`2` 市镇级、`1` 普通地点、
    /// `0` 网络节点/微点（路网交点这类只为锚定边存在的点）。
    ///
    /// `None`（缺省）= 按旧两档折算（[`Self::effective_rank`]）：有 `w/h` → 3，
    /// 否则 → 1——既有工程零迁移、渲染行为与引入前逐位一致。折算规则是冻结
    /// 契约（测试 `effective_rank_folds_legacy_two_tier_by_wh` 钉死），改动它
    /// 等于改旧工程的可视行为。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u8>,
    /// 作者手绘的**折线**几何（ADR-0046 D1；图内局部 wu 扁平点列）。
    ///
    /// `None` = 点状地点（绝大多数）；`Some` = 线状地点——作者手绘的小溪 / 小河。
    /// 小于一个 cell 的水系无法用格子表达（ADR-0046 处境），故走矢量。
    ///
    /// 与 `envelope`（面）**正交**：一个 Place 要么是点、要么是线，不兼得。
    /// 折线**不占格、不写 biome**（ADR-0046 D3）——与一格宽的笔刷河
    /// （`wv_terrain::intent::Intent::River`）并存。
    ///
    /// 锚点：`x` / `y` 缺省时由**折线中点**派生（沿用街道 `polyline_midpoint()`
    /// 先例，见 `Place::anchor_from_line`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Vec<f32>>,
    /// 地图符号类别（ADR-0064）。`None` = 未分类，渲染回退红点白环；
    /// `Some(kind)` 由专业制图按类别配色。
    ///
    /// `skip_serializing_if` 保证旧工程保存后**不多出一行 `kind: null`**——
    /// YAML 是作者手读的，空字段是噪音。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PoiKind>,
}

impl Place {
    /// 有效呈现级别（ADR-0030 D1 的折算入口，渲染层唯一可用的级别读法）。
    ///
    /// 显式 `rank` 原样返回；`None` 按旧两档折算：有 `w/h` → 3（城市，Nation
    /// 档即可见），否则 → 1（普通点，Edit 档才可见）——与引入 `rank` 前
    /// 「城市恒显 / 普通点 Region 带以下才现」的行为逐位一致。
    pub fn effective_rank(&self) -> u8 {
        match self.rank {
            Some(r) => r,
            None => {
                if self.w.is_some() && self.h.is_some() {
                    3
                } else {
                    1
                }
            }
        }
    }

    /// 线状地点的**锚点**（ADR-0046 D2）：折线中点。
    ///
    /// 口径与街道的 `polyline_midpoint()` 同款（`wv_core::source::street`）：
    /// 取**弧长中点**（不是顶点平均）——折线弯折时顶点平均会偏向拐角。
    ///
    /// - 无折线 / 折线不足 2 点 → `None`；
    /// - 折线退化（所有点重合）→ `None`。
    pub fn anchor_from_line(&self) -> Option<(f32, f32)> {
        let line = self.line.as_ref()?;
        let pts: Vec<(f32, f32)> = line
            .chunks(2)
            .filter(|p| p.len() == 2)
            .map(|p| (p[0], p[1]))
            .collect();
        if pts.len() < 2 {
            return None;
        }
        // 弧长中点：累计各段长度，找跨越总长一半的那一段并线性插值。
        let seg_len: Vec<f32> = pts
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .collect();
        let total: f32 = seg_len.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let half = total / 2.0;
        let mut acc = 0.0f32;
        for (i, len) in seg_len.iter().enumerate() {
            if acc + *len >= half {
                let t = if *len > 0.0 { (half - acc) / *len } else { 0.0 };
                let a = pts[i];
                let b = pts[i + 1];
                return Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
            acc += *len;
        }
        let last = pts[pts.len() - 1];
        Some((last.0, last.1))
    }

    /// 地点的**有效锚点**（ADR-0046 D2）：显式 `x` / `y` 优先，缺省时由折线
    /// 中点派生。
    ///
    /// 渲染与拾取取锚点的**唯一入口**（点状地点的 `x/y` 恒 `Some`，行为不变）。
    pub fn effective_anchor(&self) -> Option<(f32, f32)> {
        match (self.x, self.y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => self.anchor_from_line(),
        }
    }

    /// 点是否落在地点的范围内（含边界）。
    ///
    /// - `w` / `h` 都是 `Some` → 矩形判定（含边界）；
    /// - `w` / `h` 都是 `None` → 点判定（`EPS = 1e-3`）；
    /// - `x` / `y` 缺失 → 恒 `false`（未锚定到地图，无从判定）。
    ///
    /// 用途：几何化校验（「单位进入了事件范围却没列为参与者」）。
    pub fn contains(&self, px: f32, py: f32) -> bool {
        let (Some(cx), Some(cy)) = (self.x, self.y) else {
            return false;
        };
        match (self.w, self.h) {
            (Some(w), Some(h)) => (px - cx).abs() <= w / 2.0 && (py - cy).abs() <= h / 2.0,
            _ => {
                const EPS: f32 = 1e-3;
                (px - cx).abs() <= EPS && (py - cy).abs() <= EPS
            }
        }
    }

    /// 按地界环拟合锚点与范围：质心写入 `x/y`，包围盒宽高写入 `w/h`。
    ///
    /// 为什么需要同步：POI、标签远档判定与 `EventLocation::resolve` 今天都只读
    /// `x/y/w/h` 这四个投影字段；地界多边形才是几何真值。创建/编辑地界时在
    /// `wv-edit` 侧调用本方法，把真值投影回既有字段（与 ADR-0022 D2「双字段
    /// 同步」同口径），渲染与校验无需改读取方式。
    ///
    /// - 点数不足 3（无法构成多边形面）→ 原样返回，不改写任何字段；
    /// - 质心 = 顶点简单平均（复用 `Region::centroid` 同口径，够用且稳定）；
    /// - `w/h` = 包围盒宽高（不存旋转，与 `contains` 的轴对齐矩形判定一致）。
    pub fn fit_anchor_to_ring(&mut self, ring: &[f32]) {
        if ring.len() < 6 {
            return;
        }
        let n = (ring.len() / 2) as f32;
        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for pair in ring.chunks_exact(2) {
            let (px, py) = (pair[0], pair[1]);
            sx += px;
            sy += py;
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }
        self.x = Some(sx / n);
        self.y = Some(sy / n);
        self.w = Some(max_x - min_x);
        self.h = Some(max_y - min_y);
    }
}

/// 地点构成的图。原则以「图而非表」表达距离维度（§5.5）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DistanceGraph {
    #[serde(default)]
    pub places: Vec<Place>,
    #[serde(default)]
    pub edges: Vec<PlaceEdge>,
    #[serde(default)]
    pub vehicles: Vec<Vehicle>,
}

/// 路径搜索中的一个状态：位置 + 当前交通工具。
///
/// 状态扩展为 `(PlaceId, VehicleId)` 是为了支持**中途换乘**（§10.2）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchState {
    pub place: Id,
    pub vehicle: Id,
}

/// 一段行程。
#[derive(Debug, Clone, PartialEq)]
pub struct JourneyLeg {
    pub from: Id,
    pub to: Id,
    pub vehicle: Id,
    pub edge: Id,
    pub days: f32,
}

/// 行程时间计算结果，含命中的路径。
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledJourney {
    pub legs: Vec<JourneyLeg>,
    pub total_days: f32,
}

/// 时间可行性校验结论（§10.3）。
#[derive(Debug, Clone, PartialEq)]
pub enum Feasibility {
    /// 可达，且在截止时间前到达。`arrival` 为天数计数器。
    Reachable {
        arrival: i64,
        journey: ScheduledJourney,
    },
    /// 可达但会迟到。`earliest_arrival` 为天数计数器。
    ArrivesLate {
        earliest_arrival: i64,
        delay_days: f32,
        journey: ScheduledJourney,
    },
    /// 不可达，附原因。
    Unreachable { reason: UnreachableReason },
}

/// 不可达的具体原因，直接反馈到 UI。
#[derive(Debug, Clone, PartialEq)]
pub enum UnreachableReason {
    /// 图中不存在从起点到终点的路径。
    NoPath,
    /// 存在路径，但没有任何交通工具可用。
    NoUsableVehicle,
    /// 交通工具的前置条件未被满足。
    RequirementsUnmet { vehicle: Id },
}

impl DistanceGraph {
    /// 按 ID 查地点。
    pub fn place(&self, id: &Id) -> Option<&Place> {
        self.places.iter().find(|p| &p.id == id)
    }

    /// 按 ID 查交通工具。
    pub fn vehicle(&self, id: &Id) -> Option<&Vehicle> {
        self.vehicles.iter().find(|v| &v.id == id)
    }

    /// 计算单段耗时（天）。
    ///
    /// 这是 §10.2 公式的唯一入口；禁止在别处重复实现速度换算。
    pub fn segment_days(&self, edge: &PlaceEdge, vehicle_id: &Id) -> Option<f32> {
        let vehicle = self.vehicle(vehicle_id)?;
        vehicle.segment_days(edge.distance_km, &edge.terrain)
    }

    /// Dijkstra 路径搜索，边权为该段在可用交通工具下的最短耗时。
    ///
    /// `allowed` 为本次搜索允许使用的交通工具集合。若为空，视为全部可用。
    ///
    /// 注意：节点状态扩展为 `(PlaceId, VehicleId)`，因此「换乘」被建模为
    /// 位于同一地点、但交通工具不同的两个状态之间的零耗时转移。
    pub fn shortest_journey(
        &self,
        from: &Id,
        to: &Id,
        allowed: &[Id],
    ) -> Result<ScheduledJourney, CoreError> {
        if self.place(from).is_none() {
            return Err(CoreError::UnknownPlace(from.to_string()));
        }
        if self.place(to).is_none() {
            return Err(CoreError::UnknownPlace(to.to_string()));
        }

        let vehicles: Vec<&Vehicle> = if allowed.is_empty() {
            self.vehicles.iter().collect()
        } else {
            self.vehicles
                .iter()
                .filter(|v| allowed.contains(&v.id))
                .collect()
        };
        if vehicles.is_empty() {
            return Err(CoreError::PathSearch("没有可用交通工具".into()));
        }

        // 距离表：state -> 最短天数
        let mut best: HashMap<SearchState, f32> = HashMap::new();
        // 前驱：state -> (prev_state, leg)
        let mut prev: HashMap<SearchState, (SearchState, JourneyLeg)> = HashMap::new();

        // 起点状态：起点 × 每种交通工具
        let mut frontier: Vec<(f32, SearchState)> = Vec::new();
        for v in &vehicles {
            let s = SearchState {
                place: from.clone(),
                vehicle: v.id.clone(),
            };
            best.insert(s.clone(), 0.0);
            frontier.push((0.0, s));
        }

        let mut goal: Option<SearchState> = None;

        // 朴素 Dijkstra（节点数级别为「地点 × 工具」，规模可控）
        while !frontier.is_empty() {
            // 取当前最小
            let (idx, _) = frontier
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            let (cur_cost, cur) = frontier.swap_remove(idx);

            if cur.place == *to {
                goal = Some(cur.clone());
                break;
            }
            if cur_cost > *best.get(&cur).unwrap_or(&f32::INFINITY) {
                continue;
            }

            // 1) 换乘：同一地点，换另一种交通工具，零耗时
            for v in &vehicles {
                if v.id == cur.vehicle {
                    continue;
                }
                let next = SearchState {
                    place: cur.place.clone(),
                    vehicle: v.id.clone(),
                };
                if cur_cost < *best.get(&next).unwrap_or(&f32::INFINITY) {
                    best.insert(next.clone(), cur_cost);
                    prev.insert(
                        next.clone(),
                        (
                            cur.clone(),
                            JourneyLeg {
                                from: cur.place.clone(),
                                to: cur.place.clone(),
                                vehicle: v.id.clone(),
                                edge: Id::new_unchecked("__transfer__"),
                                days: 0.0,
                            },
                        ),
                    );
                    frontier.push((cur_cost, next));
                }
            }

            // 2) 沿边前进
            for edge in self
                .edges
                .iter()
                .filter(|e| e.a == cur.place || e.b == cur.place)
            {
                let other = if edge.a == cur.place {
                    &edge.b
                } else {
                    &edge.a
                };
                // 单向边只能从 a 到 b
                if !edge.bidirectional && edge.b != *other {
                    continue;
                }
                if !edge.bidirectional && edge.a != cur.place {
                    continue;
                }
                let Some(days) = self.segment_days(edge, &cur.vehicle) else {
                    continue;
                };
                let next = SearchState {
                    place: other.clone(),
                    vehicle: cur.vehicle.clone(),
                };
                let cost = cur_cost + days;
                if cost < *best.get(&next).unwrap_or(&f32::INFINITY) {
                    best.insert(next.clone(), cost);
                    prev.insert(
                        next.clone(),
                        (
                            cur.clone(),
                            JourneyLeg {
                                from: cur.place.clone(),
                                to: other.clone(),
                                vehicle: cur.vehicle.clone(),
                                edge: edge.id.clone(),
                                days,
                            },
                        ),
                    );
                    frontier.push((cost, next));
                }
            }
        }

        let Some(goal) = goal else {
            return Err(CoreError::PathSearch("无可行路径".into()));
        };

        // 回溯路径
        let mut legs = Vec::new();
        let mut cursor = goal.clone();
        while let Some((p, leg)) = prev.get(&cursor).cloned() {
            if leg.edge.as_str() != "__transfer__" {
                legs.push(leg);
            }
            cursor = p;
        }
        legs.reverse();

        let total_days = *best.get(&goal).unwrap_or(&0.0);
        Ok(ScheduledJourney { legs, total_days })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 构造测试图：青云城 --官道300km-- 黑水村 --山道120km-- 落霞关
    fn fixture() -> DistanceGraph {
        DistanceGraph {
            places: vec![
                Place {
                    kind: None,
                    line: None,
                    id: vid("city_qingyun"),
                    name: "青云城".into(),
                    map: None,
                    x: None,
                    y: None,
                    w: None,
                    h: None,
                    feature: None,
                    biome: None,
                    envelope: None,
                    rank: None,
                },
                Place {
                    kind: None,
                    line: None,
                    id: vid("village_heishui"),
                    name: "黑水村".into(),
                    map: None,
                    x: None,
                    y: None,
                    w: None,
                    h: None,
                    feature: None,
                    biome: None,
                    envelope: None,
                    rank: None,
                },
                Place {
                    kind: None,
                    line: None,
                    id: vid("pass_luoxia"),
                    name: "落霞关".into(),
                    map: None,
                    x: None,
                    y: None,
                    w: None,
                    h: None,
                    feature: None,
                    biome: None,
                    envelope: None,
                    rank: None,
                },
            ],
            edges: vec![
                PlaceEdge {
                    id: vid("e_qingyun_heishui"),
                    a: vid("city_qingyun"),
                    b: vid("village_heishui"),
                    distance_km: 300.0,
                    terrain: "road".into(),
                    bidirectional: true,
                    rank: None,
                },
                PlaceEdge {
                    id: vid("e_heishui_luoxia"),
                    a: vid("village_heishui"),
                    b: vid("pass_luoxia"),
                    distance_km: 120.0,
                    terrain: "mountain".into(),
                    bidirectional: true,
                    rank: None,
                },
            ],
            vehicles: vec![
                Vehicle {
                    id: vid("walk"),
                    name: "步行".into(),
                    instant: false,
                    speed_kmh: Some(5.0),
                    daily_hours: Some(10.0),
                    terrain_modifiers: [
                        ("road".to_string(), 1.2),
                        ("plain".to_string(), 1.0),
                        ("forest".to_string(), 0.7),
                        ("mountain".to_string(), 0.4),
                    ]
                    .into_iter()
                    .collect(),
                    requires_endpoints: false,
                    requirements: vec![],
                },
                Vehicle {
                    id: vid("teleport_array"),
                    name: "传送阵".into(),
                    instant: true,
                    speed_kmh: None,
                    daily_hours: None,
                    terrain_modifiers: HashMap::new(),
                    requires_endpoints: true,
                    requirements: vec!["地点具有传送阵".into()],
                },
            ],
        }
    }

    #[test]
    fn effective_speed_applies_terrain_multiplier() {
        let g = fixture();
        let walk = g.vehicle(&vid("walk")).unwrap();
        // 5 km/h × 1.2 (road) × 10 h = 60 km/日（ADR-0024 单位修正）
        let s = walk.effective_daily_speed("road").unwrap();
        assert!((s - 60.0).abs() < 1e-6, "got {s}");
        // 未声明地形回退到 1.0
        let s2 = walk.effective_daily_speed("swamp").unwrap();
        assert!((s2 - 50.0).abs() < 1e-6, "got {s2}");
    }

    #[test]
    fn instant_vehicle_costs_zero_days() {
        let g = fixture();
        let tp = g.vehicle(&vid("teleport_array")).unwrap();
        assert_eq!(tp.segment_days(9999.0, "mountain"), Some(0.0));
        assert_eq!(tp.effective_daily_speed("road"), None);
    }

    #[test]
    fn shortest_journey_uses_fastest_terrain() {
        let g = fixture();
        // 300km 走官道：300 / (5*1.2*10) = 300 / 60 = 5 天
        // 120km 走山道：120 / (5*0.4*10) = 120 / 20 = 6 天
        // 合计 11 天
        let j = g
            .shortest_journey(&vid("city_qingyun"), &vid("pass_luoxia"), &[vid("walk")])
            .unwrap();
        assert_eq!(j.legs.len(), 2);
        assert!((j.total_days - 11.0).abs() < 1e-3, "got {}", j.total_days);
    }

    #[test]
    fn teleport_makes_route_instant() {
        let g = fixture();
        let j = g
            .shortest_journey(
                &vid("city_qingyun"),
                &vid("pass_luoxia"),
                &[vid("walk"), vid("teleport_array")],
            )
            .unwrap();
        // 传送阵使用同一张边表，但每段耗时为 0
        assert_eq!(j.total_days, 0.0);
    }

    #[test]
    fn unknown_place_is_reported() {
        let g = fixture();
        let err = g
            .shortest_journey(&vid("nowhere"), &vid("pass_luoxia"), &[])
            .unwrap_err();
        assert!(matches!(err, CoreError::UnknownPlace(_)));
    }

    // ---- 几何：Place 的范围判定（2026-01 修订）----

    /// `PoiKind::key()` 必须与 serde 的 YAML 拼写逐字一致。
    ///
    /// 两条路径会同时出现在仓库里：YAML 读写走 serde，主题配色/图标表走
    /// `key()`。拼写漂移会让「作者写 medical、渲染查 MedicalColor」静默失配。
    #[test]
    fn poi_kind_key_matches_serde_spelling() {
        let all = [
            PoiKind::Other,
            PoiKind::Settlement,
            PoiKind::Peak,
            PoiKind::Port,
            PoiKind::Station,
            PoiKind::Scenic,
            PoiKind::Food,
            PoiKind::Lodging,
            PoiKind::Medical,
            PoiKind::Education,
            PoiKind::Government,
            PoiKind::Shop,
        ];
        for k in all {
            let yaml = serde_yaml::to_string(&k).expect("PoiKind 应可序列化");
            assert_eq!(yaml.trim(), k.key(), "{k:?} 的 serde 拼写与 key() 不一致");
        }
        assert_eq!(PoiKind::default(), PoiKind::Other, "缺省应为未分类");
        assert_eq!(PoiKind::COUNT, 12);
        for (i, k) in PoiKind::ALL.iter().enumerate() {
            assert_eq!(k.index(), i, "{k:?} 的 index 应等于声明序");
        }
    }

    /// 旧工程（无 `kind` 字段）读写都不该被塞进 `kind: null`。
    ///
    /// YAML 是作者手读的格式，多一个空字段就是噪音；同时反向读入必须成功
    /// （`serde(default)`），否则一次保存就会让旧工程读不回来。
    #[test]
    fn place_without_kind_round_trips_without_emitting_the_field() {
        let p = point_place();
        let yaml = serde_yaml::to_string(&p).expect("应可序列化");
        assert!(!yaml.contains("kind"), "缺省 kind 不应落盘：\n{yaml}");
        let back: Place = serde_yaml::from_str(&yaml).expect("应可反序列化");
        assert_eq!(back, p);
    }

    /// 显式分类按 snake_case 落盘并可读回（作者手写 / 渲染配色同一拼写）。
    #[test]
    fn place_kind_round_trips_as_snake_case() {
        let mut p = point_place();
        p.kind = Some(PoiKind::Medical);
        let yaml = serde_yaml::to_string(&p).expect("应可序列化");
        assert!(
            yaml.contains("kind: medical"),
            "分类应为 snake_case：\n{yaml}"
        );
        let back: Place = serde_yaml::from_str(&yaml).expect("应可反序列化");
        assert_eq!(back.kind, Some(PoiKind::Medical));
    }

    /// 矩形地点：中心 (100, 200)，宽 40、高 20 → x ∈ [80, 120]，y ∈ [190, 210]。
    fn rect_place() -> Place {
        Place {
            kind: None,
            line: None,
            id: vid("city_rect"),
            name: "rect city".into(),
            map: Some(vid("map_world")),
            x: Some(100.0),
            y: Some(200.0),
            w: Some(40.0),
            h: Some(20.0),
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
        }
    }

    fn point_place() -> Place {
        Place {
            kind: None,
            line: None,
            id: vid("pass_point"),
            name: "point pass".into(),
            map: Some(vid("map_world")),
            x: Some(10.0),
            y: Some(20.0),
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
        }
    }

    fn unanchored_place() -> Place {
        Place {
            kind: None,
            line: None,
            id: vid("place_nomap"),
            name: "unanchored".into(),
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
        }
    }

    #[test]
    fn rect_place_contains_interior_point() {
        assert!(rect_place().contains(100.0, 200.0), "centre must be inside");
        assert!(
            rect_place().contains(81.0, 191.0),
            "inner point must be inside"
        );
    }

    // ---- 呈现级别（ADR-0030 D1）----

    /// 冻结契约：`rank = None` 的折算规则——有 `w/h` → 3（城市，Nation 档即可见），
    /// 否则 → 1（普通点，Edit 档才可见）。改动本测试的期望 = 改旧工程的可视行为，
    /// 必须走 ADR。
    #[test]
    fn effective_rank_folds_legacy_two_tier_by_wh() {
        assert_eq!(
            rect_place().effective_rank(),
            3,
            "有 w/h 的城市折算为国家级"
        );
        assert_eq!(
            point_place().effective_rank(),
            1,
            "无 w/h 的普通点折算为普通地点"
        );
        assert_eq!(
            unanchored_place().effective_rank(),
            1,
            "未锚定地点同样按无 w/h 折算"
        );
    }

    /// 显式 `rank` 原样生效，压过 w/h 折算（作者显式降级/升级城市都成立）。
    #[test]
    fn explicit_rank_overrides_the_wh_fold() {
        let mut p = rect_place();
        p.rank = Some(0);
        assert_eq!(p.effective_rank(), 0, "显式 rank 压过 w/h 折算");
        let mut q = point_place();
        q.rank = Some(3);
        assert_eq!(q.effective_rank(), 3, "显式 rank 可把点标升到国家级");
    }

    /// rank 序列化遵循可选字段惯例：Some 写键、None 不写键（旧 YAML 无 rank
    /// 键照常加载，round-trip 无损）。
    #[test]
    fn rank_round_trips_through_yaml_and_none_stays_absent() {
        let mut p = rect_place();
        p.rank = Some(2);
        let text = serde_yaml::to_string(&p).expect("序列化");
        assert!(text.contains("rank: 2"), "Some(2) 应写键，实得:\n{text}");
        let back: Place = serde_yaml::from_str(&text).expect("反序列化");
        assert_eq!(back, p, "带 rank 的 Place YAML round-trip 应无损");

        let plain_text = serde_yaml::to_string(&point_place()).expect("序列化");
        assert!(
            !plain_text.contains("rank"),
            "None 不应写 rank 键（旧读取端兼容），实得:\n{plain_text}"
        );
        let plain_back: Place =
            serde_yaml::from_str(&plain_text).expect("无 rank 键的旧 YAML 应照常加载");
        assert_eq!(plain_back, point_place(), "无 rank 键 round-trip 无损");
    }

    /// PlaceEdge 距离折算（ADR-0047 D1 冻结契约，逐值手算）：
    /// ≥ 100 km → 3、≥ 30 km → 2、否则 → 1。边界值取「恰在门槛上归高档」。
    #[test]
    fn place_edge_effective_rank_folds_by_distance() {
        let edge = |km: f32| PlaceEdge {
            id: Id::new_unchecked("edge_probe"),
            a: Id::new_unchecked("place_a"),
            b: Id::new_unchecked("place_b"),
            distance_km: km,
            terrain: "road".into(),
            bidirectional: true,
            rank: None,
        };
        assert_eq!(edge(100.0).effective_rank(), 3, "100 km 恰在干线门槛上");
        assert_eq!(edge(520.0).effective_rank(), 3, "城际长边是干线");
        assert_eq!(edge(99.9).effective_rank(), 2, "门槛下沿归区域级");
        assert_eq!(edge(30.0).effective_rank(), 2, "30 km 恰在区域门槛上");
        assert_eq!(edge(29.9).effective_rank(), 1, "门槛下沿归局部");
        assert_eq!(edge(5.0).effective_rank(), 1, "短边是局部道路");
    }

    /// 显式 rank 压过距离折算（与 Place 同款：作者显式定级永远赢）。
    #[test]
    fn place_edge_explicit_rank_overrides_distance_fold() {
        let mut e = PlaceEdge {
            id: Id::new_unchecked("edge_probe"),
            a: Id::new_unchecked("place_a"),
            b: Id::new_unchecked("place_b"),
            distance_km: 500.0,
            terrain: "road".into(),
            bidirectional: true,
            rank: Some(1),
        };
        assert_eq!(e.effective_rank(), 1, "显式 rank 把长边降为局部道路");
        e.rank = Some(0);
        e.distance_km = 5.0;
        assert_eq!(e.effective_rank(), 0, "显式 0 = 仅编辑级可见");
    }

    /// PlaceEdge.rank 的 YAML 惯例与 Place.rank 同款：None 不写键、Some 写键。
    #[test]
    fn place_edge_rank_round_trips_through_yaml_and_none_stays_absent() {
        let mut e = PlaceEdge {
            id: Id::new_unchecked("edge_yaml"),
            a: Id::new_unchecked("place_a"),
            b: Id::new_unchecked("place_b"),
            distance_km: 42.0,
            terrain: "road".into(),
            bidirectional: true,
            rank: Some(3),
        };
        let text = serde_yaml::to_string(&e).expect("序列化");
        assert!(text.contains("rank: 3"), "Some(3) 应写键，实得:\n{text}");
        let back: PlaceEdge = serde_yaml::from_str(&text).expect("反序列化");
        assert_eq!(back, e, "带 rank 的 PlaceEdge YAML round-trip 应无损");

        e.rank = None;
        let plain_text = serde_yaml::to_string(&e).expect("序列化");
        assert!(
            !plain_text.contains("rank"),
            "None 不应写 rank 键（旧 YAML 兼容），实得:\n{plain_text}"
        );
        let plain_back: PlaceEdge =
            serde_yaml::from_str(&plain_text).expect("无 rank 键的旧 YAML 应照常加载");
        assert_eq!(plain_back, e, "无 rank 键 round-trip 无损");
    }

    #[test]
    fn rect_place_contains_boundary_point() {
        // 含边界：四个角（x=80/120，y=190/210）都必须算在内。
        for (px, py) in [(80.0, 190.0), (120.0, 190.0), (80.0, 210.0), (120.0, 210.0)] {
            assert!(
                rect_place().contains(px, py),
                "boundary point ({px}, {py}) must be accepted (inclusive bounds)"
            );
        }
    }

    #[test]
    fn rect_place_rejects_point_just_outside_boundary() {
        assert!(
            !rect_place().contains(79.99, 200.0),
            "just outside the left edge must be rejected"
        );
        assert!(
            !rect_place().contains(100.0, 210.01),
            "just outside the bottom edge must be rejected"
        );
    }

    #[test]
    fn point_place_matches_exact_coordinate_only() {
        assert!(
            point_place().contains(10.0, 20.0),
            "point place hits its own coordinate"
        );
        assert!(
            !point_place().contains(10.5, 20.0),
            "point place must not hit a coordinate 0.5 away (EPS = 1e-3)"
        );
    }

    #[test]
    fn point_place_tolerates_sub_epsilon_drift() {
        assert!(
            point_place().contains(10.0005, 20.0),
            "5e-4 drift is within EPS = 1e-3 and must still hit"
        );
    }

    #[test]
    fn unanchored_place_contains_nothing() {
        assert!(
            !unanchored_place().contains(0.0, 0.0),
            "a place without x/y cannot be judged geometrically, so it is always false"
        );
    }

    #[test]
    fn half_given_extent_falls_back_to_point_judgement() {
        // w 有、h 无 → 不构成矩形，退化为点判定（校验层另行报「必须成对」）。
        let p = Place {
            kind: None,
            w: Some(40.0),
            h: None,
            ..rect_place()
        };
        assert!(p.contains(100.0, 200.0), "centre still hits");
        assert!(
            !p.contains(110.0, 200.0),
            "w without h degrades to a point, so 110 must not hit"
        );
    }

    // ---- 地理事实类型 + 地界锚点拟合（theme overlay）----

    fn feature_place(feature: Option<u8>) -> Place {
        Place {
            kind: None,
            line: None,
            id: Id::new_unchecked("place_dark_forest"),
            name: "黑森林".into(),
            map: Some(Id::new_unchecked("map_world")),
            x: None,
            y: None,
            w: None,
            h: None,
            feature,
            biome: None,
            envelope: None,
            rank: None,
        }
    }

    #[test]
    fn place_biome_and_envelope_round_trip_through_yaml() {
        let mut p = feature_place(None);
        p.biome = Some(1);
        p.envelope = Some(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0]);
        let yaml = serde_yaml::to_string(&p).expect("序列化");
        let back: Place = serde_yaml::from_str(&yaml).expect("反序列化");
        assert_eq!(back.biome, Some(1));
        assert_eq!(back.envelope.as_deref().map(|v| v.len()), Some(6));
        assert_eq!(back, p, "带标记字段的 Place 往返必须无损");
    }

    /// 旧 feature 字段：读取侧保留（迁移窗口），写入侧不再落盘（ADR-0032 D1）。
    /// 严格 round-trip 契约自此只对「不含旧字段的世界」字面成立（ADR-0032 D8）。
    #[test]
    fn legacy_place_without_marker_fields_loads() {
        let yaml = "id: place_old
name: 旧地
feature: 4
";
        let p: Place = serde_yaml::from_str(yaml).expect("旧 YAML 必须照常加载");
        assert_eq!(p.biome, None, "缺省为 None");
        assert_eq!(p.envelope, None, "缺省为 None");
        assert_eq!(p.feature, Some(4), "读取侧：旧 feature 字段保留");
        let out = serde_yaml::to_string(&p).expect("序列化");
        assert!(
            !out.contains("feature"),
            "写入侧：feature 不得再落盘（ADR-0032 D1），实得:\n{out}"
        );
    }

    #[test]
    fn fit_anchor_to_ring_sets_centroid_and_bbox() {
        let mut p = feature_place(Some(4));
        p.fit_anchor_to_ring(&[0.0, 0.0, 40.0, 0.0, 40.0, 20.0, 0.0, 20.0]);
        assert_eq!((p.x, p.y), (Some(20.0), Some(10.0)), "质心为 (20,10)");
        assert_eq!((p.w, p.h), (Some(40.0), Some(20.0)), "包围盒 40×20");
    }

    #[test]
    fn fit_anchor_to_ring_with_degenerate_ring_leaves_anchors_untouched() {
        let mut p = feature_place(Some(4));
        p.fit_anchor_to_ring(&[1.0, 2.0]);
        assert_eq!(
            (p.x, p.y, p.w, p.h),
            (None, None, None, None),
            "不足 3 点不写锚点"
        );
    }
}
