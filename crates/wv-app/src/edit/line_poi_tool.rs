//! 线段 POI 的钉点交互（ADR-0070 D4 · 计划 Task 11）。
//!
//! ## 为什么放在 `edit/` 而不是 `ui/`
//!
//! 与 [`crate::edit::cell_stroke`] / [`crate::edit::region_select`] 同款落点：
//! 本模块是「地图手势 → `EditCommand`」的翻译器（AGENTS §3.3——System 不得
//! 直接改世界），不是面板渲染。面板（`ui/line_poi_panel.rs`）只出表单，
//! 手势与提交都在这里。
//!
//! ## 交互数据流
//!
//! ```text
//! 工具开 + 左键点空白 ─▶ Begin（记起点 wu，pending 出壳）
//!   光标移动           ─▶ 终点格变化？─▶ plan_polyline 重算预览（格级缓存）
//!   左键第二击         ─▶ PinEnd（终点定格，预览末点 = 精确钉点）
//! 面板「确认」/ Enter  ─▶ confirm_pending ─▶ CreateLinePoiCommand（下切模式带 CarveSpec）
//! Esc                  ─▶ cancel_pending（弃钉点）；无 pending 时清选中
//! 工具开 + 点中既有线  ─▶ Select（面板切属性编辑态）
//! ```
//!
//! ## 纯函数纪律
//!
//! 状态机与几何全部提成纯函数（[`pin_on_click`] / [`pin_on_move`] /
//! [`ensure_preview`] / [`confirm_pending`] / [`line_poi_hit`]）——
//! 「输入状态 + 点击 → 输出新状态 + 待发命令」可以直接单测，
//! **不把断言埋在 egui / Bevy 系统里**（计划 Task 11 Step 1 的硬要求）。
//! Bevy 系统 [`line_poi_tool_input`] 只做取参、gate 与回写。

use bevy::prelude::*;
use wv_core::distance::{LineKind, Place};
use wv_core::id::Id;
use wv_core::map::road_derive::effective_line_kind;
use wv_edit::command::EditCommand;
use wv_terrain::line_path::{carve_corridor, plan_polyline, LinePathMode};
use wv_terrain::{TerrainCell, TerrainGrid};

use crate::state::{
    DraggedEnd, DrillState, EndpointDrag, GridConfig, LinePoiState, PendingLinePoi, PlaybackState,
    ViewState, WorldProjection,
};

/// 选中既有线的命中阈值（**屏幕像素**）。
///
/// 换算成 wu 时乘 `view.scale`（屏幕 1 px = scale wu）；8 px 是测量橡皮
/// （`ERASER_TOLERANCE_WU`）同族的「手指宽度」手感。
pub const SELECT_TOLERANCE_PX: f32 = 8.0;

/// 强制模式（Carve）下切的固定强度（计划 Task 11：strength = 1.0）。
pub const CARVE_STRENGTH: f32 = 1.0;

/// 钉点三拍：空闲 → 跟随（已钉起点，终点跟光标）→ 定格（等确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinPhase {
    /// 未在画线：点击可能起钉，也可能命中选中。
    #[default]
    Idle,
    /// 已钉起点，终点跟着光标走。
    Following,
    /// 终点已定：等「确认」提交或 Esc 取消。
    Pinned,
}

/// 钉点会话的**纯数据投影**——Bevy 系统把它放在 `Local` 里，测试直接构造。
///
/// 拍之外唯一要记的是**绑定图**：钉点不能跨图（测量同款），变图即弃。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PinSession {
    /// 当前拍。
    pub phase: PinPhase,
    /// 钉点绑定的图（`None` = 未开钉）。
    pub map: Option<Id>,
}

/// 一次地图左键点击的裁决（纯函数输出，系统照做）。
#[derive(Debug, Clone, PartialEq)]
pub enum PinClick {
    /// Idle + 未命中既有线：起钉（起点 = 点击处）。
    Begin { start: Vec2 },
    /// Following：第二击定终点。
    PinEnd { end: Vec2 },
    /// Idle + 命中既有线：改为选中它（优先于起钉——先让作者能改已有的）。
    Select { id: Id },
    /// Pinned：等确认 / 取消，点击不动作（防止误触把定好的线搞乱）。
    Ignore,
}

/// Idle 拍一次左键点击的裁决。
///
/// `hit` 是调用方做的线命中检测（需要 world，不属于纯函数）；
/// `None` = 点击处阈值内没有既有线。
pub fn pin_on_click(session: &PinSession, cursor: Vec2, hit: Option<Id>) -> PinClick {
    match session.phase {
        PinPhase::Pinned => PinClick::Ignore,
        PinPhase::Following => PinClick::PinEnd { end: cursor },
        PinPhase::Idle => match hit {
            Some(id) => PinClick::Select { id },
            None => PinClick::Begin { start: cursor },
        },
    }
}

/// Following 拍光标移格：返回新终点格；其余情形 `None`（不重算）。
///
/// 「同格内挪动不重算」是防每帧 A* 的第一道闸（第二道在 [`ensure_preview`]）。
pub fn pin_on_move(
    session: &PinSession,
    end_cell: (i32, i32),
    cursor_cell: (i32, i32),
) -> Option<(i32, i32)> {
    match session.phase {
        PinPhase::Following if end_cell != cursor_cell => Some(cursor_cell),
        _ => None,
    }
}

/// 确保预览与终点格同步；返回**是否重算了**（测试锚：防每帧 A*）。
///
/// 重算条件（二者其一）：`preview` 为空（尚未规划 / 被模式切换作废）；
/// 终点格变了。同格内光标挪动**零重算**。不可达 / 起终点同格时预览落
/// 单点占位（`= start`）——既不给作者「画出来了」的错觉，又把「已规划」
/// 钉进缓存，避免每帧重跑 A*。
pub fn ensure_preview(
    pending: &mut PendingLinePoi,
    end_pt: Vec2,
    grid: &TerrainGrid,
    cell_size: f32,
    mode: LinePathMode,
    seed: u64,
) -> bool {
    let end_cell = cell_of(end_pt, cell_size);
    if !pending.preview.is_empty() && pending.end_cell == end_cell {
        return false; // 缓存命中：同格不重算。
    }
    let raw = plan_polyline(
        grid,
        cell_size,
        (pending.start.x, pending.start.y),
        (end_pt.x, end_pt.y),
        mode,
        seed,
    );
    let mut pts: Vec<Vec2> = raw.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    if pts.is_empty() {
        // 不可达 / 起终点同格：单点占位 =「已规划但无路」——既不给作者
        // 「画出来了」的错觉，又把「已规划」钉进缓存防重算风暴。
        pts.push(pending.start);
    } else {
        // 首尾替换为作者精确钉点（plan_polyline 文档明示的调用方职责）。
        pts[0] = pending.start;
        let n = pts.len();
        pts[n - 1] = end_pt;
    }
    pending.preview = pts;
    pending.end_cell = end_cell;
    true
}

/// 弃钉点（Esc / 面板「取消」共用）：pending 清空。
///
/// 只清 pending、不碰拍——「预览没了 = 拍失效」由交互系统每帧自愈
/// （`line_poi_tool_input` 顶部按 `pending.is_none()` 把拍拉回 Idle），
/// 面板侧拿不到交互系统的 `Local` 会话，这个分工让取消在两处都能生效。
pub fn cancel_pending(pending: &mut Option<PendingLinePoi>) {
    *pending = None;
}

/// 会话失效（变图）时的清理口径（评审修复 R1；T12 扩展拖拽会话）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionReset {
    /// 重绑会话（拍回 Idle、图换成当前图）。
    pub rebind: bool,
    /// 弃 pending。
    pub clear_pending: bool,
    /// 弃 selected。
    pub clear_selected: bool,
    /// 弃端点拖拽会话（T12：旧图预览画进新图 = 幽灵线）。
    pub clear_drag: bool,
}

/// 变图即弃的**完整口径**（纯函数）：pending、selected 与拖拽**一并**弃。
///
/// 只清 pending 的旧口径漏了 selected——旧图的选中线画进新图视框成幽灵
/// 高亮，且选中态面板会用**当前图**的比例尺换算旧图线宽写回
/// `line_width_m`（数据级错误），故三者同口径同弃（T12 加拖拽会话）。
pub fn reset_on_map_change(session: &PinSession, current: Option<&Id>) -> SessionReset {
    let changed = session.map.as_ref() != current;
    SessionReset {
        rebind: changed,
        clear_pending: changed,
        clear_selected: changed,
        clear_drag: changed,
    }
}

/// 起钉（Begin）的状态迁移（纯函数，评审修复 R2）：pending 出壳 +
/// selected 清 + **名称草稿清空** + 拍进 Following。
///
/// selected 不清则面板停在选中态（确认/取消按钮不出现）；name 不清则
/// 新线静默继承命中线的名字（`fill_fields_from_place` 的污染）。
/// 宽度**不在此清**——「上次用值」是连续画同类线的合理缺省，可辩护，
/// 由调用方保持不动即可。
pub fn begin_pin(
    pending: &mut Option<PendingLinePoi>,
    selected: &mut Option<Id>,
    name: &mut String,
    session: &mut PinSession,
    start: Vec2,
    start_cell: (i32, i32),
) {
    *pending = Some(PendingLinePoi {
        start,
        preview: Vec::new(),
        end_cell: start_cell,
    });
    *selected = None;
    name.clear();
    session.phase = PinPhase::Following;
}

