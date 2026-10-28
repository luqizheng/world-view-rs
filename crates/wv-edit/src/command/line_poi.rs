//! 线段 POI 命令族（ADR-0070）：创建（含地形下切）与属性修改。
//!
//! ## 为什么「创建 + 下切」是**一条**命令
//!
//! 作者画一条河 / 一条路时，「线段 POI 入表」与「沿线下切地形」是**同一个
//! 意图**的两半——拆成两条命令，撤销一次只退一半，世界就停在「有河名、
//! 没河床」的半成品态。命令的原子性该跟作者的心智单位走，不跟实现步骤走。
//!
//! 职责分界与 [`IntentTerrainCommand`] 同款（基线 §2.2）：
//! **`wv-terrain` 不感知 Undo/Redo，`wv-edit` 不感知像素**——下切本身由
//! `wv_terrain::line_path::carve_corridor` 完成，本族只负责快照、回放与
//! 脏路径报告。

use super::*;
use wv_core::distance::Place;
use wv_core::map::distance::LineKind;
use wv_core::map::road_derive::effective_line_kind;
use wv_terrain::line_path::carve_corridor;
use wv_terrain::{BIOME_NONE, BIOME_RIVER, BIOME_ROAD, SURFACE_WATER};

/// 一次走廊下切的**参数包**（ADR-0070 D3）。
///
/// `width_cells` 是**总宽格数**（评审裁定 I4）：走廊 = 中轴 ±
/// `(width_cells − 1) / 2` 格；`width_cells = 1` 即单格链（= 既有涂格河）。
/// 传「宽度米 ÷ 格宽」的商进来时按总宽换算，按半径解释会宽出约 2 倍。
#[derive(Debug, Clone)]
pub struct CarveSpec {
    /// 下切走廊的中轴折线（图内局部 wu 坐标）。
    pub pts: Vec<(f32, f32)>,
    /// 下切强度（0.0..=1.0，透传 `carve_corridor` 的 `strength`）。
    pub strength: f32,
    /// 走廊**总宽**格数（见类型文档——不是半径）。
    pub width_cells: u32,
    /// `true` = 河（写水面、清群系、复刻 legacy 下切深度）；`false` = 路
    /// （削到岸高附近、不写水面、跳过海面格）。
    pub mark_water: bool,
}

/// 创建线段 POI：一条命令同时「地点入表 + 沿线地形下切」（ADR-0070）。
///
/// ## 前置校验（不动世界）
///
/// - `place.line` 不足两点（`len < 4`）→ [`EditError::Precondition`]——
///   点状地点请走 `AddPlaceCommand`，线段 POI 的最小形状就是两点折线；
/// - 地点 ID 已存在 → [`EditError::Precondition`]（与 `AddPlaceCommand`
///   同款：不静默覆盖）；
/// - 带 `carve` 但目标图无地形栅格 → [`EditError::TerrainMismatch`]
///   （不替作者凭空造尺寸）。
///
/// ## 回滚
///
/// `apply` 缓存「被下切格的旧值」与「地点表」两份状态；`revert` 先逐格
/// 回放地形，再删地点——两半都退，与 apply 的改动面严格对称。
#[derive(Debug)]
pub struct CreateLinePoiCommand {
    /// 目标地图 ID（下切作用在**这张图**的栅格上）。
    pub map: wv_core::Id,
    /// 要创建的线段地点（含 `line` / `line_kind` / `line_width_m`）。
    pub place: Place,
    /// `Some` = 沿线下切；`None` = 纯矢量创建（不碰地形，也不要求有栅格）。
    pub carve: Option<CarveSpec>,
    /// 每格世界边长（wu/格）。
    pub cell_size: f32,
    /// `apply` 时缓存的受下切格旧值：`(gx, gy, before_cell)`。
    ///
    /// 空 Vec = 未 apply 过或未下切；`revert` 里地点删除是「apply 过」的
    /// 权威判据（`AddPlaceCommand` 同款语义）。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl CreateLinePoiCommand {
    /// 从 `(map, place, carve, cell_size)` 一步构造。
    pub fn new(map: wv_core::Id, place: Place, carve: Option<CarveSpec>, cell_size: f32) -> Self {
        Self {
            map,
            place,
            carve,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for CreateLinePoiCommand {
    fn label(&self) -> String {
        format!("新增线段地点「{}」", self.place.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // ① 前置校验（先于一切写操作，保证失败 = 世界原样）。
        let line_len = self.place.line.as_ref().map_or(0, |l| l.len());
        if line_len < 4 {
            return Err(EditError::Precondition(
                "线段 POI 的折线至少需要 2 个点".into(),
            ));
        }
        if world.distances.places.iter().any(|p| p.id == self.place.id) {
            return Err(EditError::Precondition(format!(
                "地点 `{}` 已存在",
                self.place.id
            )));
        }

        // ② 下切（可选）：快照 → carve → diff 缓存 → 写回（IntentTerrainCommand 同款）。
        let mut carved = false;
        if let Some(carve) = &self.carve {
            let data = world.terrain_of(&self.map).cloned().ok_or_else(|| {
                EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map))
            })?;
            let mut grid = to_grid(&data)?;
            let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
            carve_corridor(
                &mut grid,
                &carve.pts,
                carve.strength,
                self.cell_size,
                carve.width_cells,
                carve.mark_water,
            );
            let mut before = self
                .before
                .lock()
                .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
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
            world.set_terrain(self.map.clone(), from_grid(grid));
            carved = true;
        }

        // ③ 地点入表（AddPlace 语义）。
        world.distances.places.push(self.place.clone());

        let mut dirty = vec!["distances/places".into()];
        if carved {
            dirty.push(format!("maps/{}/terrain.bin", self.map));
        }
        Ok(dirty)
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        // ① 地形回放（快照 diff 逐格写回）。
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone();
        if !before.is_empty() {
            let data = world.terrain_of(&self.map).cloned().ok_or_else(|| {
                EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map))
            })?;
            let mut grid = to_grid(&data)?;
            for (gx, gy, cell) in &before {
                grid.set(*gx, *gy, *cell);
            }
            world.set_terrain(self.map.clone(), from_grid(grid));
        }
        // ② 删地点（AddPlaceCommand::revert 同款：不在表里 = TargetMissing，
        //    顺带守住了「未 apply 先 revert」）。
        let pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place.id)))?;
        world.distances.places.remove(pos);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 存量迁移（ADR-0070 §4.6 / 计划 Task 7）
// ---------------------------------------------------------------------------

/// 存量迁移的**诊断计数**（ADR-0070 §4.6「有诊断」；V1 只提示不阻止）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    /// 涂格转换数：biome 16 → 水面 + 无群系、biome 20 → 无群系（height 均不动）。
    pub converted_cells: usize,
    /// 链式合并成功落地的道路线 POI 条数。
    pub chains: usize,
    /// 未能成线的链数（折线退化 / 端点无锚点 / 自环 road 边 / 撞名完全
    /// 重放）；其边仍被消费——见命令文档。
    pub chains_skipped: usize,
    /// 模板派生河转出的河线 POI 条数。
    pub rivers: usize,
    /// 删除的手写 road 边数。
    pub deleted_edges: usize,
}