/// 预览 / 成线共用的确定性种子：起点格 + 图 ID 的 splitmix64 摘要。
///
/// `plan_polyline` 的 seed 要求「同一次钉点重预览必须同路径」——起点在
/// 会话内不变 ⇒ 种子不变 ⇒ 蜿蜒噪声不变。新 Place 还没有 ID 可哈希，
/// 故用「起点格坐标 + 图 ID」凑确定性（计划 Task 11 的口径）。
pub fn preview_seed(map_id: &str, start_cell: (i32, i32)) -> u64 {
    // FNV-1a 吃图 ID 字节 → 混入起点格 → splitmix64 收尾（与迁移器
    // `chain_id_h4` 的 splitmix 常量同族，仓库内不再造第二套终混函数）。
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for b in map_id.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
    acc ^= (start_cell.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    acc ^= (start_cell.1 as u64).rotate_left(32);
    splitmix64(acc)
}

/// splitmix64 终混（确定性摘要的唯一出口；`0_u64` 也有雪崩）。
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// wu 坐标 → 格坐标（与 `wv_terrain::line_path` 同口径：格 `(i, j)` 覆盖
/// `wu ∈ [i·cell_size, (i+1)·cell_size)`）。
pub fn cell_of(p: Vec2, cell_size: f32) -> (i32, i32) {
    (
        (p.x / cell_size).floor() as i32,
        (p.y / cell_size).floor() as i32,
    )
}

/// 宽度换算（米 → wu）。方向与 T8 渲染侧一致：
/// `km_per_wu` 是「1 wu 有多少 km」，故 1 wu = 1000 × km_per_wu 米，
/// `width_wu = width_m ÷ 1000 ÷ km_per_wu`；无比例尺 → 数字按 wu 直读。
pub fn width_m_to_wu(width_m: f32, km_per_wu: Option<f32>) -> f32 {
    match km_per_wu {
        Some(k) if k > 0.0 => width_m / 1000.0 / k,
        _ => width_m,
    }
}

/// 宽度换算（wu → 米，[`width_m_to_wu`] 的逆）。
pub fn width_wu_to_m(width_wu: f32, km_per_wu: Option<f32>) -> f32 {
    match km_per_wu {
        Some(k) if k > 0.0 => width_wu * 1000.0 * k,
        _ => width_wu,
    }
}

/// 下切走廊总宽（格）：`max(1, round(width_wu / cell_size))`。
///
/// `CarveSpec.width_cells` 是**总宽格数**（评审裁定 I4），1 = 单格链。
pub fn width_cells_of(width_wu: f32, cell_size: f32) -> u32 {
    if cell_size <= 0.0 {
        return 1;
    }
    1.max((width_wu / cell_size).round() as u32)
}

/// 当前图的 `km_per_wu`（取 X 轴分量，与渲染侧同款）。
pub fn map_km_per_wu(world: &wv_core::World, map: &Id) -> Option<f32> {
    world
        .maps
        .iter()
        .find(|n| &n.id == map)
        .and_then(|n| n.km_per_wu())
        .map(|(kx, _)| kx as f32)
}

/// 目标图的格尺寸（wu/格）——**镜像** `wv-edit` `command.rs::marker_cell_size`
/// 的推导（图幅 extent → 根图 extent → 4096；无地形栅格 = `None`）。
///
/// 为什么不直接用会话态 `GridConfig.cell_size`：那条值随当前图刷新，
/// 「捕获时与提交时格口径漂移」在纯从 World 推导的口径下不存在
/// （wv-edit 注释同款理由）。
pub fn grid_cell_size(world: &wv_core::World, map: &Id) -> Option<f32> {
    let data = world.terrain_of(map)?;
    let extent = world
        .maps
        .iter()
        .find(|m| &m.id == map)
        .and_then(|m| m.extent.as_ref())
        .or_else(|| {
            let root = world.meta.as_ref().map(|m| m.root_map.clone())?;
            world
                .maps
                .iter()
                .find(|n| n.id == root)
                .and_then(|n| n.extent.as_ref())
        })
        .map(|e| e.width)
        .unwrap_or(4096.0);
    Some((extent / data.width.max(1) as f32).max(0.01))
}

/// 目标图地形的 `TerrainGrid` 视图（`plan_polyline` 输入；镜像 `wv-edit`
/// `command.rs::to_grid` 的桥）。无地形 → `None`。
pub fn terrain_grid_of(world: &wv_core::World, map: &Id) -> Option<TerrainGrid> {
    let data = world.terrain_of(map)?;
    let cells: Vec<TerrainCell> = data
        .cells
        .iter()
        .map(|c| TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    TerrainGrid::from_cells(data.width, data.height, cells)
}

/// 点到折线的最近距离（wu）。折线不足 2 点 → `f32::INFINITY`。
pub fn point_polyline_dist(p: Vec2, pts: &[Vec2]) -> f32 {
    pts.windows(2)
        .map(|w| point_seg_dist(p, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}

/// 点到线段的最短距离（经典垂足钳制）。
fn point_seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return p.distance(a); // 退化线段 = 点距。
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    let foot = a + ab * t;
    p.distance(foot)
}

/// 扁平折线 → 顶点列表（`Place.line` 的 `[x0,y0,x1,y1,…]` 视图）。
///
/// 奇偶不齐的脏数据按「能凑几对凑几对」截断（命中 / 拖拽 / 描画三处同款
/// 容错，单独提出来免得第四份拷贝）。
pub fn line_pts(line: &[f32]) -> Vec<Vec2> {
    line.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| Vec2::new(c[0], c[1]))
        .collect()
}

/// 线段 POI 命中：当前图锚定、且是线段 POI（`effective_line_kind` 判别，
/// ADR-0070 D2 唯一入口）的地点里，点到折线最近距离 ≤ `tol_wu` 者，
/// 取**最近**的一条。
pub fn line_poi_hit(world: &wv_core::World, map: &Id, local: Vec2, tol_wu: f32) -> Option<Id> {
    let mut best: Option<(f32, &Id)> = None;
    for place in &world.distances.places {
        if place.map.as_ref() != Some(map) {
            continue;
        }
        if effective_line_kind(place).is_none() {
            continue; // 点状地点 / 非线段 POI 不参与命中。
        }
        let Some(line) = &place.line else { continue };
        let pts: Vec<Vec2> = line
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| Vec2::new(c[0], c[1]))
            .collect();
        let d = point_polyline_dist(local, &pts);
        if d <= tol_wu && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, &place.id));
        }
    }
    best.map(|(_, id)| id.clone())
}

/// 确认钉点的产出：要入表的地点 + 要下切的参数包（`None` = 纯矢量）。
#[derive(Debug, Clone)]
pub struct ConfirmedLinePoi {
    /// 要创建的线段地点（`line` = 预览折线扁平化，首尾 = 作者精确钉点）。
    pub place: Place,
    /// 强制模式才有：沿线地形下切（strength = 1.0、总宽按宽度换算、
    /// `mark_water = kind == River`）。
    pub carve: Option<wv_edit::command::CarveSpec>,
}

/// 把定格的钉点折算成创建命令的原料（纯函数）。
///
/// 返回 `None` = 不可提交：预览不足 2 点（尚未规划 / 不可达 / 起终点同格）。
///
/// 字段口径（ADR-0070 D2）：
/// - `line_kind`：河写 `None`（缺省语义 = 河，YAML 不多一行噪音）、路写
///   `Some(Road)`（迁移器同款）；
/// - `line_width_m`：仅河且宽度 > 0 才落盘（道路不落盘此字段；0 = 未声明）；
/// - `rank`：创建期不落盘（按线长自动折算，`Place::effective_rank` 语义）；
/// - `x` / `y`：缺省 → 锚点由折线中点派生（迁移器同款）。
///
/// 参数多但全部正交（会话态 + 种类 + 模式 + 宽 + 名 + 图 + 比例尺 + 格 +
/// ID）——收成结构体只会把「从哪来」藏起来，故按仓库先例显式 allow。
#[allow(clippy::too_many_arguments)]
pub fn confirm_pending(
    pending: &PendingLinePoi,
    kind: LineKind,
    mode: LinePathMode,
    width_m: f32,
    name: &str,
    map: &Id,
    km_per_wu: Option<f32>,
    cell_size: f32,
    id: Id,
) -> Option<ConfirmedLinePoi> {
    if pending.preview.len() < 2 {
        return None; // 未规划 / 不可达 / 起终点同格：没有可成线的折线。
    }
    let pts: Vec<(f32, f32)> = pending.preview.iter().map(|p| (p.x, p.y)).collect();
    let place = Place {
        id,
        // 空名回退在 commit 层带 locale 兜底；这里保守取草稿原文。
        name: name.to_owned(),
        feature: None,
        map: Some(map.clone()),
        // x/y 缺省 → 锚点由折线中点派生（迁移器同款）。
        x: None,
        y: None,
        w: None,
        h: None,
        biome: None,
        envelope: None,
        rank: None, // 创建期不落盘：按线长自动折算。
        line: Some(pts.iter().flat_map(|(x, y)| [*x, *y]).collect()),
        kind: None,
        line_kind: match kind {
            // 河 = 缺省语义（YAML 不多一行噪音，迁移器同款）；路 = 显式 Road。
            LineKind::River => None,
            LineKind::Road => Some(LineKind::Road),
        },
        // 仅河且宽度 > 0 才落盘（道路不落盘此字段；0 = 未声明）。
        line_width_m: match kind {
            LineKind::River if width_m > 0.0 => Some(width_m),
            _ => None,
        },
    };
    // 强制模式 = 建线的同时把走廊压出来（一条命令一次撤销，ADR-0070 D3）。
    let carve = match mode {
        LinePathMode::Route => None,
        LinePathMode::Carve => Some(wv_edit::command::CarveSpec {
            pts,
            strength: CARVE_STRENGTH,
            width_cells: width_cells_of(width_m_to_wu(width_m, km_per_wu), cell_size),
            mark_water: kind == LineKind::River,
        }),
    };
    Some(ConfirmedLinePoi { place, carve })
}

/// 新线段 POI 的确定性可读 ID：`river_<h6>` / `road_<h6>`（种子 = 图 ID +
/// 起终点 + 尝试序号；撞名自动加长重试直到空位）。
///
/// 字符集只有小写十六进制与前缀字母——满足 AGENTS §4.4 的 `[A-Za-z0-9_]`。
pub fn new_line_poi_id(
    world: &wv_core::World,
    kind: LineKind,
    map_id: &str,
    start: Vec2,
    end: Vec2,
) -> Id {
    let prefix = match kind {
        LineKind::River => "river",
        LineKind::Road => "road",
    };
    // 种子 = 图 ID + 起终点 wu 位型 + 尝试序号；撞名（同图重画同一条线）
    // 加长重试直到空位——确定性成立（同输入首试必同 ID），又不会撞库。
    for attempt in 0_u64.. {
        let mut acc = preview_seed(map_id, cell_of(start, 1.0));
        acc ^= ((start.x.to_bits() ^ start.y.to_bits()) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        acc ^= ((end.x.to_bits() ^ end.y.to_bits()) as u64).rotate_left(17);
        acc ^= attempt.wrapping_mul(0xD1B5_4A32_D192_ED03);
        let h = splitmix64(acc);
        let id = if attempt == 0 {
            Id::new_unchecked(format!("{prefix}_{:06x}", h & 0xFF_FFFF))
        } else {
            // 撞名加长：12 位十六进制，碰撞空间够到「同起点同终点重画
            // 1600 万次」之外。
            Id::new_unchecked(format!(
                "{prefix}_{:06x}{:04x}",
                h & 0xFF_FFFF,
                (h >> 24) & 0xFFFF
            ))
        };
        if !world.distances.places.iter().any(|p| p.id == id) {
            return id;
        }
    }
    unreachable!("尝试序号空间内必有空位")
}

/// 把「确认钉点」落成一条 Create 事务（面板「确认」按钮与 Enter 共用）。
///
/// 一个事务 = 一次撤销（计划 Task 11：「undo 一次 = 一个 Create 事务」）。
/// 提交后走 [`crate::edit::road_sync::sync_derived_road_edges`]（T9：线即
/// 路网，提交后推导边要重建）。返回是否成功（失败已发 notice，V1 只提示）。
///
/// 世界**只经 `projection` 一个口子进**：先只读（比例尺 / 格尺寸 / 撞名
/// 去重），借还清后再可变提交——ResMut 的独占借用不允许调用方拆传两参。
#[allow(clippy::too_many_arguments)]
pub fn commit_pending_line(
    pending: &PendingLinePoi,
    kind: LineKind,
    mode: LinePathMode,
    width_m: f32,
    draft_name: &str,
    map: &Id,
    edit: &mut crate::edit::editing::EditHistory,
    projection: &mut WorldProjection,
    notice: &mut crate::ui_shell::ShellNotice,
    frame: u64,
) -> bool {
    let name = draft_name.trim();
    // ── 只读段：比例尺 / 格尺寸 / ID 去重 ──
    let world = &projection.world;
    let km = map_km_per_wu(world, map);
    let cell_size = grid_cell_size(world, map);
    // 强制模式必须有地形栅格可下切（V1 只提示不阻止——切绕行仍可建）。
    if mode == LinePathMode::Carve && cell_size.is_none() {
        notice.error(
            "这张图没有地形栅格，无法下切；可改用「绕行」模式创建",
            frame,
        );
        return false;
    }
    let id = new_line_poi_id(
        world,
        kind,
        map.as_str(),
        pending.start,
        *pending.preview.last().unwrap_or(&pending.start),
    );
    let fallback = match kind {
        LineKind::River => "河流",
        LineKind::Road => "道路",
    };
    let name = if name.is_empty() {
        // 空名兜底「河流 / 道路 + 短哈希」——表里不留无名实体（可读 ID 纪律）。
        format!("{fallback}_{}", &id.as_str()[id.as_str().len() - 6..])
    } else {
        name.to_owned()
    };
    let Some(confirmed) = confirm_pending(
        pending,
        kind,
        mode,
        width_m,
        &name,
        map,
        km,
        cell_size.unwrap_or(0.0),
        id,
    ) else {
        notice.error("两点落在同一格（或不可达），拉远一点再确认", frame);
        return false;
    };
    let label = confirmed.place.name.clone();
    let cmd = wv_edit::command::CreateLinePoiCommand::new(
        map.clone(),
        confirmed.place,
        confirmed.carve,
        cell_size.unwrap_or(0.0),
    );
    // ── 可变段：提交（只读借用已全部还清）──
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // T9：线即路网——提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
            notice.info(format!("已创建「{label}」"), frame);
            true
        }
        Err(e) => {
            notice.error(format!("创建失败：{e}"), frame);
            false
        }
    }
}

// ============================================================================
// 端点拖拽（ADR-0070 D4 · 计划 Task 12）：改道 / 延伸 / 汇流吸附
// ============================================================================
//
// 与钉点同款的纯函数纪律：状态机与几何全部可单测，Bevy 系统
// [`line_poi_tool_input`] 只做取参、gate 与回写。提交走**整线重发**
// `UpdatePlaceLineCommand` + 新增段下切 [`CarveSegmentCommand`]，两条命令
// 同一 `Transaction`——一次撤销整线与新增段下切全回；旧段下切不重算
// （一次性原则：下切是创建时的一次性动作，改道只补新段的账）。

/// 端点命中：首 / 末顶点在 `tol_wu` 内才起拖（中段顶点不算——拖中间点
/// 是后续任务的口子）；两端都中取更近者。折线不足两点 → `None`。
pub fn endpoint_hit(pts: &[Vec2], p: Vec2, tol_wu: f32) -> Option<DraggedEnd> {
    let first = pts.first()?;
    let last = pts.last()?;
    let (df, dl) = (p.distance(*first), p.distance(*last));
    match (df <= tol_wu, dl <= tol_wu) {
        (true, true) if df <= dl => Some(DraggedEnd::First),
        (true, _) => Some(DraggedEnd::First),
        (_, true) => Some(DraggedEnd::Last),
        _ => None,
    }
}

/// 起拖（纯函数）：记录原线为只读基准，预览从原线出发。
///
/// `target_cell` 取起拖时光标格——首帧预览 = 原线（光标还没走出起手格
/// 就不该有新路径）。
pub fn begin_endpoint_drag(
    id: Id,
    end: DraggedEnd,
    original: Vec<Vec2>,
    cursor_cell: (i32, i32),
) -> EndpointDrag {
    EndpointDrag {
        id,
        end,
        preview: original.clone(),
        original,
        target_cell: cursor_cell,
        snap: None,
    }
}

/// 拖拽预览重算（纯函数；返回**是否重算了**）。锚点 = 未动一侧的相邻
/// 顶点，从锚点向目标（光标或吸附点）按当前模式重新规划尾巴。
///
/// 缓存键 [`EndpointDrag::target_cell`] 在**规划失败**时同样推进（T11
/// `end_cell` 同语义：同格不重试，把「不可达」也钉进缓存防重算风暴）；
/// 失败时保留上一次的好预览——端点停在原地，不给作者「线断进山里」
/// 的错觉。
pub fn ensure_drag_preview(
    drag: &mut EndpointDrag,
    target: Vec2,
    grid: &TerrainGrid,
    cell_size: f32,
    mode: LinePathMode,
    seed: u64,
) -> bool {
    let n = drag.original.len();
    if n < 2 || cell_size <= 0.0 {
        return false;
    }
    let target_cell = cell_of(target, cell_size);
    // 缓存门在纯函数里（[`ensure_preview`] 同款）：同目标格零重算——
    // 测试直接数本函数的重算次数，系统调用点不必自建计数器。
    if drag.preview.len() >= 2 && drag.target_cell == target_cell {
        return false;
    }
    let anchor = match drag.end {
        DraggedEnd::Last => drag.original[n - 2],
        DraggedEnd::First => drag.original[1],
    };
    let raw = plan_polyline(
        grid,
        cell_size,
        (anchor.x, anchor.y),
        (target.x, target.y),
        mode,
        seed,
    );
    drag.target_cell = target_cell;
    if raw.is_empty() {
        return false; // 不可达 / 与锚点同格：保留旧预览。
    }
    let mut pts: Vec<Vec2> = raw.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    pts[0] = anchor; // 首尾替换为精确位置（plan_polyline 文档的调用方职责）。
    let last = pts.len() - 1;
    pts[last] = target;
    // 拼整线：固定段原样保留，被拖的旧端点掐掉（预览首/尾 = 精确位置）。
    drag.preview = match drag.end {
        DraggedEnd::Last => {
            let mut line = drag.original[..n - 1].to_vec();
            line.extend(pts[1..].iter().copied());
            line
        }
        DraggedEnd::First => {
            let mut line: Vec<Vec2> = pts.iter().rev().copied().collect();
            line.extend(drag.original[2..].iter().copied());
            line
        }
    };
    true
}

/// 汇流吸附（纯函数）：拖拽端点落进**另一条河**折线的 `tol_wu` 容差内
/// → 返回 `(目标线 ID, 最近点)`；吸附 = 端点替换为最近点（v1 仅几何
/// 重合，不建拓扑）。自己排除（不能吸给自己）；道路线不参与（v1 只做
/// 河-河汇流；路网端点吸附由推导边按锚点容差自行处理，T9）。
pub fn river_snap(
    world: &wv_core::World,
    map: &Id,
    exclude: &Id,
    p: Vec2,
    tol_wu: f32,
) -> Option<(Id, Vec2)> {
    let mut best: Option<(f32, &Id, Vec2)> = None;
    for place in &world.distances.places {
        if &place.id == exclude || place.map.as_ref() != Some(map) {
            continue;
        }
        if effective_line_kind(place) != Some(LineKind::River) {
            continue; // 点状地点 / 道路线不吸附。
        }
        let Some(line) = &place.line else { continue };
        let (d, foot) = closest_point_on_polyline(p, &line_pts(line));
        if d <= tol_wu && best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, &place.id, foot));
        }
    }
    best.map(|(_, id, foot)| (id.clone(), foot))
}

/// 折线上距 `p` 最近的点（`(距离, 垂足)`）；折线不足 2 点 → 无穷远。
fn closest_point_on_polyline(p: Vec2, pts: &[Vec2]) -> (f32, Vec2) {
    let mut best = (f32::INFINITY, p);
    for w in pts.windows(2) {
        let (d, foot) = closest_point_on_seg(p, w[0], w[1]);
        if d < best.0 {
            best = (d, foot);
        }
    }
    best
}

/// 线段上距 `p` 最近的点（经典垂足钳制）。
fn closest_point_on_seg(p: Vec2, a: Vec2, b: Vec2) -> (f32, Vec2) {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return (p.distance(a), a); // 退化线段 = 点距。
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    let foot = a + ab * t;
    (p.distance(foot), foot)
}

/// 预览**被拖端的顶点**直写为 `point`（I3 · 评审 fix round 1）。
///
/// 末点是精确坐标、直写零成本——同格内吸附点出现 / 消失不触发格门
/// （重规划只依赖格），但末点必须跟住当前目标，否则「看到环、松手不吸」
/// （反向：环没了末点仍吸在旧环上）。调用方两处：交互系统逐帧对齐当前
/// 目标（描画与提交同源），[`commit_endpoint_drag`] 入口以会话态 snap
/// 收口（松手帧不经过交互系统的对齐块，见该处注释）。
pub fn align_preview_tail(drag: &mut EndpointDrag, point: Vec2) {
    let slot = match drag.end {
        DraggedEnd::Last => drag.preview.last_mut(),
        DraggedEnd::First => drag.preview.first_mut(),
    };
    if let Some(tail) = slot {
        *tail = point;
    }
}

/// 拖拽提交计划（纯函数）：整线 + 「分叉点起的新增段」。
///
/// 新增段 = 新折线中**不再与原线重合**的部分（从未动的一端回找最后一致
/// 的顶点为分叉点）。喂 `carve_corridor` 的只有这一段——旧段下切保留原样
/// （一次性原则），与旧段共享的前缀也不重复下切。
#[derive(Debug, Clone, PartialEq)]
pub struct DragCommitPlan {
    /// 提交后的整线（图内局部 wu；`UpdatePlaceLineCommand` 的载荷）。
    pub new_line: Vec<(f32, f32)>,
    /// 分叉点起的新增段（`carve_corridor` 的中轴）。
    pub new_segment: Vec<(f32, f32)>,
}

/// 从拖拽会话出提交计划；`None` = 不可提交（预览退化，或拖回原位无新增段
/// ——无变化不发命令、不进撤销栈）。
///
/// # 公式不变量（评审 C1/I2 防线 · fix round 1）
///
/// `fork_index` 的返回值被用作**含端切片**边界（`new[..=fork]` 或
/// `new[fork..]`），且 `new.len() ≥ 2` 由上方守卫保证。首端（`from_start`）
/// 分支的两种退出都不得下溢：
/// - 回找**命中至少一个顶点后**停住（`i ≥ 1`）→ 分叉 = 最后一致的顶点，
///   正向小标 `new.len() - i`（与 Last 端的 `i - 1` 对称——多 match 了 i 个，
///   最后一个 match 在 `new.len() - i`；旧公式 `new.len() - 1 - i` 比锚点
///   少 1，`new_segment` 漏掉「锚点 → 第一个新顶点」段，Carve 模式下第一
///   段不下切）；
/// - **原地**（预览 == 原线，循环走满 `i == common`）→ 等长时
///   `new.len() - i == 0` → 单点段 → 上方 `len < 2` 守卫拦成 `None`
///   （旧公式在此 usize 下溢，普通「点了没拖」即崩）；
/// - `i == 0`（固定端第一对就不一致——构造契约外的防御兜底）→
///   `new.len() - 1`，整线都是「新增段」（保守全切，好过下溢）。
///
/// # 与旧走廊的格集关系（评审 M1 记档 · v1 接受）
///
/// 新段按**顶点前缀求差**，不是格集求差：新段中途**横穿旧走廊**的格会被
/// 二次下切（`carve_corridor` 非幂等——只降不升，再过一遍会再挖深）；v1
/// 接受该偏差，旧走廊 ∩ 新走廊的回填留后续。
pub fn drag_commit_plan(drag: &EndpointDrag) -> Option<DragCommitPlan> {
    if drag.preview.len() < 2 {
        return None;
    }
    let new_line: Vec<(f32, f32)> = drag.preview.iter().map(|p| (p.x, p.y)).collect();
    let from_start = drag.end == DraggedEnd::First;
    let fork = fork_index(&drag.original, &drag.preview, from_start);
    let new_segment: Vec<(f32, f32)> = if from_start {
        drag.preview[..=fork].iter().map(|p| (p.x, p.y)).collect()
    } else {
        drag.preview[fork..].iter().map(|p| (p.x, p.y)).collect()
    };
    if new_segment.len() < 2 {
        return None; // 拖回原位 / 预览没动：无可提交的新增段。
    }
    Some(DragCommitPlan {
        new_line,
        new_segment,
    })
}

/// 分叉点下标：从未动的一端起，新旧折线最后一致的顶点在 `new` 中的下标。
///
/// 返回值是**含端切片**边界（不变量与退出分析见 [`drag_commit_plan`] 的
/// 公式不变量一节——评审 C1/I2：首端 `new.len() - i`、原地单点段由调用方
/// 守卫拦下、`i == 0` 兜底取整线，任何路径都不得下溢）。坐标相等按
/// `1e-3` wu 容差判（Chaikin / f32 噪声）。
fn fork_index(old: &[Vec2], new: &[Vec2], from_start: bool) -> usize {
    const EPS2: f32 = 1e-3 * 1e-3;
    let common = old.len().min(new.len());
    let mut i = 0;
    while i < common {
        let (o, n) = if from_start {
            (old[old.len() - 1 - i], new[new.len() - 1 - i])
        } else {
            (old[i], new[i])
        };
        if o.distance_squared(n) > EPS2 {
            break;
        }
        i += 1;
    }
    if from_start {
        if i == 0 {
            new.len() - 1 // 防御兜底：整线视为新增段（新.len() ≥ 2 已由调用方守卫）。
        } else {
            new.len() - i // 最后一致的顶点 = 锚点；i ≤ common ≤ new.len()，无下溢。
        }
    } else {
        i.saturating_sub(1)
    }
}