/// 判定一个世界是否还带着**存量线数据**（ADR-0070 §4.6；加载期迁移的开关）。
///
/// 三条判据任一成立即需迁移：
///
/// 1. 任一图的格还涂着旧河 / 旧路群系（16 / 20）；
/// 2. 还存在**手写** road 边（`terrain == "road"` 且 `!derived`）——排除推导边
///    是硬性的：线即路网接线后运行时边表里常驻 derived 投影边，不排除会让
///    每次加载都重迁移（实体增殖）；
/// 3. 有模板生成记录（`meta.template.rivers_seed` 在场）而全工程还没有河线
///    POI——模板河尚未转实体。
///
/// 收敛性：[`MigrateLegacyLineDataCommand::apply`] 跑完一轮后三条判据应全部
/// 转假（格转完、手写 road 边删完、河实体在场），正常世界至多迁移一次。
pub fn needs_line_migration(world: &World) -> bool {
    for data in world.terrains.values() {
        if data
            .cells
            .iter()
            .any(|c| c.biome == BIOME_RIVER || c.biome == BIOME_ROAD)
        {
            return true;
        }
    }
    if world
        .distances
        .edges
        .iter()
        .any(|e| e.terrain == "road" && !e.derived)
    {
        return true;
    }
    if world.meta.as_ref().and_then(|m| m.template).is_some() {
        let has_river = world
            .distances
            .places
            .iter()
            .any(|p| effective_line_kind(p) == Some(LineKind::River));
        if !has_river {
            return true;
        }
    }
    false
}

/// 存量线数据一次性迁移命令（ADR-0070 §4.6）：涂格转换 + 道路边链式合并 +
/// 模板派生河转实体，一条命令一次撤销全回。
///
/// ## apply 的五件事
///
/// ① **涂格转换**：全部图中 biome 16 → `SURFACE_WATER + BIOME_NONE`、
///    biome 20 → `BIOME_NONE`（height 不动）——群系 16/20 自此退役
///    （ADR-0070 D1），水面观感保留；
/// ② **道路边链式合并**：只吃**手写** road 边（`terrain == "road"` 且
///    `!derived`）——derived 是运行时投影，不排除会把每次加载都变成重迁移；
///    从度数≠2 的节点出发穿过一切 2 度节点成链，≥3 度路口与端点为线端点；
///    每链落一个 `Place { id: "road_mig_<端点A>_<端点B>", line_kind: Road }`
///    （撞名时追加折线短哈希后缀，见「幂等」），`rank` = 链上原边最高
///    `effective_rank`；**2 度中间节点保留不删**（计划钉死：它们是锚定
///    节点，不是垃圾）；`a == b` 的**自环** road 边不成链（计入
///    chains_skipped，边仍删除）；
/// ③ 被合并的边从 `world.distances.edges` 删除（此后由推导边接管）；
/// ④ **模板派生河转实体**：`meta.template.rivers_seed` 在场 ∧ 全工程无河线
///    POI 时，取第一张「有 extent 且有地形」的图跑
///    `wv_terrain::climate::generate_rivers`，每条折线落一个
///    `Place { id: "river_tpl_<序号:02>", line_kind: River }`——无下切、
///    不写地形，此后加载不再派生（ADR-0070 §4.2 生成器统一）；
/// ⑤ 返回脏路径（变动过的 `maps/<map>/terrain.bin` + `distances/places` +
///    `distances/edges`）。
///
/// ## 为什么删边与建线必须在同一命令
///
/// 迁移在**加载时自动**跑（spec §4.6 评审 §2 收编），对重复导入是常设通路：
/// 若「建线」与「删边」分成两条命令，建线那半每次加载都会因「手写 road 边
/// 仍在」重跑一遍——同一条路反复落实体。同一命令内原子完成，`needs_line_migration`
/// 才能在一轮后转假。
///
/// ## 幂等（评审 §2 收编；I1 撞名裁定修订）
///
/// 链 / 河 Place 的 ID 确定性派生（端点 ID 字典序 / 模板序号 / 撞名后缀 =
/// 折线坐标哈希）。apply 遇**基名已存在**（场上已有同 ID Place，如菱形
/// 平行路的第二条链）→ 追加折线坐标 splitmix 短哈希后缀
/// `road_mig_<A>_<B>_<h4>` 落实体——撞名的两条链**都落地**，不丢数据；
/// **加后缀仍撞**（同端点同折线的完全重放）→ 跳过不重复插（upsert 不覆盖
/// 既有实体；边照删保收敛）。同一数据重导入重跑得到同批实体，不增殖。
///
/// ## 回滚
///
/// `apply` 缓存三份 before：被改格（分图）、被删边、真正新插入的 Place ID；
/// `revert` 全量回放——撤销后世界与迁移前逐格逐边一致。
#[derive(Debug)]
pub struct MigrateLegacyLineDataCommand {
    /// before 快照：被改格（分图）——`(map, gx, gy, old_cell)`。
    before_cells: std::sync::Mutex<Vec<(wv_core::Id, i32, i32, TerrainCellData)>>,
    /// before 快照：被删的手写 road 边，带**原下标**——revert 按下标升序
    /// 插回，undo 后边表顺序与迁移前逐位一致（评审 M1；含非 road 边的
    /// 相对位置）。
    before_edges: std::sync::Mutex<Vec<(usize, wv_core::map::distance::PlaceEdge)>>,
    /// before 快照：本次 apply **真正新插入**的 Place ID（revert 只删这些，
    /// upsert 跳过的不算）。
    added_places: std::sync::Mutex<Vec<wv_core::Id>>,
    /// 诊断计数（apply 后经 [`Self::report`] 读取；V1 只提示）。
    report: std::sync::Mutex<MigrationReport>,
}

impl Default for MigrateLegacyLineDataCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrateLegacyLineDataCommand {
    /// 构造一条空迁移命令（改什么由 apply 时的世界数据决定）。
    pub fn new() -> Self {
        Self {
            before_cells: std::sync::Mutex::new(Vec::new()),
            before_edges: std::sync::Mutex::new(Vec::new()),
            added_places: std::sync::Mutex::new(Vec::new()),
            report: std::sync::Mutex::new(MigrationReport::default()),
        }
    }

    /// 迁移后的诊断计数快照（V1 只提示：供加载 notice / 校验面板列计数）。
    pub fn report(&self) -> MigrationReport {
        self.report.lock().map(|r| *r).unwrap_or_default()
    }
}