/// 端点拖拽「新增段下切」的应用层内部命令（ADR-0070 D4 · 计划 Task 12）。
///
/// ## 为什么在 wv-app 而不是 wv-edit
///
/// v1 的下切粒度是「整线重发 + 新增段差量」——差量来自交互会话（旧折线 vs
/// 新折线的分叉，[`drag_commit_plan`]），wv-edit 的命令族没有这个概念。
/// 仿 `road_sync::MigrateCarrier` 先例：应用层实现 `EditCommand`，仍走
/// `Transaction → History` 唯一编辑链（AGENTS §3.3），不绕过命令栈。
/// 快照 / diff / 回放与 `CreateLinePoiCommand` 的下切半区逐位同款。
///
/// ## 撤销语义
///
/// 本命令与 `UpdatePlaceLineCommand` 同一事务：撤销一次 = 整线回退 +
/// 新增段下切逐格回放，世界与拖拽前逐位一致。
#[derive(Debug)]
pub struct CarveSegmentCommand {
    map: Id,
    spec: wv_edit::command::CarveSpec,
    cell_size: f32,
    /// apply 时缓存的受下切格旧值：`(gx, gy, before_cell)`。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl CarveSegmentCommand {
    /// 从 `(map, spec, cell_size)` 一步构造。
    pub fn new(map: Id, spec: wv_edit::command::CarveSpec, cell_size: f32) -> Self {
        Self {
            map,
            spec,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }
}

/// 把 `TerrainGrid` 写回世界（`terrain_grid_of` 的逆桥；wv-edit 命令内部
/// `from_grid` 的应用侧镜像）。
fn store_grid(world: &mut wv_core::World, map: &Id, grid: &TerrainGrid) {
    let cells = grid
        .cells()
        .iter()
        .map(|c| wv_core::terrain::TerrainCellData {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    world.set_terrain(
        map.clone(),
        wv_core::terrain::TerrainData {
            width: grid.width(),
            height: grid.height(),
            cells,
        },
    );
}

impl EditCommand for CarveSegmentCommand {
    fn label(&self) -> String {
        format!("改道下切新段（{}）", self.map)
    }

    fn apply(&self, world: &mut wv_core::World) -> wv_edit::command::EditResult<Vec<String>> {
        let Some(mut grid) = terrain_grid_of(world, &self.map) else {
            return Err(wv_edit::command::EditError::TerrainMismatch(format!(
                "地图 `{}` 无地形网格",
                self.map
            )));
        };
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        carve_corridor(
            &mut grid,
            &self.spec.pts,
            self.spec.strength,
            self.cell_size,
            self.spec.width_cells,
            self.spec.mark_water,
        );
        let mut before = self
            .before
            .lock()
            .map_err(|_| wv_edit::command::EditError::Precondition("命令锁中毒".into()))?;
        before.clear();
        let width = grid.width() as usize;
        for (idx, (old, new)) in snapshot.iter().zip(grid.cells().iter()).enumerate() {
            if old != new {
                let gx = (idx % width) as i32;
                let gy = (idx / width) as i32;
                before.push((gx, gy, *old));
            }
        }
        drop(before);
        store_grid(world, &self.map, &grid);
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut wv_core::World) -> wv_edit::command::EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| wv_edit::command::EditError::Precondition("命令锁中毒".into()))?
            .clone();
        if before.is_empty() {
            return Ok(()); // 未 apply 过 / 无格变动 → 原样。
        }
        let Some(mut grid) = terrain_grid_of(world, &self.map) else {
            return Err(wv_edit::command::EditError::TerrainMismatch(format!(
                "地图 `{}` 无地形网格",
                self.map
            )));
        };
        for (gx, gy, cell) in &before {
            grid.set(*gx, *gy, *cell);
        }
        store_grid(world, &self.map, &grid);
        Ok(())
    }
}

/// 端点拖拽的松手提交（系统松手帧与测试共用；[`commit_pending_line`]
/// 的拖拽对偶）。
///
/// 一条事务 = `UpdatePlaceLineCommand`（整线重发）+（下切模式且有地形时）
/// [`CarveSegmentCommand`]（仅新增段）——**一次撤销整线 + 两段下切全回；
/// 旧段下切保留原样**（一次性原则：创建时的下切是已发生的账，改道只补
/// 新段的账）。绕行模式（Route）只重发整线，不碰地形。
///
/// 世界只经 `projection` 一个口子进（与 [`commit_pending_line`] 同款）；
/// 提交后走 [`crate::edit::road_sync::sync_derived_road_edges`]（T9）。
/// 返回是否成功（无变化 / 失败均 `false`，失败已发 notice）。
pub fn commit_endpoint_drag(
    drag: &mut EndpointDrag,
    mode: LinePathMode,
    map: &Id,
    edit: &mut crate::edit::editing::EditHistory,
    projection: &mut WorldProjection,
    notice: &mut crate::ui_shell::ShellNotice,
    frame: u64,
) -> bool {
    // I3（评审 fix round 1）：松手帧以**会话态 snap** 为准收口末点——吸附点
    // 可能落在「上次重算」的同格内，格门不触发，preview 末点还停在旧目标上
    // （看到环、松手不吸）。直写末点零成本（不绕格门重跑 A*——路径只依赖
    // 格，末点归零误差）；描画侧的逐帧对齐见交互系统同款直写。
    if let Some((_, sp)) = drag.snap {
        align_preview_tail(drag, sp);
    }
    let Some(plan) = drag_commit_plan(drag) else {
        return false; // 拖回原位 / 预览退化：不发命令、不进撤销栈。
    };
    let world = &projection.world;
    let Some(place) = world
        .distances
        .places
        .iter()
        .find(|p| p.id == drag.id)
        .cloned()
    else {
        notice.error("选中的线已不在表里（被撤销删除等）", frame);
        return false;
    };
    // 拖了又拖回原位（折线逐点一致）：同「无真变化」，不发命令。
    let new_flat: Vec<f32> = plan.new_line.iter().flat_map(|(x, y)| [*x, *y]).collect();
    if place.line.as_deref() == Some(new_flat.as_slice()) {
        return false;
    }
    let km = map_km_per_wu(world, map);
    let cell_size = grid_cell_size(world, map);
    let carve = match mode {
        LinePathMode::Route => None,
        LinePathMode::Carve => {
            let Some(cell) = cell_size else {
                notice.error(
                    "这张图没有地形栅格，无法下切新段；可改用「绕行」模式拖拽",
                    frame,
                );
                return false;
            };
            Some(wv_edit::command::CarveSpec {
                pts: plan.new_segment,
                strength: CARVE_STRENGTH,
                width_cells: width_cells_of(
                    width_m_to_wu(place.line_width_m.unwrap_or(0.0), km),
                    cell,
                ),
                mark_water: effective_line_kind(&place) != Some(LineKind::Road),
            })
        }
    };
    let name = place.name.clone();
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::UpdatePlaceLineCommand::new(
        drag.id.clone(),
        Some(new_flat),
    )));
    if let Some(spec) = carve {
        tx.push(Box::new(CarveSegmentCommand::new(
            map.clone(),
            spec,
            cell_size.unwrap_or(0.0),
        )));
    }
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // T9：线即路网——折线变了，推导边要重建（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
            notice.info(format!("已改道「{name}」"), frame);
            true
        }
        Err(e) => {
            notice.error(format!("改道失败：{e}"), frame);
            false
        }
    }
}