/// 链式合并的邻接表：节点 →（入射边下标，对端节点）。
type RoadAdj<'a> = std::collections::HashMap<&'a wv_core::Id, Vec<(usize, &'a wv_core::Id)>>;

/// 从 `start` 沿 `first_edge` 行走成链：穿过一切 2 度节点，遇度数≠2 的节点
/// （≥3 度路口 / 1 度端点）或回到起点（纯环）即收线。
///
/// 返回（节点序列，走过的边下标序列）；纯环的节点序列首尾同为 `start`
/// （闭合折线）。行走顺序由邻接表构建序决定（= 边表下标序），确定性成立。
fn walk_chain(
    adj: &RoadAdj<'_>,
    start: &wv_core::Id,
    first_edge: usize,
) -> (Vec<wv_core::Id>, Vec<usize>) {
    let mut nodes = vec![start.clone()];
    let mut edges_used = vec![first_edge];
    // first_edge 从 start 出发的对端。
    let mut cur = match adj
        .get(start)
        .and_then(|v| v.iter().find(|(e, _)| *e == first_edge))
    {
        Some((_, n)) => (*n).clone(),
        None => return (nodes, edges_used),
    };
    let mut cur_edge = first_edge;
    loop {
        nodes.push(cur.clone());
        if &cur == start {
            break; // 纯环走满一圈：折线闭合。
        }
        let Some(incid) = adj.get(&cur) else { break };
        if incid.len() != 2 {
            break; // ≥3 度路口 / 1 度端点：线到此为止。
        }
        // 2 度节点直穿：走「不是来路」的那条边。
        let Some((next_edge, next_node)) = incid.iter().find(|(e, _)| *e != cur_edge) else {
            break; // 平行自环等退化输入，保底不死循环。
        };
        cur_edge = *next_edge;
        edges_used.push(cur_edge);
        cur = (*next_node).clone();
    }
    (nodes, edges_used)
}

/// 链折线坐标的确定性短哈希（16 bit，`{:04x}`）——链 ID 撞名后缀（评审 I1）。
///
/// 每个折点的 `(x, y)` 以 f32 位型喂 `wv_terrain::hash::cell_hash01`
/// （逐格哈希当混入函数，裁定指定），逐点折进单个 u64 累积器，末端
/// splitmix64 收尾取低 16 位。同折线必同后缀（可重放）；16 bit 空间下
/// 异折线极小概率同后缀，撞了也只是走「跳过不重插」，无数据损失。
fn chain_id_h4(pts: &[(f32, f32)]) -> String {
    let mut acc: u64 = 0x9E37_79B9_7F4A_7C15;
    for (i, &(x, y)) in pts.iter().enumerate() {
        let h = wv_terrain::hash::cell_hash01(x.to_bits() as i64, y.to_bits() as i64, i as u64);
        acc = (acc ^ (h.to_bits() as u64)).rotate_left(17);
    }
    // splitmix64 收尾（常量与 wv_terrain::climate 的格哈希同族）。
    let mut z = acc;
    z ^= z >> 30;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    format!("{:04x}", z & 0xFFFF)
}

impl EditCommand for MigrateLegacyLineDataCommand {
    fn label(&self) -> String {
        "存量线数据迁移（ADR-0070）".to_string()
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 每轮 apply 重置快照与计数（命令可被 undo→redo 重复驱动）。
        self.before_cells
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clear();
        self.before_edges
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clear();
        self.added_places
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clear();
        *self
            .report
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? =
            MigrationReport::default();
        let mut report = MigrationReport::default();
        let mut dirty: Vec<String> = Vec::new();

        // ── ① 涂格转换：16 → 水面 + 无群系，20 → 无群系（height 不动）──
        let map_ids: Vec<wv_core::Id> = world.terrains.keys().cloned().collect();
        for map_id in &map_ids {
            let Some(data) = world.terrains.get_mut(map_id) else {
                continue;
            };
            let width = data.width.max(1) as usize;
            let mut changed: Vec<(i32, i32, TerrainCellData)> = Vec::new();
            for (idx, cell) in data.cells.iter_mut().enumerate() {
                let converted = if cell.biome == BIOME_RIVER {
                    Some(TerrainCellData {
                        height: cell.height,
                        surface: SURFACE_WATER,
                        biome: BIOME_NONE,
                    })
                } else if cell.biome == BIOME_ROAD {
                    Some(TerrainCellData {
                        height: cell.height,
                        surface: cell.surface,
                        biome: BIOME_NONE,
                    })
                } else {
                    None
                };
                if let Some(nc) = converted {
                    if *cell != nc {
                        changed.push(((idx % width) as i32, (idx / width) as i32, *cell));
                        *cell = nc;
                    }
                }
            }
            if !changed.is_empty() {
                report.converted_cells += changed.len();
                self.before_cells
                    .lock()
                    .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
                    .extend(
                        changed
                            .into_iter()
                            .map(|(gx, gy, c)| (map_id.clone(), gx, gy, c)),
                    );
                dirty.push(format!("maps/{}/terrain.bin", map_id));
            }
        }

        // ── ② 链式合并：先在不可变借用上把链与 Place 候选算完，再动表 ──
        // 只吃手写 road 边（derived 是运行时投影，见类型文档）。
        // 自环（a == b）单独收走（评审 M3）：进邻接表会把端点度数虚增 2、
        // 走链也会原地打转出零长退化线——不成链，直接消费。
        let mut hand_road: Vec<usize> = Vec::new();
        let mut self_loops: Vec<usize> = Vec::new();
        for (i, e) in world.distances.edges.iter().enumerate() {
            if e.terrain == "road" && !e.derived {
                if e.a == e.b {
                    self_loops.push(i);
                } else {
                    hand_road.push(i);
                }
            }
        }
        let mut adj: RoadAdj<'_> = Default::default();
        for &i in &hand_road {
            let e = &world.distances.edges[i];
            adj.entry(&e.a).or_default().push((i, &e.b));
            adj.entry(&e.b).or_default().push((i, &e.a));
        }
        let degree = |n: &wv_core::Id| adj.get(n).map_or(0, Vec::len);

        // 从度数≠2 的节点出发成链（排序 = 遍历序确定性）。
        let mut starts: Vec<&wv_core::Id> =
            adj.keys().copied().filter(|n| degree(n) != 2).collect();
        starts.sort_unstable();
        let mut visited: std::collections::HashSet<usize> = Default::default();
        let mut chains: Vec<(Vec<wv_core::Id>, Vec<usize>)> = Vec::new();
        for s in &starts {
            let incident: Vec<(usize, &wv_core::Id)> = adj.get(*s).cloned().unwrap_or_default();
            for (e0, _) in incident {
                if visited.contains(&e0) {
                    continue;
                }
                let (nodes, edges) = walk_chain(&adj, s, e0);
                for e in &edges {
                    visited.insert(*e);
                }
                chains.push((nodes, edges));
            }
        }
        // 剩余未访问边 = 纯环（全节点度 2，没有度≠2 起点）。从环上字典序
        // 最小节点断开成链（闭合折线），环 ID 由最小节点确定（确定性）。
        // TODO(阶段X)：环线的图拓扑在推导边上退化为自环（derive 对同点
        // 折线跳过），距离图暂不还原环连接——待裁条目见 ADR-0070 §4.6。
        for &i in &hand_road {
            if visited.contains(&i) {
                continue;
            }
            let probe_start = world.distances.edges[i].a.clone();
            let probe_nodes = walk_chain(&adj, &probe_start, i).0;
            let Some(min_node) = probe_nodes.iter().min().cloned() else {
                continue;
            };
            let Some(e0) = adj.get(&min_node).and_then(|v| v.first()).map(|(e, _)| *e) else {
                continue;
            };
            let (nodes, edges) = walk_chain(&adj, &min_node, e0);
            for e in &edges {
                visited.insert(*e);
            }
            chains.push((nodes, edges));
        }

        // 链 → 线段 POI 候选（仍在不可变借用里算）。
        let mut consumed: Vec<usize> = self_loops.clone();
        report.chains_skipped += self_loops.len();
        let mut new_places: Vec<Place> = Vec::new();
        let mut added_ids: Vec<wv_core::Id> = Vec::new();
        // 占名判据含**本批已暂存**的候选：同一轮里撞名的第二条链（菱形
        // 平行路）必须看到第一条链的基名，否则两条会同 ID 双插（评审 I1）。
        let mut staged_ids: Vec<String> = Vec::new();
        for (nodes, edges) in &chains {
            consumed.extend(edges.iter().copied());
            // 折线 = 途经节点锚点连线（2 度节点即中间折点，effective_anchor）。
            let mut pts: Vec<(f32, f32)> = Vec::new();
            for n in nodes {
                if let Some(p) = world.distances.places.iter().find(|p| &p.id == n) {
                    if let Some(a) = p.effective_anchor() {
                        pts.push(a);
                    }
                }
            }
            if pts.len() < 2 {
                // 端点连锚点都凑不出（悬空 / 未锚定）：链画不成线。边仍消费
                // ——留着会让 needs_line_migration 永真，每次加载重迁移。
                report.chains_skipped += 1;
                continue;
            }
            let first = nodes[0].clone();
            let last = nodes.last().expect("链节点序列非空").clone();
            let (lo, hi) = if first <= last {
                (first, last)
            } else {
                (last, first)
            };
            let base_id = format!("road_mig_{}_{}", lo.as_str(), hi.as_str());
            // 撞名（评审 I1 裁定：数据完整优先）：基名已被占（场上实体或本批
            // 暂存候选）→ 追加折线短哈希后缀再落实体，两条链都落地不丢数据；
            // 加后缀仍撞（同端点同折线的完全重放）→ 跳过不重插——upsert 不
            // 覆盖既有实体，边照删保收敛。
            let id_str = if world
                .distances
                .places
                .iter()
                .any(|p| p.id.as_str() == base_id)
                || staged_ids.iter().any(|s| s == &base_id)
            {
                let h4 = chain_id_h4(&pts);
                format!("{}_{}", base_id, h4)
            } else {
                base_id
            };
            if world
                .distances
                .places
                .iter()
                .any(|p| p.id.as_str() == id_str)
                || staged_ids.iter().any(|s| s == &id_str)
            {
                report.chains_skipped += 1;
                continue;
            }
            staged_ids.push(id_str.clone());
            // 链的呈现级别 = 链上原边最高 effective_rank（与 PlaceEdge 同一张
            // 冻结表折算——迁移前后全国/区域档路网显隐零回归）。
            let rank = edges
                .iter()
                .filter_map(|&i| world.distances.edges.get(i))
                .map(|e| e.effective_rank())
                .max()
                .unwrap_or(1);
            // 链所在图 = 字典序小端点 Place 的图。跨图链的折线是两套局部坐标
            // 混排——几何不完美，但端点吸附与公里折算仍成立，拓扑与收敛优先。
            // TODO(阶段X)：跨图链的几何口径待裁（罕见输入，先不做裁剪）。
            let map = world
                .distances
                .places
                .iter()
                .find(|p| p.id == lo)
                .and_then(|p| p.map.clone());
            let name = if lo == hi {
                format!("迁移道路（环线）{}", lo.as_str())
            } else {
                format!("迁移道路 {}–{}", lo.as_str(), hi.as_str())
            };
            // M2：走受校验构造。端点均为受校验 Id、后缀是十六进制，字符集
            // 必合法；带原串的 panic 仅作不变式哨兵（不可达）。
            let id = wv_core::Id::new(id_str.clone()).unwrap_or_else(|e| {
                panic!("链 ID `{id_str}` 构造失败：{e:?}（端点为受校验 Id，理应不可能）")
            });
            added_ids.push(id.clone());
            new_places.push(Place {
                id,
                name,
                feature: None,
                map,
                // x/y 缺省 → 锚点由折线中点派生（anchor_from_line 现成）。
                x: None,
                y: None,
                w: None,
                h: None,
                biome: None,
                envelope: None,
                rank: Some(rank),
                line: Some(pts.iter().flat_map(|(x, y)| [*x, *y]).collect()),
                kind: None,
                line_kind: Some(LineKind::Road),
                line_width_m: None,
            });
            report.chains += 1;
        }

        // ── ③ 删除被合并的边（降序删，下标不漂移）+ 记 before（带原下标，
        //    revert 按下标升序插回——评审 M1）──
        consumed.sort_unstable();
        consumed.dedup();
        {
            let mut before_edges = self
                .before_edges
                .lock()
                .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
            for &i in consumed.iter().rev() {
                if let Some(e) = world.distances.edges.get(i) {
                    before_edges.push((i, e.clone()));
                }
            }
        }
        for &i in consumed.iter().rev() {
            if i < world.distances.edges.len() {
                world.distances.edges.remove(i);
            }
        }
        report.deleted_edges = consumed.len();

        // ②' 插入链 Place（upsert 已在候选阶段过滤）。
        for p in new_places {
            world.distances.places.push(p);
        }

        // ── ④ 模板派生河转实体（spec §4.6）──
        if let Some(tpl) = world.meta.as_ref().and_then(|m| m.template) {
            let has_river = world
                .distances
                .places
                .iter()
                .any(|p| effective_line_kind(p) == Some(LineKind::River));
            if !has_river {
                // 第一张「有 extent 且有地形」的图（maps 声明序）。
                let target = world.maps.iter().find_map(|m| {
                    let ext = m.extent.as_ref()?;
                    world.terrain_of(&m.id)?;
                    Some((m.id.clone(), ext.width, ext.height))
                });
                if let Some((map_id, ex_w, ex_h)) = target {
                    let rivers = {
                        let data = world.terrain_of(&map_id).ok_or_else(|| {
                            EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", map_id))
                        })?;
                        let heights: Vec<i16> = data.cells.iter().map(|c| c.height).collect();
                        // max_rivers = 12：与渲染侧先例同参（world_projection），
                        // 生成器统一（ADR-0070 §4.2）。
                        wv_terrain::climate::generate_rivers(
                            data.width,
                            data.height,
                            &heights,
                            tpl.rivers_seed,
                            12,
                        )
                    };
                    for (i, poly) in rivers.iter().enumerate() {
                        // 归一化 [0,1] → 图内局部 wu（渲染侧同款换算）。
                        let flat: Vec<f32> = poly
                            .iter()
                            .flat_map(|p| [p[0] * ex_w, p[1] * ex_h])
                            .collect();
                        let id_str = format!("river_tpl_{:02}", i);
                        // 幂等：已迁移过 → 不增殖。
                        if world
                            .distances
                            .places
                            .iter()
                            .any(|p| p.id.as_str() == id_str)
                        {
                            continue;
                        }
                        // M2：受校验构造，同链 ID 哨兵语义（`river_tpl_NN`
                        // 必合法，不可达分支）。
                        let id = wv_core::Id::new(id_str.clone())
                            .unwrap_or_else(|e| panic!("河 ID `{id_str}` 构造失败：{e:?}"));
                        added_ids.push(id.clone());
                        world.distances.places.push(Place {
                            id,
                            name: format!("模板河流 {:02}", i + 1),
                            feature: None,
                            map: Some(map_id.clone()),
                            // 锚点缺省由折线中点派生，不写 x/y。
                            x: None,
                            y: None,
                            w: None,
                            h: None,
                            biome: None,
                            envelope: None,
                            rank: None,
                            line: Some(flat),
                            kind: None,
                            line_kind: Some(LineKind::River),
                            line_width_m: None,
                        });
                        report.rivers += 1;
                    }
                }
                // 无合格图（全工程没有带地形的图）→ 跳过此步（计划钉死）。
            }
        }

        // ── ⑤ 脏路径 + 落账 ──
        if report.deleted_edges > 0 {
            dirty.push("distances/edges".into());
        }
        if !added_ids.is_empty() {
            dirty.push("distances/places".into());
        }
        *self
            .added_places
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = added_ids;
        *self
            .report
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = report;
        Ok(dirty)
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        // ① 回放被改格（按图分组直接写 TerrainData——apply 未走 grid 桥，
        //    回放同口径；apply 未跑过 → 快照空 → 无事发生）。
        let before_cells = self
            .before_cells
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone();
        if !before_cells.is_empty() {
            let mut by_map: std::collections::BTreeMap<
                wv_core::Id,
                Vec<(i32, i32, TerrainCellData)>,
            > = Default::default();
            for (map_id, gx, gy, cell) in before_cells {
                by_map.entry(map_id).or_default().push((gx, gy, cell));
            }
            for (map_id, cells) in by_map {
                let Some(data) = world.terrains.get_mut(&map_id) else {
                    continue;
                };
                let width = data.width.max(1) as usize;
                for (gx, gy, cell) in cells {
                    let idx = gy as usize * width + gx as usize;
                    if idx < data.cells.len() {
                        data.cells[idx] = cell;
                    }
                }
            }
        }
        // ② 恢复被删边（评审 M1）：先摘掉**现存**被删边（防重复 revert 时
        //    同 ID 双份），再按**原下标**升序插回——undo 后边表与迁移前逐位
        //    一致（含非 road 边的相对位置）。apply 未跑过 → 空 Vec → 原样。
        let mut before_edges = self
            .before_edges
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone();
        before_edges.sort_by_key(|(i, _)| *i);
        for (_, e) in &before_edges {
            if let Some(pos) = world.distances.edges.iter().position(|x| x.id == e.id) {
                world.distances.edges.remove(pos);
            }
        }
        for (i, e) in before_edges {
            let at = i.min(world.distances.edges.len());
            world.distances.edges.insert(at, e);
        }
        // ③ 删除新增 Place（只删 apply 真正插过的；upsert 跳过的不在此列）。
        let added = self
            .added_places
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone();
        if !added.is_empty() {
            world.distances.places.retain(|p| !added.contains(&p.id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use wv_core::map::road_derive::{derive_road_edges, ROAD_SNAP_TOLERANCE_WU};
    use wv_core::map::Extent;
    use wv_core::map::MapLevel;
    use wv_core::time::TimeSpan;
    use wv_terrain::{BIOME_NONE, BIOME_RIVER, BIOME_ROAD, SURFACE_LAND, SURFACE_WATER};

    const MIG_MAP: &str = "map_world";

    fn vid(s: &str) -> wv_core::Id {
        wv_core::Id::new_unchecked(s)
    }

    /// 主图：`unit_per_km = 1.0`（1 wu = 1 km，手算方便）、extent 256×256。
    fn mig_map() -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: vid(MIG_MAP),
            name: "主世界".into(),
            level: MapLevel::World,
            unit_per_km: Some(1.0),
            extent: Some(Extent::sized(256.0, 256.0)),
            ..wv_core::map::MapNode::default()
        }
    }

    fn mig_point(id: &str, x: f32, y: f32) -> Place {
        Place {
            id: vid(id),
            name: id.into(),
            feature: None,
            map: Some(vid(MIG_MAP)),
            x: Some(x),
            y: Some(y),
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: None,
            kind: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    fn road_edge(id: &str, a: &str, b: &str, km: f32) -> wv_core::map::distance::PlaceEdge {
        wv_core::map::distance::PlaceEdge {
            id: vid(id),
            a: vid(a),
            b: vid(b),
            distance_km: km,
            terrain: "road".into(),
            bidirectional: true,
            rank: None,
            derived: false,
        }
    }

    /// 4×4 平地图（群系 1、陆地、高 100）：涂格与路网夹具的公共底座。
    fn flat_world() -> World {
        let mut w = World::default();
        w.maps.push(mig_map());
        w.set_terrain(
            vid(MIG_MAP),
            wv_core::terrain::TerrainData {
                width: 4,
                height: 4,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height: 100,
                        surface: SURFACE_LAND,
                        biome: 1,
                    };
                    16
                ],
            },
        );
        w
    }

    /// 十字路网：中心 4 度 + 四端各 1 度，四条边各 100 km（= 100 wu）。
    fn cross_world() -> World {
        let mut w = flat_world();
        w.distances.places = vec![
            mig_point("cross_center", 128.0, 128.0),
            mig_point("north_end", 128.0, 28.0),
            mig_point("south_end", 128.0, 228.0),
            mig_point("east_end", 228.0, 128.0),
            mig_point("west_end", 28.0, 128.0),
        ];
        w.distances.edges = vec![
            road_edge("e_cn", "cross_center", "north_end", 100.0),
            road_edge("e_cs", "cross_center", "south_end", 100.0),
            road_edge("e_ce", "cross_center", "east_end", 100.0),
            road_edge("e_cw", "cross_center", "west_end", 100.0),
        ];
        w
    }

    fn migrate(w: &mut World) -> MigrationReport {
        let cmd = MigrateLegacyLineDataCommand::new();
        cmd.apply(w).expect("迁移应成功");
        cmd.report()
    }

    /// 手算 fixture：十字路网 5 节点 4 边 → 4 条线，端点恰为度≠2 的节点；
    /// 边删除后 derive_road_edges 恢复 4 条边，公里和 = 原 4 边之和 ±10%。
    #[test]
    fn chain_merge_preserves_topology() {
        let mut w = cross_world();
        assert!(needs_line_migration(&w), "手写 road 边在场应判定需迁移");
        let cmd = MigrateLegacyLineDataCommand::new();
        let dirty = cmd.apply(&mut w).expect("迁移应成功");
        let rep = cmd.report();
        assert_eq!(rep.chains, 4, "四条边各成一链，实得 {:?}", rep);
        assert_eq!(rep.chains_skipped, 0, "无跳过链");
        assert_eq!(rep.deleted_edges, 4, "全部手写边被消费");
        assert!(
            dirty.iter().any(|d| d == "distances/places"),
            "实得 {:?}",
            dirty
        );
        assert!(
            dirty.iter().any(|d| d == "distances/edges"),
            "实得 {:?}",
            dirty
        );
        assert!(
            !dirty.iter().any(|d| d.contains("terrain.bin")),
            "无格变动不应标脏地形，实得 {:?}",
            dirty
        );

        // 四条 road_mig_* 线：道路语义、2 折点、rank = 原边最高折算（100 km → 3）。
        let lines: Vec<&Place> = w
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with("road_mig_"))
            .collect();
        assert_eq!(
            lines.len(),
            4,
            "实得 {:?}",
            lines.iter().map(|p| p.id.as_str()).collect::<Vec<_>>()
        );
        for p in &lines {
            assert_eq!(p.line_kind, Some(LineKind::Road), "链应为道路线");
            assert_eq!(
                p.line.as_ref().map(|l| l.len()),
                Some(4),
                "十字网每链恰 2 折点，实得 {:?}",
                p.line
            );
            assert_eq!(p.rank, Some(3), "原边 100 km → effective_rank 3");
        }
        // 端点集合 = 中心 ×4 + 四端各恰一次（度≠2 节点全为线端点）。
        let center = (128.0_f32, 128.0_f32);
        let mut ends: Vec<(f32, f32)> = Vec::new();
        for p in &lines {
            let l = p.line.as_ref().expect("有折线");
            let (a, b) = ((l[0], l[1]), (l[2], l[3]));
            assert!(a == center || b == center, "每链一端应是中心，实得 {:?}", l);
            ends.push(if a == center { b } else { a });
        }
        for leaf in [(128.0, 28.0), (128.0, 228.0), (228.0, 128.0), (28.0, 128.0)] {
            assert_eq!(
                ends.iter().filter(|e| **e == leaf).count(),
                1,
                "端点 {leaf:?} 应恰出现一次，实得 {:?}",
                ends
            );
        }

        // 边删除后 derive 恢复 4 条边，公里和与原值一致（±10%）。
        assert!(
            w.distances.edges.is_empty(),
            "手写边应全部删除，实得 {}",
            w.distances.edges.len()
        );
        let derived = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(
            derived.edges.len(),
            4,
            "推导应恢复四条边，实得 {:?}",
            derived
        );
        let sum: f32 = derived.edges.iter().map(|e| e.distance_km).sum();
        assert!(
            (sum - 400.0).abs() <= 40.0,
            "恢复边公里和应 400 ±10%（360..440），实得 {}",
            sum
        );
        // 收敛：迁移后判据转假。
        assert!(!needs_line_migration(&w));
    }

    /// 2 度中间节点是锚定节点不是垃圾：合并后**保留不删**（计划钉死），
    /// 且链折线把它当作中间折点（三折点、全长 100 wu）。
    #[test]
    fn degree_two_junctions_survive_chain_merge() {
        let mut w = cross_world();
        // 在中心与北端之间插入 2 度中继（两段各 50 km）。
        w.distances.places.push(mig_point("mid_stop", 128.0, 78.0));
        let pos = w
            .distances
            .edges
            .iter()
            .position(|e| e.id.as_str() == "e_cn")
            .expect("e_cn 在场");
        w.distances.edges.remove(pos);
        w.distances
            .edges
            .push(road_edge("e_cm", "cross_center", "mid_stop", 50.0));
        w.distances
            .edges
            .push(road_edge("e_mn", "mid_stop", "north_end", 50.0));

        let rep = migrate(&mut w);
        assert_eq!(rep.chains, 4, "实得 {:?}", rep);
        assert_eq!(rep.deleted_edges, 5, "五条手写边全部消费");
        assert!(
            w.distances
                .places
                .iter()
                .any(|p| p.id.as_str() == "mid_stop"),
            "2 度中间节点不得删除"
        );
        let chain = w
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == "road_mig_cross_center_north_end")
            .expect("C–N 链应存在");
        let line = chain.line.as_ref().expect("有折线");
        assert_eq!(line.len(), 6, "跨中继链应有 3 折点，实得 {:?}", line);
        let pts: Vec<(f32, f32)> = line.chunks(2).map(|p| (p[0], p[1])).collect();
        let len: f32 = pts
            .windows(2)
            .map(|seg| ((seg[1].0 - seg[0].0).powi(2) + (seg[1].1 - seg[0].1).powi(2)).sqrt())
            .sum();
        assert!(
            (len - 100.0).abs() < 1e-3,
            "链折线长应 100 wu（50+50），实得 {}",
            len
        );
    }

    /// 涂格 16（旧河）→ `SURFACE_WATER + BIOME_NONE`，height 不动。
    #[test]
    fn painted_river_cells_become_surface_water() {
        let mut w = flat_world();
        let river_cells = [0_usize, 5, 9, 14];
        for (k, &idx) in river_cells.iter().enumerate() {
            let c = &mut w.terrains.get_mut(&vid(MIG_MAP)).expect("有地形").cells[idx];
            c.biome = BIOME_RIVER;
            if k == 0 {
                c.surface = SURFACE_WATER; // 原本就是水面的一格
            }
        }
        let heights_before: Vec<i16> = w.terrains[&vid(MIG_MAP)]
            .cells
            .iter()
            .map(|c| c.height)
            .collect();

        let cmd = MigrateLegacyLineDataCommand::new();
        let dirty = cmd.apply(&mut w).expect("迁移应成功");
        let rep = cmd.report();
        assert_eq!(rep.converted_cells, 4, "实得 {:?}", rep);
        let data = &w.terrains[&vid(MIG_MAP)];
        for &idx in &river_cells {
            let c = &data.cells[idx];
            assert_eq!(c.biome, BIOME_NONE, "格 {idx} 群系应清零");
            assert_eq!(
                c.surface, SURFACE_WATER,
                "格 {idx} 应转水面（保留水面观感）"
            );
        }
        let heights_after: Vec<i16> = data.cells.iter().map(|c| c.height).collect();
        assert_eq!(heights_before, heights_after, "height 不得被动");
        assert!(
            dirty.iter().any(|d| d == "maps/map_world/terrain.bin"),
            "实得 {:?}",
            dirty
        );
    }

    /// 涂格 20（旧路）→ `BIOME_NONE`，surface 与 height 都不动。
    #[test]
    fn painted_road_cells_become_biome_none() {
        let mut w = flat_world();
        let road_cells = [3_usize, 7];
        for &idx in &road_cells {
            let c = &mut w.terrains.get_mut(&vid(MIG_MAP)).expect("有地形").cells[idx];
            c.biome = BIOME_ROAD;
            c.surface = SURFACE_LAND;
        }
        let cmd = MigrateLegacyLineDataCommand::new();
        cmd.apply(&mut w).expect("迁移应成功");
        let rep = cmd.report();
        assert_eq!(rep.converted_cells, 2, "实得 {:?}", rep);
        let data = &w.terrains[&vid(MIG_MAP)];
        for &idx in &road_cells {
            let c = &data.cells[idx];
            assert_eq!(c.biome, BIOME_NONE, "格 {idx} 群系应清零");
            assert_eq!(c.surface, SURFACE_LAND, "路格 surface 不动（只清群系）");
            assert_eq!(c.height, 100, "height 不动");
        }
    }

    /// 撤销 = 全量回放：被改格逐格恢复、被删边恢复、新增 Place 删净。
    #[test]
    fn migration_undo_restores_original_cells_and_edges() {
        let mut w = cross_world();
        // 夹具增强（评审 M1）：road 边之间插一条非 road 边——undo 后它的
        // 相对位置也必须逐位复原，不能只看「同集合」。
        w.distances.edges.insert(
            2,
            wv_core::map::distance::PlaceEdge {
                id: vid("e_sea"),
                a: vid("north_end"),
                b: vid("south_end"),
                distance_km: 200.0,
                terrain: "sea".into(),
                bidirectional: true,
                rank: None,
                derived: false,
            },
        );
        w.terrains.get_mut(&vid(MIG_MAP)).expect("有地形").cells[2].biome = BIOME_RIVER;
        w.terrains.get_mut(&vid(MIG_MAP)).expect("有地形").cells[8].biome = BIOME_ROAD;
        let terrain_before = w.terrains[&vid(MIG_MAP)].clone();
        let edges_before = w.distances.edges.clone();
        let places_before = w.distances.places.len();

        let cmd = MigrateLegacyLineDataCommand::new();
        cmd.apply(&mut w).expect("迁移应成功");
        assert_ne!(
            w.terrains[&vid(MIG_MAP)],
            terrain_before,
            "apply 应改动格子"
        );
        assert!(w.distances.edges.len() < edges_before.len(), "apply 应删边");
        assert!(
            w.distances.places.len() > places_before,
            "apply 应新增链 Place"
        );

        cmd.revert(&mut w).expect("回退应成功");
        assert_eq!(w.terrains[&vid(MIG_MAP)], terrain_before, "格子应逐格恢复");
        assert_eq!(w.distances.places.len(), places_before, "迁移 Place 应删净");
        assert_eq!(w.distances.edges.len(), edges_before.len(), "边数应恢复");
        for e in &edges_before {
            assert!(
                w.distances.edges.iter().any(|x| x == e),
                "边 {} 应恢复",
                e.id
            );
        }
        // 顺序逐位复原（评审 M1）：整表相等而非仅同集合——含非 road 边的
        // 相对位置（e_sea 须回到下标 2）。
        assert_eq!(
            w.distances.edges, edges_before,
            "undo 后边表应与迁移前逐位一致"
        );
    }

    /// apply → revert → 再 apply：链 ID 确定性（端点字典序）⇒ 迁移 Place 数
    /// 与首跑相等（不增殖）。
    #[test]
    fn re_running_migration_after_undo_produces_no_duplicates() {
        let mut w = cross_world();
        let mig_count = |world: &World| {
            world
                .distances
                .places
                .iter()
                .filter(|p| p.id.as_str().starts_with("road_mig_"))
                .count()
        };
        let cmd = MigrateLegacyLineDataCommand::new();
        cmd.apply(&mut w).expect("首跑应成功");
        let first = mig_count(&w);
        assert_eq!(first, 4, "首跑应产 4 条链，实得 {}", first);
        cmd.revert(&mut w).expect("回退应成功");
        cmd.apply(&mut w).expect("再跑应成功");
        assert_eq!(
            mig_count(&w),
            first,
            "apply→revert→apply 的迁移 Place 数应与首跑相等"
        );
    }

    /// 菱形平行路（评审 I1）：两枢纽各经不同中继的两条路——链基名撞名时
    /// 第二条链加折线短哈希后缀落实体，**两条都存活**（旧实现静默跳过 =
    /// 第二条链的边被删而无线替代）；原 4 条平行路边全删；再跑一遍幂等
    /// 不增殖。
    ///
    /// 夹具注：枢纽各接一条码头支路，使枢纽的路网度数 ≠ 2——纯 4 边菱形
    /// 全节点度 2，走「纯环」分支（单闭合链，见 ADR-0070 §4.6 TODO），
    /// 而撞名场景的前提是枢纽成路口。
    #[test]
    fn parallel_route_chains_both_survive_migration() {
        let mut w = flat_world();
        w.distances.places = vec![
            mig_point("hub_a", 64.0, 64.0),
            mig_point("hub_b", 192.0, 192.0),
            mig_point("relay_n", 192.0, 64.0),
            mig_point("relay_s", 64.0, 192.0),
            mig_point("dock_a", 16.0, 64.0),
            mig_point("dock_b", 192.0, 240.0),
        ];
        w.distances.edges = vec![
            road_edge("e_an", "hub_a", "relay_n", 128.0),
            road_edge("e_nb", "relay_n", "hub_b", 128.0),
            road_edge("e_as", "hub_a", "relay_s", 128.0),
            road_edge("e_sb", "relay_s", "hub_b", 128.0),
            road_edge("e_da", "dock_a", "hub_a", 48.0),
            road_edge("e_bd", "hub_b", "dock_b", 48.0),
        ];

        let rep = migrate(&mut w);
        assert_eq!(rep.chains, 4, "两平行链 + 两支路链应全落地，实得 {:?}", rep);
        assert_eq!(
            rep.chains_skipped, 0,
            "撞名走后缀，不得跳过，实得 {:?}",
            rep
        );
        assert_eq!(
            rep.deleted_edges, 6,
            "六条手写边全消费（含原 4 条平行路边），实得 {:?}",
            rep
        );
        assert!(
            w.distances.edges.is_empty(),
            "原 4 条平行路边应全删，实得 {:?}",
            w.distances.edges
        );

        // 基名恰一条 + 带后缀恰一条：撞名的两条链都落实体。
        let base_id = "road_mig_hub_a_hub_b";
        let base_cnt = w
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str() == base_id)
            .count();
        assert_eq!(base_cnt, 1, "基名链应恰一条，实得 {}", base_cnt);
        let base = w
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == base_id)
            .expect("基名链在场");
        let suffixed = w
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str().starts_with(&format!("{}_", base_id)))
            .expect("第二条链应带哈希后缀落实体");
        let line_of = |p: &Place| p.line.clone().expect("有折线");
        let (lb, ls) = (line_of(base), line_of(suffixed));
        let ends = |l: &[f32]| (l[0], l[1], l[l.len() - 2], l[l.len() - 1]);
        assert_eq!(ends(&lb), ends(&ls), "两链端点应相同（hub_a/hub_b 锚点）",);
        assert_ne!(lb, ls, "两链折线应不同（各经不同中继）");

        // 再跑一遍（手写边已清空 → 空转）：幂等不增殖。
        let before_cnt = w
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with("road_mig_"))
            .count();
        let cmd = MigrateLegacyLineDataCommand::new();
        let dirty = cmd.apply(&mut w).expect("二次迁移应空转成功");
        assert!(
            !dirty.iter().any(|d| d == "distances/places"),
            "空转不得标脏 places，实得 {:?}",
            dirty
        );
        let after_cnt = w
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with("road_mig_"))
            .count();
        assert_eq!(
            after_cnt, before_cnt,
            "幂等：二次迁移不得增殖，实得 {} → {}",
            before_cnt, after_cnt
        );
    }

    /// 自环 road 边（评审 M3）：`a == a` 不成链（零长退化线无意义），计入
    /// chains_skipped，边仍被删除（留着会让 needs_line_migration 永真）。
    #[test]
    fn self_loop_road_edges_are_skipped_and_counted() {
        let mut w = flat_world();
        w.distances.places = vec![mig_point("loop_town", 128.0, 128.0)];
        w.distances.edges = vec![road_edge("e_loop", "loop_town", "loop_town", 5.0)];
        assert!(needs_line_migration(&w), "自环手写 road 边在场应判定需迁移");

        let rep = migrate(&mut w);
        assert!(
            rep.chains_skipped >= 1,
            "自环应计入 skipped，实得 {:?}",
            rep
        );
        assert_eq!(rep.chains, 0, "自环不得成链，实得 {:?}", rep);
        assert!(
            w.distances
                .places
                .iter()
                .all(|p| !p.id.as_str().starts_with("road_mig_")),
            "不得产 road_mig_a_a 实体，实得 {:?}",
            w.distances
                .places
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            w.distances.edges.is_empty(),
            "自环边应被删除，实得 {:?}",
            w.distances.edges
        );
        assert!(!needs_line_migration(&w), "迁移后应收敛");
    }

    /// `terrain == "sea"` 的边是非道路连接：迁移原样保留。
    #[test]
    fn sea_route_edges_untouched() {
        let mut w = cross_world();
        let sea = wv_core::map::distance::PlaceEdge {
            id: vid("e_sea"),
            a: vid("north_end"),
            b: vid("south_end"),
            distance_km: 200.0,
            terrain: "sea".into(),
            bidirectional: true,
            rank: None,
            derived: false,
        };
        w.distances.edges.push(sea);
        let cmd = MigrateLegacyLineDataCommand::new();
        cmd.apply(&mut w).expect("迁移应成功");
        let kept = w
            .distances
            .edges
            .iter()
            .find(|e| e.id.as_str() == "e_sea")
            .expect("sea 边应保留");
        assert_eq!(kept.terrain, "sea", "terrain 原样");
        assert_eq!(kept.distance_km, 200.0, "公里原样");
        assert_eq!(kept.a.as_str(), "north_end", "端点原样");
        assert!(
            w.distances.edges.iter().all(|e| e.terrain == "sea"),
            "道路边应全部消费，实得 {:?}",
            w.distances.edges
        );
    }

    /// 带 `rivers_seed` 的模板世界：首次迁移把派生河转为河线 POI 实体
    /// （无下切），二次 apply 不增殖。
    #[test]
    fn template_rivers_land_as_entities_on_first_load() {
        let mut w = World::default();
        w.maps.push(mig_map());
        // 20×20 全格 1000 m 高地：generate_rivers 必有源头与 ≥8 点折线。
        w.set_terrain(
            vid(MIG_MAP),
            wv_core::terrain::TerrainData {
                width: 20,
                height: 20,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height: 1000,
                        surface: SURFACE_LAND,
                        biome: 1,
                    };
                    400
                ],
            },
        );
        w.meta = Some(wv_core::WorldMeta {
            comment: None,
            name: "t".into(),
            version: "1".into(),
            schema_version: 2,
            root_map: vid(MIG_MAP),
            timeline_range: TimeSpan { from: 0, to: None },
            template: Some(wv_core::WorldTemplate {
                kind: wv_core::WorldTemplateKind::ColdNorth,
                seed: 1,
                rivers_seed: 42,
            }),
        });
        assert!(
            needs_line_migration(&w),
            "有 rivers_seed 而无河线 POI 应判定需迁移"
        );

        let river_count = |world: &World| {
            world
                .distances
                .places
                .iter()
                .filter(|p| p.id.as_str().starts_with("river_tpl_"))
                .count()
        };
        let cmd = MigrateLegacyLineDataCommand::new();
        cmd.apply(&mut w).expect("首次迁移应成功");
        let first = river_count(&w);
        assert!(first >= 1, "河线 Place 应 ≥1，实得 {}", first);
        let river = w
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == "river_tpl_00")
            .expect("应有 river_tpl_00");
        assert_eq!(river.line_kind, Some(LineKind::River), "河语义");
        assert_eq!(
            river.map.as_ref().map(|m| m.as_str()),
            Some(MIG_MAP),
            "锚定在带地形的图上"
        );
        assert!(
            river.line.as_ref().map(|l| l.len()).unwrap_or(0) >= 4,
            "至少两点的折线"
        );
        assert!(
            river.x.is_none() && river.y.is_none(),
            "锚点由折线中点派生，不写 x/y"
        );
        assert!(
            w.terrains[&vid(MIG_MAP)]
                .cells
                .iter()
                .all(|c| c.height == 1000),
            "模板河迁移无下切（height 全不动）"
        );
        assert_eq!(cmd.report().rivers, first, "诊断计数与实体数一致");

        // 二次 apply 不增殖；判据收敛。
        cmd.apply(&mut w).expect("二次 apply 应为无新增的空转");
        assert_eq!(river_count(&w), first, "二次 apply 不得增殖河实体");
        assert!(!needs_line_migration(&w), "迁移后判据应转假");
    }
}

/// 改线段 POI 的属性（名称 / 基准宽 / 呈现级别）。
///
/// `None` = **不改**（`UpdatePlaceCommand` 同语义）——不是「写成 None」。
/// `line` 本身的改动（重画折线）继续走既有 `UpdatePlaceLineCommand`；
/// `line_kind` 的改动走 `SetPlaceKindCommand` 式的独立命令待后续任务，
/// 本命令只管「这条线多宽、多显眼、叫什么」。
///
/// ## 回滚
///
/// `apply` 缓存整 `Place`（与 `UpdatePlaceCommand` 同款：快照整实体，
/// apply 改了哪些字段 revert 就退哪些，不漏）。
#[derive(Debug)]
pub struct UpdateLinePoiCommand {
    /// 目标地点 ID。
    pub id: wv_core::Id,
    /// 新名称（`None` = 不改）。
    pub new_name: Option<String>,
    /// 新基准宽米（`None` = 不改；仅河流语义消费，道路不落盘此字段）。
    pub new_width_m: Option<f32>,
    /// 新呈现级别（`None` = 不改）。
    pub new_rank: Option<u8>,
    before: std::sync::Mutex<Option<Place>>,
}

impl UpdateLinePoiCommand {
    pub fn new(
        id: wv_core::Id,
        new_name: Option<String>,
        new_width_m: Option<f32>,
        new_rank: Option<u8>,
    ) -> Self {
        Self {
            id,
            new_name,
            new_width_m,
            new_rank,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateLinePoiCommand {
    fn label(&self) -> String {
        format!("改线段地点「{}」", self.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(p.clone());
        if let Some(name) = &self.new_name {
            p.name = name.clone();
        }
        if let Some(w) = self.new_width_m {
            p.line_width_m = Some(w);
        }
        if let Some(r) = self.new_rank {
            p.rank = Some(r);
        }
        Ok(vec!["distances/places".into()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateLinePoi 尚未 apply".into()))?;
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        *p = prev;
        Ok(())
    }
}