/// 线段 POI 交互系统（`Update`；排在 `track_cursor_world` 之后）。
///
/// Gate 链（任一命中即退，顺序有意）：
/// 1. 工具未激活 → 弃会话（切工具残留的 pending / 选中一并清）；
/// 2. 加载遮罩 → 不消费输入；
/// 3. **播放锁**（T10 评审 minor ①）→ 播放中不消费输入、不重算预览
///    （已开的预览原样保留，恢复暂停后继续）。
#[allow(clippy::too_many_arguments)]
pub fn line_poi_tool_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<crate::app_screen::AppScreen>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut line_poi: ResMut<LinePoiState>,
    mut projection: ResMut<WorldProjection>,
    mut session: Local<PinSession>,
    time: Res<Time>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
) {
    // ① 工具未激活 → 弃会话（切工具残留的钉点 / 选中 / 拖拽一并清：面板随
    //    工具收起，残留选中再无出口）。
    if !line_poi.tool_active {
        if line_poi.pending.take().is_some()
            || line_poi.selected.take().is_some()
            || line_poi.drag.take().is_some()
            || *session != PinSession::default()
        {
            *session = PinSession::default();
        }
        return;
    }
    // ② 加载遮罩：不消费输入。
    if screen.shows_loading_overlay() {
        return;
    }
    // ③ 播放锁（T10 评审 minor ①）：播放中不消费输入、**不重算预览**——
    //    已开的预览原样保留（pending / 拖拽会话在态里，恢复暂停即继续）。
    //    唯一放行的是拖拽的「松手」裁决（T12）：播放中松手 = 弃拖拽不提交
    //    （与面板 `panel_can_commit` 同口径——提交必须闸，会话本身可留）。
    let drag_release = buttons.just_released(MouseButton::Left) && line_poi.drag.is_some();
    if playback.playing && !drag_release {
        return;
    }
    // 面板「确认」可能在本系统之外清了 pending（egui pass 晚于 Update）：
    // pending 没了就把拍拉回 Idle，防止「幽灵 Following」吃掉下一次点击。
    if line_poi.pending.is_none() && session.phase != PinPhase::Idle {
        session.phase = PinPhase::Idle;
    }

    // 变图即弃（测量同款：钉点不能跨图）。R1：selected 与 pending 同口径
    // 同弃——旧图选中残留 = 幽灵高亮 + 错比例尺写回（数据级错误）。
    let map = match drill.current().cloned() {
        Some(m) => {
            let reset = reset_on_map_change(&session, Some(&m));
            if reset.rebind {
                *session = PinSession {
                    phase: PinPhase::Idle,
                    map: Some(m.clone()),
                };
            }
            if reset.clear_pending {
                line_poi.pending = None;
            }
            if reset.clear_selected {
                line_poi.selected = None;
            }
            if reset.clear_drag {
                line_poi.drag = None;
            }
            m
        }
        None => return,
    };

    let frame = (time.elapsed_secs() * 60.0) as u64;

    // ── 松手：拖拽提交（T12）。播放中松手在下面的 gate 已放行到这里，
    //    但提交必须闸（与面板 `panel_can_commit` 同口径）：播放中弃拖拽。──
    if drag_release {
        if let Some(mut drag) = line_poi.drag.take() {
            if !playback.playing {
                commit_endpoint_drag(
                    &mut drag,
                    line_poi.mode,
                    &map,
                    &mut edit,
                    &mut projection,
                    &mut notice,
                    frame,
                );
            }
        }
        return;
    }

    // ── Esc：优先级 拖拽 > 钉点 > 选中 ──
    // （必须用 Bevy 的 `KeyCode`——`ButtonInput<egui::Key>` 没有维护者，
    //   cell_stroke 的 M2 教训；`wants_keyboard` = 作者在输入框里打字，
    //   Esc 归 egui，不归地图手势。）
    if keys.just_pressed(KeyCode::Escape) && !ui.wants_keyboard {
        if line_poi.drag.take().is_some() {
            // 拖拽中 Esc = 弃拖拽：未提交，世界原样；选中保留（面板还在）。
            return;
        }
        if session.phase != PinPhase::Idle {
            cancel_pending(&mut line_poi.pending);
            session.phase = PinPhase::Idle;
            return;
        }
        line_poi.selected = None;
        return;
    }

    // ── Enter：Pinned 拍 = 确认（与面板「确认」按钮同一条提交路）──
    if keys.just_pressed(KeyCode::Enter) && !ui.wants_keyboard && session.phase == PinPhase::Pinned
    {
        if let Some(pending) = line_poi.pending.clone() {
            let ok = commit_pending_line(
                &pending,
                line_poi.kind,
                line_poi.mode,
                line_poi.width_m,
                &line_poi.name,
                &map,
                &mut edit,
                &mut projection,
                &mut notice,
                frame,
            );
            if ok {
                line_poi.pending = None;
                session.phase = PinPhase::Idle; // 连画：工具保持激活。
            }
        }
        return;
    }

    let Some(cursor) = view.cursor_world else {
        return;
    };
    let node = drill.current_node(&projection.world);
    let origin = node
        .map(|n| wv_render::drill::map_origin(&projection.world, n, &grid))
        .unwrap_or(grid.map_origin);
    let local = cursor - origin;

    // ── 预览重算（格级缓存，防每帧 A*）──
    // - Following：光标跨格（或预览被模式切换作废）→ 按光标重算；
    // - Pinned：预览被模式切换作废 → 按**定格终点**重算一次（终点取格心，
    //   ≤ 半格的精度损失——精确钉点只在首次 PinEnd 时回填，不另设字段）。
    // 网格 clone（大图 ≈ MB 级）只在 `need` 为真时发生——同格挪动帧零拷贝
    // （评审 Minor 1：clone 在 need 判定外 = 白拷）。
    let mode = line_poi.mode;
    if let Some(pending) = &mut line_poi.pending {
        let cell = grid.cell_size;
        let cursor_cell = cell_of(local, cell);
        let need = match session.phase {
            PinPhase::Following => {
                pending.preview.is_empty()
                    || pin_on_move(&session, pending.end_cell, cursor_cell) == Some(cursor_cell)
            }
            PinPhase::Pinned => pending.preview.is_empty(),
            PinPhase::Idle => false,
        };
        if need {
            if let Some(terrain) = terrain_grid_of(&projection.world, &map) {
                let end_pt = if session.phase == PinPhase::Following {
                    local
                } else {
                    Vec2::new(
                        (pending.end_cell.0 as f32 + 0.5) * cell,
                        (pending.end_cell.1 as f32 + 0.5) * cell,
                    )
                };
                let seed = preview_seed(map.as_str(), cell_of(pending.start, cell));
                ensure_preview(pending, end_pt, &terrain, cell, mode, seed);
            }
        }
    }

    // ── 拖拽预览重算（T12：汇流吸附 + 格级缓存，防每帧 A*）──
    // 吸附检测是廉价几何（逐帧无妨）；重规划走 T11 同款格门——目标点
    // （光标或吸附点）跨格才重算。拖拽中的预览跟随吸附点，松手提交即吸附点。
    if let Some(drag) = &mut line_poi.drag {
        let cell = grid.cell_size;
        let tol = SELECT_TOLERANCE_PX * view.scale.max(0.25);
        let snapped = river_snap(&projection.world, &map, &drag.id, local, tol);
        let target = snapped.as_ref().map_or(local, |(_, p)| *p);
        // 网格 clone（大图 ≈ MB 级）只在跨格时发生；同格内由
        // `ensure_drag_preview` 的缓存门直接短路（零重算）。
        let stale = drag.target_cell != cell_of(target, cell);
        drag.snap = snapped;
        // I3（描画侧）：末点恒等于当前目标（吸附点或光标）——吸附在同格
        // 内出现 / 消失时环、线、松手提交三者一致（提交侧的权威收口在
        // `commit_endpoint_drag` 入口：松手帧不经过本块）。同格内的普通
        // 拖动因此即时跟手，松手会提交 ≤1 格的端点微移（旧行为是静默
        // no-op——拖了没反应反而难读）。
        align_preview_tail(drag, target);
        if stale {
            if let Some(terrain) = terrain_grid_of(&projection.world, &map) {
                let n = drag.original.len();
                let anchor = match drag.end {
                    DraggedEnd::Last => drag.original[n.saturating_sub(2)],
                    DraggedEnd::First => drag.original[1.min(n.saturating_sub(1))],
                };
                let seed = preview_seed(map.as_str(), cell_of(anchor, cell));
                ensure_drag_preview(drag, target, &terrain, cell, mode, seed);
            }
        }
    }

    // ── 左键点击 ──
    if !buttons.just_pressed(MouseButton::Left) || ui.wants_pointer {
        return;
    }
    // 拖拽进行中：点击不参与（拖拽由移动 + 松手驱动，T12）。
    if line_poi.drag.is_some() {
        return;
    }
    // 端点命中优先（Idle + 已选中）：起拖。拖中段顶点不是本任务的口子，
    // 命中阈值与选中同源（屏幕 8 px × scale）。
    if session.phase == PinPhase::Idle {
        if let Some(id) = line_poi.selected.clone() {
            let pts = projection
                .world
                .distances
                .places
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| p.line.as_deref())
                .map(line_pts);
            let tol = SELECT_TOLERANCE_PX * view.scale.max(0.25);
            if let Some(pts) = pts {
                if let Some(end) = endpoint_hit(&pts, local, tol) {
                    line_poi.drag = Some(begin_endpoint_drag(
                        id,
                        end,
                        pts,
                        cell_of(local, grid.cell_size),
                    ));
                    return;
                }
            }
        }
    }
    // 命中检测只在 Idle 拍有意义（Following 的点击一律定终点）。
    let hit = if session.phase == PinPhase::Idle {
        let tol = SELECT_TOLERANCE_PX * view.scale.max(0.25);
        line_poi_hit(&projection.world, &map, local, tol)
    } else {
        None
    };
    match pin_on_click(&session, local, hit) {
        PinClick::Begin { start } => {
            let cell = grid.cell_size;
            // R2：起钉同时弃选中 + 清名称草稿（不静默继承选中线的名字；
            // 宽度不动 = 保留「上次用值」）。先解构再借字段——ResMut 的
            // 不同字段不能在同一次调用里各借一个 &mut。
            let LinePoiState {
                ref mut pending,
                ref mut selected,
                ref mut name,
                ..
            } = *line_poi;
            begin_pin(
                pending,
                selected,
                name,
                &mut session,
                start,
                cell_of(start, cell),
            );
            // 立即规划一次（同格 = 单点占位，不出橡皮筋——起终点同格本就无路）。
            if let Some(terrain) = terrain_grid_of(&projection.world, &map) {
                if let Some(pending) = &mut line_poi.pending {
                    let seed = preview_seed(map.as_str(), cell_of(start, cell));
                    ensure_preview(pending, local, &terrain, cell, mode, seed);
                }
            }
        }
        PinClick::PinEnd { end } => {
            if let Some(pending) = &mut line_poi.pending {
                pending.end_cell = cell_of(end, grid.cell_size);
                // 预览末点换成精确钉点（≤ 半格的缓存偏差在这里归零）。
                if pending.preview.len() >= 2 {
                    if let Some(last) = pending.preview.last_mut() {
                        *last = end;
                    }
                }
            }
            session.phase = PinPhase::Pinned;
        }
        PinClick::Select { id } => {
            line_poi.selected = Some(id.clone());
            // 面板字段填充（名称 + 显示宽度）：选中态表单的草稿来源。
            if let Some(place) = projection
                .world
                .distances
                .places
                .iter()
                .find(|p| p.id == id)
            {
                let km = map_km_per_wu(&projection.world, &map);
                let (name, width) = crate::ui::line_poi_panel::fill_fields_from_place(place, km);
                line_poi.name = name;
                line_poi.width_m = width;
            }
        }
        PinClick::Ignore => {}
    }
}

/// 预览描画（`Update`，与测量 / 选择草稿同层）：钉点预览折线 + 起终点圆点 +
/// 选中线高亮。全部 gizmo 即帧即弃，不建 mesh 资产（历史坑：每帧重建 mesh
/// = 资产泄漏）。
#[allow(clippy::too_many_arguments)]
pub fn draw_line_poi_preview(
    mut gizmos: Gizmos,
    line_poi: Res<LinePoiState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    projection: Res<WorldProjection>,
) {
    if !line_poi.tool_active || view.scale <= 0.0 {
        return;
    }
    // 颜色按种类走：河 = 水青、路 = 土金（会话预览色，非主题数据）。
    let (line_color, dot_color) = match line_poi.kind {
        LineKind::River => (Color::srgb(0.30, 0.62, 0.86), Color::srgb(0.55, 0.80, 0.95)),
        LineKind::Road => (Color::srgb(0.85, 0.65, 0.30), Color::srgb(0.95, 0.82, 0.55)),
    };
    let to_view = |p: Vec2| grid.map_to_view(p);
    let dot_r = (SELECT_TOLERANCE_PX * 0.6 * view.scale).max(0.5);

    // 选中线高亮：金白描边（给「面板正在编辑哪条」一个明确锚）。拖拽中的
    // 那条让位给下方的拖拽预览——新旧两线叠影读不出「改成了什么」。
    let dragging_selected = line_poi
        .selected
        .as_ref()
        .is_some_and(|id| line_poi.drag.as_ref().is_some_and(|d| &d.id == id));
    if let (Some(id), false) = (&line_poi.selected, dragging_selected) {
        if let Some(place) = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| &p.id == id)
        {
            if let Some(line) = &place.line {
                let pts = line_pts(line);
                for w in pts.windows(2) {
                    gizmos.line_2d(to_view(w[0]), to_view(w[1]), Color::srgb(1.0, 0.85, 0.40));
                }
                for p in &pts {
                    gizmos.circle_2d(
                        Isometry2d::from_translation(to_view(*p)),
                        dot_r,
                        Color::srgb(1.0, 0.85, 0.40),
                    );
                }
            }
        }
    }

    // 端点拖拽（T12）：新折线 + 汇流吸附目标高亮 + 拖动端点圆点。全部
    // gizmo 即帧即弃（每帧重建 mesh 资产 = 泄漏，历史坑）。颜色按被拖线
    // 的种类走（与工具当前种类可能不一致——选中后切了种类档）。
    if let Some(drag) = &line_poi.drag {
        let kind = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id == drag.id)
            .map(|p| effective_line_kind(p).unwrap_or(LineKind::River))
            .unwrap_or(line_poi.kind);
        let (drag_color, drag_dot) = match kind {
            LineKind::River => (Color::srgb(0.30, 0.62, 0.86), Color::srgb(0.55, 0.80, 0.95)),
            LineKind::Road => (Color::srgb(0.85, 0.65, 0.30), Color::srgb(0.95, 0.82, 0.55)),
        };
        // 吸附目标：亮青描边 + 吸附点圆环（「松手会吸到这」的明确预告）。
        if let Some((sid, sp)) = &drag.snap {
            let snap_color = Color::srgb(0.55, 0.95, 0.90);
            if let Some(place) = projection
                .world
                .distances
                .places
                .iter()
                .find(|p| &p.id == sid)
            {
                if let Some(line) = &place.line {
                    for w in line_pts(line).windows(2) {
                        gizmos.line_2d(to_view(w[0]), to_view(w[1]), snap_color);
                    }
                }
            }
            gizmos.circle_2d(
                Isometry2d::from_translation(to_view(*sp)),
                dot_r * 1.6,
                snap_color,
            );
        }
        // 拖拽中的新折线（旧线仍按选中高亮让位逻辑隐藏）。
        if drag.preview.len() >= 2 {
            for w in drag.preview.windows(2) {
                gizmos.line_2d(to_view(w[0]), to_view(w[1]), drag_color);
            }
        }
        // 拖动端点：实心大圆点（哪端在手上要一眼可辨）。
        if let Some(tip) = match drag.end {
            DraggedEnd::Last => drag.preview.last(),
            DraggedEnd::First => drag.preview.first(),
        } {
            gizmos.circle_2d(
                Isometry2d::from_translation(to_view(*tip)),
                dot_r * 1.5,
                drag_dot,
            );
        }
    }

    // 钉点预览：折线 + 起终点圆点（起点实心、终点空心感 = 半径大一圈）。
    let Some(pending) = &line_poi.pending else {
        return;
    };
    if pending.preview.len() >= 2 {
        let pts: Vec<Vec2> = pending.preview.iter().map(|p| to_view(*p)).collect();
        for w in pts.windows(2) {
            gizmos.line_2d(w[0], w[1], line_color);
        }
    }
    gizmos.circle_2d(
        Isometry2d::from_translation(to_view(pending.start)),
        dot_r,
        dot_color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 夹具 ────────────────────────────────────────────────────────

    /// 32×32 平地网格（海拔 100，全陆）。
    fn flat_grid() -> TerrainGrid {
        let cells = vec![
            TerrainCell {
                height: 100,
                surface: 1,
                biome: 0,
            };
            32 * 32
        ];
        TerrainGrid::from_cells(32, 32, cells).expect("夹具尺寸合法")
    }

    fn map_id() -> Id {
        Id::new_unchecked("map_a")
    }

    /// 走完「起钉 → 移动 → 定格」到 Pinned 拍，返回（pending, session）。
    fn pinned_session() -> (PendingLinePoi, PinSession) {
        let grid = flat_grid();
        let cell = 8.0_f32;
        let mut session = PinSession {
            phase: PinPhase::Idle,
            map: Some(map_id()),
        };
        let start = Vec2::new(40.0, 40.0);
        let click1 = pin_on_click(&session, start, None);
        assert!(matches!(click1, PinClick::Begin { .. }), "第一击应起钉");
        let mut pending = PendingLinePoi {
            start,
            preview: Vec::new(),
            end_cell: cell_of(start, cell),
        };
        session.phase = PinPhase::Following;
        // 光标移到 (200, 40)：跨格 → 重算。
        ensure_preview(
            &mut pending,
            Vec2::new(200.0, 40.0),
            &grid,
            cell,
            LinePathMode::Route,
            7,
        );
        let click2 = pin_on_click(&session, Vec2::new(200.0, 40.0), None);
        assert!(matches!(click2, PinClick::PinEnd { .. }), "第二击应定终点");
        session.phase = PinPhase::Pinned;
        (pending, session)
    }

    // ── 计划 Task 11 Step 1 · 测试 1 ────────────────────────────────

    /// 钉两点后确认：河 + 强制模式 → CreateLinePoi 原料带 CarveSpec
    /// （strength 1.0、总宽换算、mark_water = true），折线 ≥ 2 点。
    #[test]
    fn pin_two_points_then_confirm_emits_create_command() {
        let (pending, _session) = pinned_session();
        assert!(
            pending.preview.len() >= 2,
            "平地上绕行应可规划，实际 {} 点",
            pending.preview.len()
        );

        // 河 + 强制模式：带 CarveSpec，mark_water = true。
        let out = confirm_pending(
            &pending,
            LineKind::River,
            LinePathMode::Carve,
            60.0,
            "青龙河",
            &map_id(),
            None,
            8.0,
            Id::new_unchecked("river_000001"),
        )
        .expect("预览 ≥ 2 点应可确认");
        let carve = out.carve.expect("强制模式必须带 CarveSpec");
        assert!(
            (carve.strength - 1.0).abs() < 1e-6,
            "strength 应为 1.0，实际 {}",
            carve.strength
        );
        assert!(carve.mark_water, "河的下切必须写水面");
        assert_eq!(
            carve.width_cells, 8,
            "60 wu ÷ 8 wu/格 = 7.5 → round = 8，实际 {}",
            carve.width_cells
        );
        assert!(carve.pts.len() >= 2, "下切折线至少 2 点");
        // 地点字段：河写 None（缺省语义）、宽落盘、锚点缺省。
        assert_eq!(out.place.line_kind, None, "河的 line_kind 走缺省 None");
        assert_eq!(out.place.line_width_m, Some(60.0));
        assert_eq!(
            out.place.line.as_ref().map(|l| l.len() / 2),
            Some(pending.preview.len())
        );
        assert!(
            out.place.x.is_none() && out.place.y.is_none(),
            "锚点应由折线中点派生"
        );

        // 路 + 强制模式：mark_water = false、line_kind = Some(Road)、宽不落盘。
        let road = confirm_pending(
            &pending,
            LineKind::Road,
            LinePathMode::Carve,
            60.0,
            "官道",
            &map_id(),
            None,
            8.0,
            Id::new_unchecked("road_000001"),
        )
        .expect("路同款可确认");
        let carve = road.carve.expect("路强制模式也带 CarveSpec");
        assert!(!carve.mark_water, "路的下切不写水面");
        assert_eq!(road.place.line_kind, Some(LineKind::Road));
        assert_eq!(road.place.line_width_m, None, "道路不落盘宽度");

        // 绕行模式：carve = None（纯矢量创建）。
        let route = confirm_pending(
            &pending,
            LineKind::River,
            LinePathMode::Route,
            60.0,
            "青龙河",
            &map_id(),
            None,
            8.0,
            Id::new_unchecked("river_000002"),
        )
        .expect("绕行同款可确认");
        assert!(route.carve.is_none(), "绕行模式不下切");
    }

    /// 预览不足 2 点（同格 / 未规划）→ 不可确认。
    #[test]
    fn confirm_rejects_degenerate_preview() {
        let mut pending = PendingLinePoi {
            start: Vec2::new(40.0, 40.0),
            preview: Vec::new(),
            end_cell: (5, 5),
        };
        assert!(confirm_pending(
            &pending,
            LineKind::River,
            LinePathMode::Route,
            0.0,
            "x",
            &map_id(),
            None,
            8.0,
            Id::new_unchecked("river_x")
        )
        .is_none());
        // 单点占位（不可达）同样拒收。
        pending.preview = vec![pending.start];
        assert!(confirm_pending(
            &pending,
            LineKind::River,
            LinePathMode::Route,
            0.0,
            "x",
            &map_id(),
            None,
            8.0,
            Id::new_unchecked("river_x")
        )
        .is_none());
    }

    // ── 计划 Task 11 Step 1 · 测试 2 ────────────────────────────────

    /// 预览只在终点格变化时重算：同格内挪动零重算（防每帧 A* 的计数器断言）。
    #[test]
    fn preview_recomputes_only_on_end_cell_change() {
        let grid = flat_grid();
        let cell = 8.0_f32;
        let mut pending = PendingLinePoi {
            start: Vec2::new(40.0, 40.0),
            preview: Vec::new(),
            end_cell: cell_of(Vec2::new(40.0, 40.0), cell),
        };
        let mut recomputes = 0_u32;
        let feed = |pending: &mut PendingLinePoi, end: Vec2| {
            ensure_preview(pending, end, &grid, cell, LinePathMode::Route, 7)
        };
        // 首次：必然重算（preview 空）。
        recomputes += feed(&mut pending, Vec2::new(100.0, 100.0)) as u32;
        assert_eq!(recomputes, 1);
        // 同格内三次挪动（格 (12,12)：wu ∈ [96,104)² 内任意点）：零重算。
        recomputes += feed(&mut pending, Vec2::new(101.0, 100.0)) as u32;
        recomputes += feed(&mut pending, Vec2::new(103.5, 102.2)) as u32;
        recomputes += feed(&mut pending, Vec2::new(100.9, 103.9)) as u32;
        assert_eq!(
            recomputes, 1,
            "同格内挪动不得重算，实际重算 {recomputes} 次"
        );
        // 跨一格 → 恰好再重算一次。
        recomputes += feed(&mut pending, Vec2::new(120.0, 100.0)) as u32;
        assert_eq!(recomputes, 2, "跨格应恰好重算一次，实际 {recomputes}");

        // 模式切换路径：面板把 preview 清空 → 下一次 ensure 重算一次。
        pending.preview.clear();
        recomputes += feed(&mut pending, Vec2::new(120.0, 100.0)) as u32;
        assert_eq!(recomputes, 3, "preview 被作废后应重算恰好一次");
    }

    // ── 计划 Task 11 Step 1 · 测试 5 ────────────────────────────────

    /// 取消清空 pending（拍复位由交互系统的自愈逻辑承担——选中不动，
    /// 归下一次 Esc）。
    #[test]
    fn cancel_clears_pending() {
        let (pending, _session) = pinned_session();
        let mut pending_opt = Some(pending);
        assert!(pending_opt.is_some(), "夹具应有在画的线");
        cancel_pending(&mut pending_opt);
        assert!(pending_opt.is_none(), "取消后 pending 必须清空");
    }

    // ── 评审修复 R1/R2（fix round 1）───────────────────────────────

    /// 变图即弃 = pending 与 selected 一并弃（R1：旧口径只清 pending，
    /// 旧图选中残留 → 幽灵高亮 + 错比例尺写回 line_width_m）。
    #[test]
    fn map_change_reset_clears_pending_and_selection() {
        let a = Id::new_unchecked("map_a");
        let b = Id::new_unchecked("map_b");
        let bound = PinSession {
            phase: PinPhase::Pinned,
            map: Some(a.clone()),
        };
        let r = reset_on_map_change(&bound, Some(&a));
        assert!(
            !r.rebind && !r.clear_pending && !r.clear_selected,
            "同图零动作，实际 {r:?}"
        );
        let r = reset_on_map_change(&bound, Some(&b));
        assert!(
            r.rebind && r.clear_pending && r.clear_selected,
            "跨图三清，实际 {r:?}"
        );
        // 绑定丢失（drill 短暂返回 None 的会话）视为换图。
        assert!(reset_on_map_change(&bound, None).clear_selected);
        // 未绑过图的全新会话：要绑到当前图（否则钉点没有目标图）。
        assert!(reset_on_map_change(&PinSession::default(), Some(&a)).rebind);
    }

    /// 起钉清态（R2）：selected 清、名称草稿清（不静默继承选中线名），
    /// 拍进 Following；宽度**不在本函数职责内**（调用方保持不动 = 保留
    /// 上次用值）。
    #[test]
    fn begin_pin_clears_selection_and_name_draft() {
        let mut pending = None;
        let mut selected = Some(Id::new_unchecked("river_1"));
        let mut name = "青龙河".to_owned();
        let mut session = PinSession {
            phase: PinPhase::Idle,
            map: Some(map_id()),
        };
        begin_pin(
            &mut pending,
            &mut selected,
            &mut name,
            &mut session,
            Vec2::new(40.0, 40.0),
            (5, 5),
        );
        assert!(selected.is_none(), "起钉必须弃选中（面板切回新建态）");
        assert!(name.is_empty(), "名称草稿必须清空，实际 {name:?}");
        assert_eq!(
            pending.as_ref().map(|p| (p.start, p.end_cell)),
            Some((Vec2::new(40.0, 40.0), (5, 5))),
            "pending 出壳记起点与格"
        );
        assert_eq!(session.phase, PinPhase::Following);
    }

    // ── 状态机补充 ──────────────────────────────────────────────────

    /// Idle + 命中既有线 → Select（优先于起钉）；点空白 → Begin。
    #[test]
    fn idle_click_selects_existing_line_over_beginning() {
        let session = PinSession::default();
        let hit = Some(Id::new_unchecked("river_1"));
        assert_eq!(
            pin_on_click(&session, Vec2::new(10.0, 10.0), hit),
            PinClick::Select {
                id: Id::new_unchecked("river_1")
            },
            "命中既有线优先于起钉"
        );
        assert_eq!(
            pin_on_click(&session, Vec2::new(10.0, 10.0), None),
            PinClick::Begin {
                start: Vec2::new(10.0, 10.0)
            },
            "点空白 = 起钉"
        );
    }

    /// Pinned 拍点击不动作（等确认 / 取消）；Following 拍点击 = 定终点。
    #[test]
    fn click_outcomes_follow_the_phase() {
        let pinned = PinSession {
            phase: PinPhase::Pinned,
            map: None,
        };
        assert_eq!(pin_on_click(&pinned, Vec2::ZERO, None), PinClick::Ignore);
        let following = PinSession {
            phase: PinPhase::Following,
            map: None,
        };
        assert_eq!(
            pin_on_click(
                &following,
                Vec2::new(3.0, 4.0),
                Some(Id::new_unchecked("river_1"))
            ),
            PinClick::PinEnd {
                end: Vec2::new(3.0, 4.0)
            },
            "Following 拍点击一律定终点（忽略命中）"
        );
        assert_eq!(
            pin_on_move(&following, (1, 1), (2, 1)),
            Some((2, 1)),
            "Following 跨格 = 新终点格"
        );
        assert_eq!(pin_on_move(&following, (1, 1), (1, 1)), None, "同格不重算");
        assert_eq!(
            pin_on_move(&pinned, (1, 1), (2, 1)),
            None,
            "Pinned 不跟随光标"
        );
    }

    // ── seed 与 ID ──────────────────────────────────────────────────

    /// 种子确定性：同输入同种子；图 ID / 起点格任一变化种子即变。
    #[test]
    fn preview_seed_is_deterministic_per_start_and_map() {
        let a = preview_seed("map_a", (3, 4));
        assert_eq!(
            a,
            preview_seed("map_a", (3, 4)),
            "同一次钉点重预览必须同路径"
        );
        assert_ne!(a, preview_seed("map_b", (3, 4)), "换图种子应变");
        assert_ne!(a, preview_seed("map_a", (4, 4)), "换起点格种子应变");
    }

    /// 新 ID 确定性派生、撞名加长重试、字符集合规。
    #[test]
    fn new_line_poi_id_is_deterministic_and_collision_free() {
        let mut world = wv_core::World::default();
        let mk = |id: &str| Place {
            id: Id::new_unchecked(id),
            name: String::new(),
            feature: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: None,
            kind: None,
            line_kind: None,
            line_width_m: None,
        };
        let a = new_line_poi_id(
            &world,
            LineKind::River,
            "map_a",
            Vec2::new(8.0, 8.0),
            Vec2::new(80.0, 8.0),
        );
        let b = new_line_poi_id(
            &world,
            LineKind::River,
            "map_a",
            Vec2::new(8.0, 8.0),
            Vec2::new(80.0, 8.0),
        );
        assert_eq!(a, b, "同输入应派生同 ID");
        world.distances.places.push(mk(a.as_str()));
        let c = new_line_poi_id(
            &world,
            LineKind::River,
            "map_a",
            Vec2::new(8.0, 8.0),
            Vec2::new(80.0, 8.0),
        );
        assert_ne!(a, c, "撞名应加长重试出新 ID");
        for id in [a.as_str(), c.as_str()] {
            assert!(
                id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
                "ID `{id}` 必须满足 [A-Za-z0-9_]"
            );
        }
    }

    // ── 命中与宽度 ──────────────────────────────────────────────────

    /// 命中：阈值内取最近的一条线段 POI；阈值外 / 非线段地点不命中。
    #[test]
    fn line_poi_hit_finds_nearest_line_within_tolerance() {
        let mut world = wv_core::World::default();
        let mk_line = |id: &str, y: f32| Place {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            feature: None,
            map: Some(map_id()),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(vec![0.0, y, 200.0, y]),
            kind: None,
            line_kind: None, // 河（缺省语义）
            line_width_m: None,
        };
        world.distances.places.push(mk_line("river_near", 40.0));
        world.distances.places.push(mk_line("river_far", 60.0));
        // 点状地点（无 line）不得命中。
        let mut dot = mk_line("dot", 40.0);
        dot.line = None;
        world.distances.places.push(dot);

        // (100, 46)：距近线 6 ≤ 8、距远线 14 → 命中近线。
        assert_eq!(
            line_poi_hit(&world, &map_id(), Vec2::new(100.0, 46.0), 8.0),
            Some(Id::new_unchecked("river_near")),
            "阈值内应取最近的一条"
        );
        // (100, 55)：距两线 5 / 5，平手取先入表者（稳定序）。
        assert!(line_poi_hit(&world, &map_id(), Vec2::new(100.0, 55.0), 8.0).is_some());
        // (100, 80)：两线都 > 8 → 不命中。
        assert_eq!(
            line_poi_hit(&world, &map_id(), Vec2::new(100.0, 80.0), 8.0),
            None
        );
        // 折线端点外延：垂足落在线段外也应按端点距离判。
        assert_eq!(
            line_poi_hit(&world, &map_id(), Vec2::new(205.0, 40.0), 8.0),
            Some(Id::new_unchecked("river_near")),
            "端点外按最近端点判距离"
        );
    }

    /// 宽度换算方向与 T8 一致：`m ÷ 1000 ÷ km_per_wu`；无比例尺按 wu 直读；
    /// 总宽格数下限 1。
    #[test]
    fn width_conversions_match_render_side_direction() {
        assert!(
            (width_m_to_wu(1000.0, Some(0.5)) - 2.0).abs() < 1e-4,
            "1000 m ÷ 0.5 km/wu = 2 wu，实际 {}",
            width_m_to_wu(1000.0, Some(0.5))
        );
        assert!(
            (width_m_to_wu(60.0, None) - 60.0).abs() < 1e-4,
            "无比例尺按 wu 直读"
        );
        assert!(
            (width_wu_to_m(2.0, Some(0.5)) - 1000.0).abs() < 1e-2,
            "逆换算 2 wu = 1000 m"
        );
        assert_eq!(width_cells_of(60.0, 8.0), 8, "7.5 格 round = 8");
        assert_eq!(width_cells_of(0.0, 8.0), 1, "总宽下限 1 格（单格链）");
        assert_eq!(width_cells_of(4.0, 8.0), 1, "半格 round 到 0 → 钳回 1");
    }

    /// 格尺寸推导镜像 `wv-edit::marker_cell_size`：extent ÷ 栅格宽。
    #[test]
    fn grid_cell_size_derives_from_extent_over_raster_width() {
        let mut world = wv_core::World::default();
        let terrain = wv_core::terrain::TerrainData {
            width: 128,
            height: 128,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 0,
                    surface: 1,
                    biome: 0,
                };
                128 * 128
            ],
        };
        world.set_terrain(map_id(), terrain);
        let mut node = wv_core::map::MapNode {
            id: map_id(),
            name: "甲".into(),
            level: wv_core::map::MapLevel::Region,
            parent: None,
            children: vec![],
            layers: vec![],
            icons: vec![],
            extent: Some(wv_core::map::Extent {
                width: 1024.0,
                height: 1024.0,
                origin: None,
            }),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        };
        world.maps.push(node.clone());
        node.extent = None;
        let _ = node;
        let cell = grid_cell_size(&world, &map_id()).expect("有地形应推得出");
        assert!((cell - 8.0).abs() < 1e-4, "1024 / 128 = 8，实际 {cell}");
        // 无地形图 → None。
        assert!(grid_cell_size(&world, &Id::new_unchecked("nope")).is_none());
    }

    // ══ 计划 Task 12 · 端点拖拽（改道 / 延伸 / 汇流吸附）═══════════════

    use crate::state::{DraggedEnd, EndpointDrag};
    use crate::ui_shell::ShellNotice;

    /// 32×32 平地（高 100、全陆）+ 图节点（extent 256×256 → 格 8 wu）的最小世界。
    fn drag_world() -> wv_core::World {
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map_id(),
            name: "甲".into(),
            level: wv_core::map::MapLevel::Region,
            parent: None,
            children: vec![],
            layers: vec![],
            icons: vec![],
            extent: Some(wv_core::map::Extent {
                width: 256.0,
                height: 256.0,
                origin: None,
            }),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });
        let cells = vec![
            wv_core::terrain::TerrainCellData {
                height: 100,
                surface: 1,
                biome: 0,
            };
            32 * 32
        ];
        world.set_terrain(
            map_id(),
            wv_core::terrain::TerrainData {
                width: 32,
                height: 32,
                cells,
            },
        );
        world
    }

    /// 取图上地形格的快捷方式（列主序断言用）。
    fn cell_at(world: &wv_core::World, i: i32, j: i32) -> wv_core::terrain::TerrainCellData {
        let data = world.terrain_of(&map_id()).expect("夹具有地形");
        data.cells[(j as usize) * 32 + i as usize]
    }

    /// 写回图上地形格（夹具快照回放用）。
    fn snapshot_corridor(
        world: &wv_core::World,
        corridor: &[(i32, i32)],
    ) -> Vec<wv_core::terrain::TerrainCellData> {
        corridor
            .iter()
            .map(|&(i, j)| cell_at(world, i, j))
            .collect()
    }

    /// 端点命中：首/末顶点容差内才命中（中段不算），两端都中取更近者。
    #[test]
    fn endpoint_hit_picks_nearest_end_within_tolerance() {
        let pts = vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)];
        assert_eq!(
            endpoint_hit(&pts, Vec2::new(3.0, 2.0), 8.0),
            Some(DraggedEnd::First),
            "起点 3.6 wu 内应命中首端"
        );
        assert_eq!(
            endpoint_hit(&pts, Vec2::new(97.0, 1.0), 8.0),
            Some(DraggedEnd::Last),
            "终点 3.2 wu 内应命中末端"
        );
        assert_eq!(
            endpoint_hit(&pts, Vec2::new(50.0, 0.0), 8.0),
            None,
            "中段顶点不是端点，不得起拖"
        );
        assert_eq!(
            endpoint_hit(&pts, Vec2::new(11.0, 0.0), 8.0),
            None,
            "容差外不命中"
        );
    }

    /// 拖拽预览只在新目标格变化时重算（T11 end_cell 缓存门同语义，防每帧 A*）。
    #[test]
    fn drag_preview_recomputes_only_on_target_cell_change() {
        let grid = flat_grid();
        let cell = 8.0_f32;
        let original = vec![Vec2::new(40.0, 40.0), Vec2::new(40.0, 120.0)];
        let mut drag = begin_endpoint_drag(
            Id::new_unchecked("river_1"),
            DraggedEnd::Last,
            original,
            (5, 15),
        );
        let mut recomputes = 0_u32;
        let feed = |drag: &mut EndpointDrag, target: Vec2| {
            ensure_drag_preview(drag, target, &grid, cell, LinePathMode::Carve, 7)
        };
        // 首次挪到新格：必然重算。
        recomputes += feed(&mut drag, Vec2::new(200.0, 124.0)) as u32;
        assert_eq!(recomputes, 1);
        // 同格内挪动（格 (25,15)：wu ∈ [200,208)×[120,128)）：零重算。
        recomputes += feed(&mut drag, Vec2::new(201.0, 121.0)) as u32;
        recomputes += feed(&mut drag, Vec2::new(206.5, 127.2)) as u32;
        assert_eq!(
            recomputes, 1,
            "同格内拖动不得重算，实际重算 {recomputes} 次"
        );
        // 跨格 → 恰好再重算一次。
        recomputes += feed(&mut drag, Vec2::new(200.0, 140.0)) as u32;
        assert_eq!(recomputes, 2, "跨格应恰好重算一次，实际 {recomputes}");
    }

    /// 提交计划：整线 = 固定段 + 新段；下切只喂「分叉点起的新增段」
    /// （与旧段共享的前缀不得重复进 carve_corridor）。
    #[test]
    fn drag_commit_plan_feeds_only_the_forked_tail_to_carve() {
        let original = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(80.0, 0.0),
            Vec2::new(160.0, 0.0),
        ];
        // 拖末端到 (160, 80)：锚点 = (80,0)，预览 = 固定段 + 新路径。
        let mut drag = begin_endpoint_drag(
            Id::new_unchecked("river_1"),
            DraggedEnd::Last,
            original,
            (20, 0),
        );
        let grid = flat_grid();
        assert!(ensure_drag_preview(
            &mut drag,
            Vec2::new(160.0, 80.0),
            &grid,
            8.0,
            LinePathMode::Carve,
            7
        ));
        let plan = drag_commit_plan(&drag).expect("有新段应可出计划");
        // 整线保留固定段（前两折点原样）。
        assert_eq!(plan.new_line[0], (0.0, 0.0));
        assert_eq!(plan.new_line[1], (80.0, 0.0));
        // 新段从分叉点（锚点）起——不是从线头重来。
        assert_eq!(
            plan.new_segment[0],
            (80.0, 0.0),
            "新段必须从分叉点起，实际 {:?}",
            plan.new_segment[0]
        );
        assert_eq!(
            plan.new_segment.len() + 1,
            plan.new_line.len(),
            "新段 = 整线去掉分叉点之前的固定段"
        );

        // 拖回原位（预览 == 原线）：无新增段 → 不可提交（不发命令）。
        let mut drag = begin_endpoint_drag(
            Id::new_unchecked("river_1"),
            DraggedEnd::Last,
            vec![Vec2::new(0.0, 0.0), Vec2::new(80.0, 0.0)],
            (10, 0),
        );
        drag.preview = vec![Vec2::new(0.0, 0.0), Vec2::new(80.0, 0.0)];
        assert!(
            drag_commit_plan(&drag).is_none(),
            "拖回原位不得产出提交计划"
        );
    }

    /// 首端原地（评审 C1）：点了一下没拖 / 拖回原位，预览与原线逐位一致
    /// ——旧公式 `new.len() - 1 - i` 在首端分支循环走满后 usize 下溢
    /// （debug panic / release 巨下标越界切片），普通操作必崩。
    #[test]
    fn drag_commit_plan_first_end_in_place_yields_full_line_no_carve() {
        let original = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(80.0, 0.0),
            Vec2::new(160.0, 0.0),
        ];
        let drag = begin_endpoint_drag(
            Id::new_unchecked("river_1"),
            DraggedEnd::First,
            original,
            (0, 0),
        );
        // 预览 == 原线（begin 即此态）：无新增段 → 不产命令 / 空 carve 段。
        assert!(
            drag_commit_plan(&drag).is_none(),
            "首端原地不得产出提交计划"
        );
    }

    /// 首端改道（评审 I2）：新段必须**含锚点段**——旧公式比锚点少 1，
    /// `new_segment` 漏掉「锚点 → 第一个新顶点」段，Carve 模式下第一段
    /// 不下切。
    #[test]
    fn first_end_reroute_carve_covers_anchor_segment() {
        let original = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(80.0, 0.0),
            Vec2::new(160.0, 0.0),
        ];
        let mut drag = begin_endpoint_drag(
            Id::new_unchecked("river_1"),
            DraggedEnd::First,
            original,
            (0, 0),
        );
        let grid = flat_grid();
        // 首端拖到 (0, 80)：锚点 = original[1] = (80,0)，从锚点向新端规划。
        assert!(ensure_drag_preview(
            &mut drag,
            Vec2::new(0.0, 80.0),
            &grid,
            8.0,
            LinePathMode::Carve,
            7
        ));
        let plan = drag_commit_plan(&drag).expect("首端改道应可出计划");
        // 固定端（末折点）原样保留在整线尾部。
        assert_eq!(
            *plan.new_line.last().expect("整线非空"),
            (160.0, 0.0),
            "固定端必须原样"
        );
        // 新段末端 = 锚点（与 Last 端「新段起点 = 锚点」对称）：
        // Carve 模式下锚点 → 第一个新顶点这段必须在切列表里。
        assert_eq!(
            *plan.new_segment.last().expect("新段非空"),
            (80.0, 0.0),
            "新段必须含锚点段，实得尾点 {:?}",
            plan.new_segment.last()
        );
        assert!(
            plan.new_segment.len() >= 2,
            "锚点 → 新端至少一段，实得 {} 点",
            plan.new_segment.len()
        );
    }

    /// 同格吸附（评审 I3）：吸附点出现在「上次重算」的同格内——格门不
    /// 触发，但松手提交必须落在吸附点（看到环、松手不吸 = 假吸附）。
    #[test]
    fn snap_target_in_same_cell_still_lands_on_snapped_point() {
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut projection = WorldProjection {
            world: drag_world(),
            ..Default::default()
        };
        // 目标河：水平短线 y=2；被拖河：二点线（创建态，无下切）。
        let mk = |id: &str, line: Vec<f32>| Place {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            feature: None,
            map: Some(map_id()),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(line),
            kind: None,
            line_kind: None, // 河（缺省语义）
            line_width_m: None,
        };
        projection
            .world
            .distances
            .places
            .push(mk("river_target", vec![140.0, 2.0, 160.0, 2.0]));
        projection
            .world
            .distances
            .places
            .push(mk("river_drag", vec![40.0, 40.0, 120.0, 40.0]));

        // 拖末端到 (150, 5)：规划一次（目标格 (18,0)）。
        let original = vec![Vec2::new(40.0, 40.0), Vec2::new(120.0, 40.0)];
        let mut drag = begin_endpoint_drag(
            Id::new_unchecked("river_drag"),
            DraggedEnd::Last,
            original,
            (15, 5),
        );
        let grid = terrain_grid_of(&projection.world, &map_id()).expect("夹具有地形");
        assert!(
            ensure_drag_preview(
                &mut drag,
                Vec2::new(150.0, 5.0),
                &grid,
                8.0,
                LinePathMode::Route,
                7
            ),
            "平地应可规划"
        );
        // 同格内吸附出现：目标线最近点 (150,2) 与上次目标 (150,5) 同格。
        let (sid, sp) = river_snap(
            &projection.world,
            &map_id(),
            &Id::new_unchecked("river_drag"),
            Vec2::new(150.0, 5.0),
            8.0,
        )
        .expect("3 wu 内应吸附");
        assert_eq!(sid, Id::new_unchecked("river_target"));
        assert!(
            sp.distance(Vec2::new(150.0, 2.0)) < 1e-3,
            "夹具前提：吸附点 = (150,2)，实际 ({},{})",
            sp.x,
            sp.y
        );
        assert_eq!(
            cell_of(sp, 8.0),
            cell_of(Vec2::new(150.0, 5.0), 8.0),
            "夹具前提：吸附点与上次重算目标同格（格门不触发）"
        );
        drag.snap = Some((sid, sp));

        // 松手提交（绕行模式，不下切）：折线末点必须落在吸附点。
        let mut notice = ShellNotice::default();
        assert!(
            commit_endpoint_drag(
                &mut drag,
                LinePathMode::Route,
                &map_id(),
                &mut edit,
                &mut projection,
                &mut notice,
                1
            ),
            "提交应成功，notice = {:?}",
            notice.text
        );
        let p = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == "river_drag")
            .expect("提交不得删地点");
        let line = p.line.as_ref().expect("提交后仍应有折线");
        let (lx, ly) = (line[line.len() - 2], line[line.len() - 1]);
        assert!(
            ((lx - 150.0).powi(2) + (ly - 2.0).powi(2)).sqrt() < 1e-3,
            "末点应 = 吸附点 (150,2)，实际 ({lx},{ly})"
        );
    }

    /// 汇流吸附（计划 Task 12）：拖端点至另一条河的折线容差内 → 端点落
    /// 该线最近点（误差 < 1e-3）；容差外 / 自己 / 道路线都不吸附。
    #[test]
    fn endpoint_snaps_to_other_river_line() {
        let mut world = wv_core::World::default();
        let mk = |id: &str, line: Vec<f32>, road: bool| Place {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            feature: None,
            map: Some(map_id()),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(line),
            kind: None,
            line_kind: road.then_some(LineKind::Road),
            line_width_m: None,
        };
        // 目标河：水平线 y=0；被拖的河：斜线（远离目标）。
        world
            .distances
            .places
            .push(mk("river_target", vec![0.0, 0.0, 100.0, 0.0], false));
        world
            .distances
            .places
            .push(mk("river_mover", vec![50.0, 50.0, 60.0, 60.0], false));

        let mover = Id::new_unchecked("river_mover");
        // (50, 5)：距目标线 5 ≤ 8 → 吸附到最近点 (50, 0)。
        let (id, pt) = river_snap(&world, &map_id(), &mover, Vec2::new(50.0, 5.0), 8.0)
            .expect("8 wu 内应吸附");
        assert_eq!(id, Id::new_unchecked("river_target"));
        assert!(
            pt.distance(Vec2::new(50.0, 0.0)) < 1e-3,
            "端点应落目标线最近点，实际 ({}, {})",
            pt.x,
            pt.y
        );
        // (50, 11)：距目标线 11 > 8 → 不吸附。
        assert!(river_snap(&world, &map_id(), &mover, Vec2::new(50.0, 11.0), 8.0).is_none());
        // 排除自己：把目标线换成 mover 自己的线附近 → 不吸附。
        assert!(river_snap(&world, &map_id(), &mover, Vec2::new(55.0, 55.0), 8.0).is_none());
        // 道路线不参与（v1 只做河-河汇流）。
        world
            .distances
            .places
            .push(mk("road_target", vec![0.0, 20.0, 100.0, 20.0], true));
        assert!(
            river_snap(&world, &map_id(), &mover, Vec2::new(50.0, 25.0), 8.0).is_none(),
            "路线不是汇流吸附目标"
        );
    }

    /// 变图即弃扩展（T12）：端点拖拽会话与 pending / selected 同口径同弃。
    #[test]
    fn map_change_reset_clears_drag_session() {
        let a = Id::new_unchecked("map_a");
        let b = Id::new_unchecked("map_b");
        let bound = PinSession {
            phase: PinPhase::Idle,
            map: Some(a.clone()),
        };
        assert!(
            !reset_on_map_change(&bound, Some(&a)).clear_drag,
            "同图不得清拖拽"
        );
        assert!(
            reset_on_map_change(&bound, Some(&b)).clear_drag,
            "跨图必须清拖拽（旧图预览画进新图 = 幽灵线）"
        );
    }

    /// 改道提交（计划 Task 12 点名）：旧走廊格逐位不变（一次性原则——旧段
    /// 下切不重算），仅分叉点起的新增段下切；撤销一次 = 整线 + 新增段全回。
    #[test]
    fn rerouting_keeps_previous_carve() {
        let cell = 8.0_f32;
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut projection = WorldProjection {
            world: drag_world(),
            ..Default::default()
        };

        // 夹具：竖直三折点河 (40,40)→(40,120)→(40,200)，创建即下切
        // （宽 1 格、写水面）——「创建于下切模式」的既有状态。
        let line: Vec<Vec2> = vec![
            Vec2::new(40.0, 40.0),
            Vec2::new(40.0, 120.0),
            Vec2::new(40.0, 200.0),
        ];
        let place = Place {
            id: Id::new_unchecked("river_drag"),
            name: "测试河".into(),
            feature: None,
            map: Some(map_id()),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(line.iter().flat_map(|p| [p.x, p.y]).collect()),
            kind: None,
            line_kind: None, // 河（缺省语义）
            line_width_m: None,
        };
        let carve = wv_edit::command::CarveSpec {
            pts: vec![(40.0, 40.0), (40.0, 120.0), (40.0, 200.0)],
            strength: 1.0,
            width_cells: 1,
            mark_water: true,
        };
        let mut tx = wv_edit::history::Transaction::new();
        tx.push(Box::new(wv_edit::command::CreateLinePoiCommand::new(
            map_id(),
            place,
            Some(carve),
            cell,
        )));
        edit.history
            .commit(&mut projection.world, tx)
            .expect("夹具创建应成功");

        // 旧走廊快照（竖列 i=5，j=5..=25）。
        let corridor: Vec<(i32, i32)> = (5..=25).map(|j| (5, j)).collect();
        let before = snapshot_corridor(&projection.world, &corridor);

        // 拖末端到 (200, 124)：锚点 = (40,120)，新段向东近直线（下切规划无噪声）。
        let mut drag = begin_endpoint_drag(
            Id::new_unchecked("river_drag"),
            DraggedEnd::Last,
            line.clone(),
            (5, 25),
        );
        let grid = terrain_grid_of(&projection.world, &map_id()).expect("夹具有地形");
        assert!(
            ensure_drag_preview(
                &mut drag,
                Vec2::new(200.0, 124.0),
                &grid,
                cell,
                LinePathMode::Carve,
                7
            ),
            "平地应可规划新段"
        );
        let mut notice = ShellNotice::default();
        assert!(
            commit_endpoint_drag(
                &mut drag,
                LinePathMode::Carve,
                &map_id(),
                &mut edit,
                &mut projection,
                &mut notice,
                1
            ),
            "改道提交应成功，notice = {:?}",
            notice.text
        );

        // ① 旧走廊（除分叉口 ±1 格）逐位不变。
        let after = snapshot_corridor(&projection.world, &corridor);
        for (k, &(i, j)) in corridor.iter().enumerate() {
            if (14..=16).contains(&j) {
                continue; // 新旧走廊交叠的分叉口邻域，不参与断言。
            }
            assert_eq!(after[k], before[k], "旧走廊格 ({i},{j}) 不得被改道触碰");
        }
        // ② 新增段（东向走廊，行 j=15）已下切成水。
        for i in [8_i32, 15, 22] {
            let c = cell_at(&projection.world, i, 15);
            assert_eq!(
                c.surface,
                wv_terrain::SURFACE_WATER,
                "新段格 ({i},15) 应为水面"
            );
            assert!(
                c.height < 100,
                "新段格 ({i},15) 应被下切，实际高度 {}",
                c.height
            );
        }
        // ③ 整线已重发：固定段保留，末点 = 拖拽落点。
        let p = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == "river_drag")
            .expect("改道不得删地点");
        let new_line = p.line.as_ref().expect("改道后仍应有折线");
        assert_eq!(
            &new_line[0..4],
            &[40.0, 40.0, 40.0, 120.0],
            "固定段必须原样保留，实得 {:?}",
            &new_line[0..4]
        );
        let (lx, ly) = (new_line[new_line.len() - 2], new_line[new_line.len() - 1]);
        assert!(
            (lx - 200.0).abs() < 1e-3 && (ly - 124.0).abs() < 1e-3,
            "末点应 = 拖拽落点 (200,124)，实际 ({lx},{ly})"
        );

        // ④ 一次撤销：整线 + 新增段下切全回。
        edit.history.undo(&mut projection.world).expect("应可撤销");
        let p = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == "river_drag")
            .expect("撤销不得删地点");
        assert_eq!(
            p.line.as_ref().expect("撤销后仍应有折线"),
            &line.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<f32>>(),
            "撤销后整线应回到原样"
        );
        let after_undo = snapshot_corridor(&projection.world, &corridor);
        for (k, &(i, j)) in corridor.iter().enumerate() {
            assert_eq!(after_undo[k], before[k], "撤销后旧走廊格 ({i},{j}) 应复原");
        }
        let c = cell_at(&projection.world, 15, 15);
        assert_ne!(
            c.surface,
            wv_terrain::SURFACE_WATER,
            "撤销后新段格应退出水面"
        );
    }
}
