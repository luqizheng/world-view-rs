//! 编辑命令。
//!
//! 每个修改动作都实现 `EditCommand`。命令必须满足：
//! - 可序列化（用于事务日志与快照）；
//! - 可逆（或声明为不可逆，走「追加区间」路径）；
//! - 不直接触碰文件系统（那是 `wv-io` 的职责）。

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::World;
use wv_terrain::continent::{generate_continents, ContinentParams};
use wv_terrain::edit::{apply_stroke, Stroke};
use wv_terrain::intent::{apply_intent, Intent};
use wv_terrain::{TerrainCell, TerrainGrid};

/// `wv-core::terrain::TerrainData` → `wv-terrain::TerrainGrid` 的桥。
///
/// `wv-core` 不认识 `TerrainGrid`（不依赖 wv-terrain），因此两层的接缝在这里。
/// `wv-io` 有对称的一对函数用于落盘，二者共享同一布局约定（行优先、三通道）。
fn to_grid(data: &TerrainData) -> EditResult<TerrainGrid> {
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
        .ok_or_else(|| EditError::TerrainMismatch("cells 长度与尺寸不符".into()))
}

/// `TerrainGrid` → `TerrainData` 的桥（[`to_grid`] 的逆运算，保留尺寸与顺序）。
fn from_grid(grid: TerrainGrid) -> TerrainData {
    let (w, h) = (grid.width(), grid.height());
    let cells: Vec<TerrainCellData> = grid
        .into_cells()
        .into_iter()
        .map(|c| TerrainCellData {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    TerrainData {
        width: w,
        height: h,
        cells,
    }
}

pub type EditResult<T> = Result<T, EditError>;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("命令作用对象不存在: {0}")]
    TargetMissing(String),

    #[error("命令不满足前置条件: {0}")]
    Precondition(String),

    #[error("时间区间冲突: {0}")]
    SpanConflict(String),

    #[error("地形网格不存在或尺寸不符: {0}")]
    TerrainMismatch(String),
}

/// 编辑命令 trait。
///
/// `apply` 就地修改世界；`revert` 必须能把世界恢复到 `apply` 之前的状态。
/// 实现者若无法提供 `revert`，应改为「追加一个区间」而非覆盖。
///
/// ## 为什么要求 `Send + Sync`
///
/// `History` 会被 `wv-app` 作为 Bevy 的 `Resource` 持有，而 Bevy 资源必须是
/// `Send + Sync + 'static`（系统可跨线程并行）。因此命令对象也必须满足同一约束。
/// 需要「就地缓存旧值」的实现应使用 `Mutex` 而非 `RefCell`（见 [`PaintTerrainCommand`]）。
pub trait EditCommand: std::fmt::Debug + Send + Sync {
    /// 命令的人类可读描述，用于 Undo/Redo UI。
    fn label(&self) -> String;

    /// 执行。返回受影响的文件相对路径，供 `DirtySet` 使用。
    ///
    /// 由于命令需要 `revert`，实现通常把「被改动的旧值」缓存进内部可变状态
    /// （见 [`PaintTerrainCommand`] 的 `before` 字段）。
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>>;

    /// 回退。
    fn revert(&self, world: &mut World) -> EditResult<()>;
}

/// 刷地形 / 改海拔的命令。
///
/// ## 为什么命令要缓存 `before`
///
/// 命令必须可 `revert`，但网格是「就地修改」的。因此 `apply` 时把**受影响格的旧值**
/// 记进 `before`（`Mutex` 包裹以在 `&self` 下可变），`revert` 时原样写回。
///
/// 这正是 `wv-terrain` 与 `wv-edit` 的职责分界（基线 §2.2）：
/// **`wv-terrain` 不感知 Undo/Redo，`wv-edit` 不感知像素**。
#[derive(Debug)]
pub struct PaintTerrainCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 一次笔触（一串笔刷采样）。
    pub stroke: Stroke,
    /// 每格世界边长。
    pub cell_size: f32,
    /// `apply` 时缓存的受影响格旧值：`(gx, gy, before_cell)`。
    ///
    /// 用 `Mutex` 而非 `RefCell`，使命令可跨线程（Bevy 系统是并行的）。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl PaintTerrainCommand {
    pub fn new(map: wv_core::Id, stroke: Stroke, cell_size: f32) -> Self {
        Self {
            map,
            stroke,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 取回目标网格的**中立数据**。
    ///
    /// 查的是**这张图自己的**栅格（`World.terrains` 按图 ID 索引，Phase 2 起
    /// 每个图各有各的底图）。该图没有栅格时报 `TerrainMismatch`——不替作者
    /// 凭空造一张，因为尺寸该由作者的地图范围决定。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))
    }
}

impl EditCommand for PaintTerrainCommand {
    fn label(&self) -> String {
        format!("刷地形（{} 笔）", self.stroke.brushes.len())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let cell_size = self.cell_size;
        let stroke = &self.stroke;

        // ① 中立数据 → TerrainGrid
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        // ② 施加笔触（wv-terrain 的职责）
        apply_stroke(&mut grid, stroke, cell_size);
        // ③ 收集差异格作为 `before`（用于 revert）
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
        // ④ 写回中立数据（写回**这张图**的格子，不动别的图）
        world.set_terrain(self.map.clone(), from_grid(grid));

        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        for (gx, gy, cell) in before.iter() {
            grid.set(*gx, *gy, *cell);
        }
        drop(before);
        world.set_terrain(self.map.clone(), from_grid(grid));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 意图笔刷命令（2026-10-15，意图笔刷 plan v0.1）
// ---------------------------------------------------------------------------

/// 意图笔刷命令：作者给「意图 + 大致区域」，程序生成自然地形。
///
/// ## 与 [`PaintTerrainCommand`] 的分工
///
/// 两者都落 `maps/<map>/terrain.bin`、都用「快照 → 改格 → diff 缓存 → 写回 /
/// 按表回滚」的同一模式；差别只在第 ② 步调的是 `apply_stroke`（直接笔刷）
/// 还是 [`apply_intent`]（意图生成）。
///
/// ## seed 的确定性契约
///
/// `seed` 在 [`Self::new`] 里从 `(map, intent)` 派生并**存进命令**——
/// undo 后 redo 重放的是同一个 seed，wv-terrain 保证同 seed 同 intent 逐格
/// 一致，因此 redo 结果与首次 apply 完全相同。
#[derive(Debug)]
pub struct IntentTerrainCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 地形意图（山脉 / 高原 / 丘陵 / 火山 / 湖 / 河）。
    pub intent: Intent,
    /// 每格世界边长。
    pub cell_size: f32,
    /// 派生种子（构造时算好，随命令存活）。
    pub seed: u64,
    /// `apply` 时缓存的受影响格旧值：`(gx, gy, before_cell)`。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl IntentTerrainCommand {
    /// 构造时从图 ID 与意图内容派生 seed（见 [`Intent::derive_seed`]）。
    pub fn new(map: wv_core::Id, intent: Intent, cell_size: f32) -> Self {
        let seed = intent.derive_seed(map.as_str());
        Self {
            map,
            intent,
            cell_size,
            seed,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 与 [`PaintTerrainCommand::terrain`] 同款：按图查栅格，缺失报
    /// `TerrainMismatch`（不替作者凭空造尺寸）。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))
    }
}

impl EditCommand for IntentTerrainCommand {
    fn label(&self) -> String {
        let what = match &self.intent {
            Intent::MountainRange { .. } => "山脉",
            Intent::Plateau { .. } => "高原",
            Intent::Hills { .. } => "丘陵",
            Intent::Volcano { .. } => "火山",
            Intent::Lake { .. } => "湖",
            Intent::River { .. } => "河",
            Intent::MountainMassif { .. } => "高山群",
            Intent::HillsRegion { .. } => "丘陵",
            Intent::PlainsRegion { .. } => "平原",
            Intent::LakeRegion { .. } => "湖泊",
        };
        format!("生成{what}")
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        apply_intent(&mut grid, &self.intent, self.seed, self.cell_size);
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
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        for (gx, gy, cell) in before.iter() {
            grid.set(*gx, *gy, *cell);
        }
        drop(before);
        world.set_terrain(self.map.clone(), from_grid(grid));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 大陆生成（大陆生成 UI plan v0.1 Task 1；README §12-40）
// ---------------------------------------------------------------------------

/// 大陆生成宏命令：一次 apply 完成「意图包络 → 大陆配方 → 双通道栅格」。
///
/// ## 与 [`IntentTerrainCommand`] 的两点差异
///
/// 1. **seed 显式传入**，不在构造时 derive——面板上有骰子按钮，同一个圈
///    「重骰」语义要求作者能换 seed 再生成；意图笔刷的确定性来自
///    `Intent::derive_seed`，大陆生成则把骰子交给作者。
/// 2. 一次命令可带**多条包络**（多块大陆同批生成），撤销是 Q16 的
///    原子语义：一撤全回。
///
/// before 缓存与 [`IntentTerrainCommand`] 同款：只记真正变过的格，
/// 撤销逐格回放；`revert` 后 `before` 保留，redo 重放同一 seed 结果逐位一致。
#[derive(Debug)]
pub struct GenerateContinentCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 意图包络（每条 ≥ 3 点；世界单位）。图层面不落盘——只活在命令里。
    pub envelopes: Vec<Vec<(f32, f32)>>,
    /// 大陆配方八参数（全局单一面板）。
    pub params: ContinentParams,
    /// 骰子种子（面板「重骰」直接换这个值）。
    pub seed: u64,
    /// 每格世界边长。
    pub cell_size: f32,
    /// `apply` 时缓存的受影响格旧值：`(gx, gy, before_cell)`。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl GenerateContinentCommand {
    /// 按作者面板参数构造；`seed` 显式给出（见类型文档）。
    pub fn new(
        map: wv_core::Id,
        envelopes: Vec<Vec<(f32, f32)>>,
        params: ContinentParams,
        seed: u64,
        cell_size: f32,
    ) -> Self {
        Self {
            map,
            envelopes,
            params,
            seed,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 与 [`IntentTerrainCommand::terrain`] 同款：缺失栅格报 `TerrainMismatch`，
    /// 不替作者凭空造尺寸（V1：只提示，不阻止）。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))
    }
}

impl EditCommand for GenerateContinentCommand {
    fn label(&self) -> String {
        "生成大陆".to_string()
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        generate_continents(
            &mut grid,
            &self.envelopes,
            &self.params,
            self.seed,
            self.cell_size,
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
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        for (gx, gy, cell) in before.iter() {
            grid.set(*gx, *gy, *cell);
        }
        drop(before);
        world.set_terrain(self.map.clone(), from_grid(grid));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 地图新建（2026-10-14：「新建地图」对话框的落点）
// ---------------------------------------------------------------------------

/// 新增一张地图（节点 + 父引用 + 可选预填充栅格）。
///
/// ## 为什么「挂到父图 children」是命令的一部分
///
/// P4 的层级关系只存 ID 引用（`MapNode.parent` / `children`），新节点不挂进
/// 父图的 `children` 就是一张悬空图——钻取树、地图树都看不见它。三件事
/// （push 节点、回填父引用、可选写栅格）必须同生共死，`revert` 才能完整回退。
///
/// ## 幂等保护
///
/// 目标 ID 已存在时 `apply` 报错（调用方——「新建地图」对话框——负责先生成
/// 唯一 ID）；命令层不静默覆盖，那等于悄悄毁掉一张已有的图。
#[derive(Debug)]
pub struct AddMapCommand {
    /// 新地图节点（`parent` 必须已指向既有地图；根图 `parent` 为 `None`）。
    pub node: wv_core::map::MapNode,
    /// 预填充栅格（海洋 / 陆地）。`None` = 不带栅格（沿用父图底图继承）。
    pub terrain: Option<TerrainData>,
}

impl AddMapCommand {
    pub fn new(node: wv_core::map::MapNode, terrain: Option<TerrainData>) -> Self {
        Self { node, terrain }
    }

    /// 受影响的目录：节点 YAML + 预填充栅格的 `terrain.bin`。
    fn dirty_paths(&self) -> Vec<String> {
        let mut paths = vec!["maps".to_owned()];
        if self.terrain.is_some() {
            paths.push(format!("maps/{}/terrain.bin", self.node.id));
        }
        paths
    }
}

impl EditCommand for AddMapCommand {
    fn label(&self) -> String {
        format!("新建地图「{}」", self.node.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.maps.iter().any(|m| m.id == self.node.id) {
            return Err(EditError::Precondition(format!(
                "地图 `{}` 已存在",
                self.node.id
            )));
        }
        // 父引用必须在树上真实存在（悬空 parent 会让钻取与校验双输）。
        if let Some(parent) = &self.node.parent {
            let Some(pnode) = world.maps.iter_mut().find(|m| &m.id == parent) else {
                return Err(EditError::TargetMissing(format!("父地图 `{parent}`")));
            };
            pnode.children.push(self.node.id.clone());
        }
        if let Some(data) = &self.terrain {
            world.set_terrain(self.node.id.clone(), data.clone());
        }
        world.maps.push(self.node.clone());
        Ok(self.dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        // 对称拆三件套：移除节点、摘掉父引用、丢弃预填充栅格。
        let existed = world
            .maps
            .iter()
            .position(|m| m.id == self.node.id)
            .map(|i| world.maps.remove(i))
            .is_some();
        if !existed {
            return Err(EditError::TargetMissing(format!(
                "地图 `{}` 不存在，无法撤销新建",
                self.node.id
            )));
        }
        if let Some(parent) = &self.node.parent {
            if let Some(pnode) = world.maps.iter_mut().find(|m| &m.id == parent) {
                pnode.children.retain(|c| c != &self.node.id);
            }
        }
        world.terrains.remove(&self.node.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 城市子图编辑命令（ADR-0022 · 2026-10-16）
// ---------------------------------------------------------------------------

use wv_core::base::geometry::Point;
use wv_core::map::{GeometryData, IconShape, IconSpec, MapNode};
use wv_core::Id;

/// 新建城市子图（ADR-0022 D5 · `AddCityCommand`）。
///
/// 在指定父图下挂一张 `MapLevel::City` 子图，同时在父图上落一个 `IconSpec`
///（默认菱形，`drill_into = city.id`）作为钻入入口——这是用户在父图上
/// 「看见这座城」的唯一视觉锚点。
///
/// ## 与 `AddMapCommand` 的边界
///
/// `AddMapCommand` 是「建任意图」的通用入口（任意 `MapLevel`、可选 terrain）；
/// `AddCityCommand` 是「建城市图」的特化版本：
/// - 强制 `level = MapLevel::City`（建错了 call site 应自行校验）；
/// - **不**带 terrain 预填（城市图继承父图底图即可，per ADR-0009 §7）；
/// - 自动落 `IconSpec`（普通子图不需要这个）。
///
/// ## 河流复制
///
/// M2 阶段**不**复制父图河流入子图——这是 M3 (`D3`) 的语义；
/// 本命令 apply 仅挂图标 + 注册子图节点。M3 接 D3 时在本命令的 `apply`
/// 末尾追加 clipping 步骤即可，向后兼容。
///
/// ## 脏路径
///
/// - `maps`（新增子图节点的 `index.yaml`）
/// - `maps/{parent_id}/index.yaml`（父图加了 icons / children）
#[derive(Debug)]
pub struct AddCityCommand {
    pub city: MapNode,
    /// 钻入图标的 ID（`Id` 字符集 `[A-Za-z0-9_]`）。
    pub icon_id: Id,
    /// 钻入图标的形状（默认菱形；可用 `IconShape::City` 之类预留枚举）。
    pub icon_shape: IconShape,
    /// 钻入图标的中心锚点（[0,1]²，父图 extent 归一化）。默认 `(0.5, 0.5)`。
    pub icon_anchor: [f32; 2],
}

impl AddCityCommand {
    pub fn new(city: MapNode, icon_id: Id, icon_shape: IconShape) -> Self {
        Self {
            city,
            icon_id,
            icon_shape,
            icon_anchor: [0.5, 0.5],
        }
    }
}

impl EditCommand for AddCityCommand {
    fn label(&self) -> String {
        format!("新建城市「{}」", self.city.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.maps.iter().any(|m| m.id == self.city.id) {
            return Err(EditError::Precondition(format!(
                "城市 `{}` 已存在",
                self.city.id
            )));
        }
        let parent_id = self
            .city
            .parent
            .clone()
            .ok_or_else(|| EditError::Precondition("城市必须挂在某父图下".into()))?;
        let parent = world
            .maps
            .iter_mut()
            .find(|m| m.id == parent_id)
            .ok_or_else(|| EditError::TargetMissing(format!("父图 `{parent_id}`")))?;
        parent.icons.push(IconSpec {
            id: self.icon_id.clone(),
            anchor: Some(self.icon_anchor),
            shape: self.icon_shape,
            size: None,
            color: None,
            drill_into: Some(self.city.id.clone()),
        });
        parent.children.push(self.city.id.clone());

        let city_id = self.city.id.clone();
        world.maps.push(self.city.clone());
        Ok(vec![
            "maps".to_owned(),
            format!("maps/{parent_id}/index.yaml"),
            format!("maps/{city_id}/index.yaml"),
        ])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let existed = world
            .maps
            .iter()
            .position(|m| m.id == self.city.id)
            .map(|i| world.maps.remove(i))
            .is_some();
        if !existed {
            return Err(EditError::TargetMissing(format!(
                "城市 `{}` 不存在，无法撤销新建",
                self.city.id
            )));
        }
        let parent_id_ref = self
            .city
            .parent
            .as_ref()
            .ok_or_else(|| EditError::Precondition("城市必须挂在某父图下".into()))?;
        if let Some(parent) = world.maps.iter_mut().find(|m| &m.id == parent_id_ref) {
            parent.children.retain(|c| c != &self.city.id);
            parent
                .icons
                .retain(|ic| ic.drill_into.as_ref() != Some(&self.city.id));
        }
        Ok(())
    }
}

/// 平移城市（ADR-0022 D2 · `MoveCityCommand`）。
///
/// 把城市在父图坐标系里平移 `delta`（wu）：
/// - `extent.origin` += delta（若 `Some`）
/// - `outline_polygon`（`Polygon` / `PolygonWithHoles`，含洞环）所有顶点 += delta
/// - 子图 LOCAL 几何（街道、地点、building）**不动**——per ADR-0009 §9 「本图 local」
/// - 引用该城市的 `Place` / `Track` / `Faction` **不动**——它们是独立实体
///
/// ## revert
///
/// 缓存整张 `MapNode` 的快照，`revert` 时把 `extent` 与 `outline_polygon` 还原；
/// 其余字段（icons / layers / children / 名字等）不参与同步，保留当前值。
/// 这避免了「撤回期间又有人改了同一节点」导致的字段级不一致。
///
/// ## 失败语义
///
/// - 城市不存在：`TargetMissing`
/// - `outline_polygon = Some(Points)` 或 `Ref`：`Precondition`（不支持变体）
#[derive(Debug)]
pub struct MoveCityCommand {
    pub city_id: Id,
    pub delta: Point,
    before_snapshot: std::sync::Mutex<Option<MapNode>>,
}

impl MoveCityCommand {
    pub fn new(city_id: Id, delta: Point) -> Self {
        Self {
            city_id,
            delta,
            before_snapshot: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for MoveCityCommand {
    fn label(&self) -> String {
        format!(
            "平移城市「{}」by ({:.1}, {:.1})",
            self.city_id, self.delta.x, self.delta.y
        )
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        // 缓存快照。
        *self
            .before_snapshot
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = Some(node.clone());
        // 平移 extent.origin。
        if let Some(ext) = node.extent.as_mut() {
            let origin = ext.origin.unwrap_or(Point::ORIGIN);
            ext.origin = Some(Point {
                x: origin.x + self.delta.x,
                y: origin.y + self.delta.y,
            });
        }
        // 平移 outline_polygon（仅 Polygon 变体）。
        if let Some(poly) = node.outline_polygon.as_mut() {
            match poly {
                GeometryData::Polygon(v) => {
                    for chunk in v.chunks_exact_mut(2) {
                        chunk[0] += self.delta.x;
                        chunk[1] += self.delta.y;
                    }
                }
                GeometryData::PolygonWithHoles { outer, holes } => {
                    for chunk in outer.chunks_exact_mut(2) {
                        chunk[0] += self.delta.x;
                        chunk[1] += self.delta.y;
                    }
                    for hole in holes {
                        for chunk in hole.chunks_exact_mut(2) {
                            chunk[0] += self.delta.x;
                            chunk[1] += self.delta.y;
                        }
                    }
                }
                GeometryData::Points(_) | GeometryData::Ref(_) => {
                    return Err(EditError::Precondition(format!(
                        "城市「{}」的 outline_polygon 不是多边形变体，不支持平移",
                        self.city_id
                    )));
                }
            }
        }
        Ok(vec![format!("maps/{}/index.yaml", self.city_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snap = self
            .before_snapshot
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("MoveCity 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        node.extent = snap.extent;
        node.outline_polygon = snap.outline_polygon;
        Ok(())
    }
}

/// 重画城市轮廓（ADR-0022 D5 · `SetCityOutlineCommand`）。
///
/// 用新 polygon 覆盖 `outline_polygon`（允许 `None` 把轮廓擦掉）；
/// `revert` 缓存旧值原子还原。
///
/// ## 用途
///
/// 用户在父图上重画轮廓（拖 4 个角点编辑）；与 `AddCityCommand` 的
/// `outline_polygon` 字段共享同一 `GeometryData::Polygon` 变体约定。
#[derive(Debug)]
pub struct SetCityOutlineCommand {
    pub city_id: Id,
    pub new_polygon: Option<GeometryData>,
    before: std::sync::Mutex<Option<Option<GeometryData>>>,
}

impl SetCityOutlineCommand {
    pub fn new(city_id: Id, new_polygon: Option<GeometryData>) -> Self {
        Self {
            city_id,
            new_polygon,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetCityOutlineCommand {
    fn label(&self) -> String {
        if self.new_polygon.is_some() {
            format!("重画城市「{}」轮廓", self.city_id)
        } else {
            format!("擦除城市「{}」轮廓", self.city_id)
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        if let Some(GeometryData::Polygon(v)) = &self.new_polygon {
            if v.len() % 2 != 0 || v.len() < 6 {
                return Err(EditError::Precondition(
                    "outline_polygon 必须是 ≥3 顶点的扁平坐标对".into(),
                ));
            }
        }
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? =
            Some(node.outline_polygon.clone());
        node.outline_polygon = self.new_polygon.clone();
        Ok(vec![format!("maps/{}/index.yaml", self.city_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("SetCityOutline 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        node.outline_polygon = prev;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 势力编辑命令（S12 v0.2）
// ---------------------------------------------------------------------------

use wv_core::map::GeometrySpan;
use wv_core::{Color, Faction, FactionColorMode, FactionLayer, Membership};

/// 受影响的目录相对路径，供 `DirtySet` 落盘聚合。
///
/// Faction 与 GeometrySpan 是 YAML 实体（§4.5），分别落在
/// `factions/<id>.yaml` 与 `geometry/faction/<id>.yaml`；命令层无法知道
/// 具体文件名（owner 字符串），只声明目录，IO 落盘时再具体化。
fn faction_dirty_paths() -> Vec<String> {
    vec!["factions".into(), "geometry/faction".into()]
}

/// 势力层命令的脏路径：层与势力都写（删层会把成员的 `layer` 置空）。
fn faction_layer_dirty_paths() -> Vec<String> {
    vec!["faction_layers".into(), "factions".into()]
}

/// 新增一个 `Faction`。
///
/// `apply` 在 `world.factions` 末尾追加；`revert` 移除（按 ID 定位）。
#[derive(Debug)]
pub struct AddFactionCommand {
    pub faction: Faction,
}

impl AddFactionCommand {
    pub fn new(faction: Faction) -> Self {
        Self { faction }
    }
}

impl EditCommand for AddFactionCommand {
    fn label(&self) -> String {
        format!("新增势力「{}」", self.faction.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.factions.iter().any(|f| f.id == self.faction.id) {
            return Err(EditError::Precondition(format!(
                "势力 `{}` 已存在",
                self.faction.id
            )));
        }
        world.factions.push(self.faction.clone());
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .factions
            .iter()
            .position(|f| f.id == self.faction.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.faction.id)))?;
        world.factions.remove(pos);
        Ok(())
    }
}

/// 删除一个 `Faction`（及其名下的所有 `GeometrySpan`——避免悬空几何）。
///
/// **为什么连带删几何**：保留悬空几何会导致 `factions_at` 永远跳过该 owner，
/// 让作者误以为「数据没了」实则还在——更糟的是未来带洞多边形规则启用时，
/// 悬空会让校验规则错乱。一并删、撤销时一并恢复，是最干净的语义。
#[derive(Debug)]
pub struct RemoveFactionCommand {
    pub id: wv_core::Id,
    /// `apply` 时缓存的 Faction 本体（用于 revert 还原）。
    before_faction: std::sync::Mutex<Option<Faction>>,
    /// `apply` 时缓存的、本 Faction 名下所有 GeometrySpan（用于 revert 还原）。
    before_geos: std::sync::Mutex<Vec<GeometrySpan>>,
}

impl RemoveFactionCommand {
    pub fn new(id: wv_core::Id) -> Self {
        Self {
            id,
            before_faction: std::sync::Mutex::new(None),
            before_geos: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for RemoveFactionCommand {
    fn label(&self) -> String {
        format!("删除势力「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .factions
            .iter()
            .position(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        // 缓存本体与所属几何。
        let f = world.factions.remove(pos);
        let geos: Vec<GeometrySpan> = world
            .faction_geometry
            .iter()
            .filter(|g| g.owner == self.id)
            .cloned()
            .collect();
        world.faction_geometry.retain(|g| g.owner != self.id);
        *self
            .before_faction
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f);
        *self
            .before_geos
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = geos;
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let f = self
            .before_faction
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveFaction 尚未 apply".into()))?;
        let geos = self
            .before_geos
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        // 恢复几何（顺序不影响 `factions_at`，仅影响迭代顺序）。
        for g in geos {
            world.faction_geometry.push(g);
        }
        // 恢复 Faction 到原位（按 ID 排序语义不强——找首个能让 ID 升序的位置）。
        let insert_at = world
            .factions
            .iter()
            .position(|x| x.id > f.id)
            .unwrap_or(world.factions.len());
        world.factions.insert(insert_at, f);
        Ok(())
    }
}

/// 重命名 / 改色。
///
/// `apply` 缓存旧 `Faction` 全字段；`revert` 用缓存恢复。
#[derive(Debug)]
pub struct UpdateFactionCommand {
    pub id: wv_core::Id,
    pub new_name: String,
    pub new_color: Color,
    before: std::sync::Mutex<Option<Faction>>,
}

impl UpdateFactionCommand {
    pub fn new(id: wv_core::Id, new_name: String, new_color: Color) -> Self {
        Self {
            id,
            new_name,
            new_color,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateFactionCommand {
    fn label(&self) -> String {
        format!("改势力「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.name = self.new_name.clone();
        f.color = self.new_color.clone();
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateFaction 尚未 apply".into()))?;
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *f = prev;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 势力层 / 归属边命令（ADR-0027）
// ---------------------------------------------------------------------------

/// 新增一个势力层。
#[derive(Debug)]
pub struct AddFactionLayerCommand {
    pub layer: FactionLayer,
}

impl AddFactionLayerCommand {
    pub fn new(layer: FactionLayer) -> Self {
        Self { layer }
    }
}

impl EditCommand for AddFactionLayerCommand {
    fn label(&self) -> String {
        format!("新增势力层「{}」", self.layer.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.faction_layers.iter().any(|l| l.id == self.layer.id) {
            return Err(EditError::Precondition(format!(
                "势力层 `{}` 已存在",
                self.layer.id
            )));
        }
        world.faction_layers.push(self.layer.clone());
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .faction_layers
            .iter()
            .position(|l| l.id == self.layer.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.layer.id)))?;
        world.faction_layers.remove(pos);
        Ok(())
    }
}

/// 修改一个势力层的名称 / z / 配色策略 / 可见性（全字段快照，undo 原样恢复）。
#[derive(Debug)]
pub struct UpdateFactionLayerCommand {
    pub id: wv_core::Id,
    pub new_name: String,
    pub new_z: i32,
    pub new_color_mode: FactionColorMode,
    pub new_visible: bool,
    before: std::sync::Mutex<Option<FactionLayer>>,
}

impl UpdateFactionLayerCommand {
    pub fn new(
        id: wv_core::Id,
        new_name: String,
        new_z: i32,
        new_color_mode: FactionColorMode,
        new_visible: bool,
    ) -> Self {
        Self {
            id,
            new_name,
            new_z,
            new_color_mode,
            new_visible,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateFactionLayerCommand {
    fn label(&self) -> String {
        format!("改势力层「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let l = world
            .faction_layers
            .iter_mut()
            .find(|l| l.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(l.clone());
        l.name = self.new_name.clone();
        l.z = self.new_z;
        l.color_mode = self.new_color_mode;
        l.visible = self.new_visible;
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateFactionLayer 尚未 apply".into()))?;
        let l = world
            .faction_layers
            .iter_mut()
            .find(|l| l.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.id)))?;
        *l = prev;
        Ok(())
    }
}

/// 删除一个势力层：层下势力 `layer` 置 `None`（回落默认层），**不删势力本体**。
#[derive(Debug)]
pub struct RemoveFactionLayerCommand {
    pub id: wv_core::Id,
    before_layer: std::sync::Mutex<Option<(usize, FactionLayer)>>,
    before_factions: std::sync::Mutex<Vec<(wv_core::Id, Option<wv_core::Id>)>>,
}

impl RemoveFactionLayerCommand {
    pub fn new(id: wv_core::Id) -> Self {
        Self {
            id,
            before_layer: std::sync::Mutex::new(None),
            before_factions: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for RemoveFactionLayerCommand {
    fn label(&self) -> String {
        format!("删除势力层「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .faction_layers
            .iter()
            .position(|l| l.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.id)))?;
        let layer = world.faction_layers.remove(pos);
        let mut affected = Vec::new();
        for f in &mut world.factions {
            if f.layer.as_ref() == Some(&self.id) {
                affected.push((f.id.clone(), f.layer.clone()));
                f.layer = None;
            }
        }
        *self
            .before_layer
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, layer));
        *self
            .before_factions
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = affected;
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, layer) = self
            .before_layer
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveFactionLayer 尚未 apply".into()))?;
        let affected = self
            .before_factions
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        for (fid, old_layer) in affected {
            if let Some(f) = world.factions.iter_mut().find(|f| f.id == fid) {
                f.layer = old_layer;
            }
        }
        let at = pos.min(world.faction_layers.len());
        world.faction_layers.insert(at, layer);
        Ok(())
    }
}

/// 把势力挂到某个层（`None` = 默认层）。
#[derive(Debug)]
pub struct SetFactionLayerCommand {
    pub id: wv_core::Id,
    pub layer: Option<wv_core::Id>,
    before: std::sync::Mutex<Option<Faction>>,
}

impl SetFactionLayerCommand {
    pub fn new(id: wv_core::Id, layer: Option<wv_core::Id>) -> Self {
        Self {
            id,
            layer,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetFactionLayerCommand {
    fn label(&self) -> String {
        format!("改势力「{}」的势力层", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.layer = self.layer.clone();
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.id, &self.before, "SetFactionLayer")
    }
}

/// 改势力的等级提示（自由字符串，不参与层级判定）。
#[derive(Debug)]
pub struct SetFactionRankCommand {
    pub id: wv_core::Id,
    pub rank: Option<String>,
    before: std::sync::Mutex<Option<Faction>>,
}

impl SetFactionRankCommand {
    pub fn new(id: wv_core::Id, rank: Option<String>) -> Self {
        Self {
            id,
            rank,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetFactionRankCommand {
    fn label(&self) -> String {
        format!("改势力「{}」的等级提示", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.rank = self.rank.clone();
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.id, &self.before, "SetFactionRank")
    }
}

/// 改势力的驻地 / 首府引用（`None` = 不锚定）。
#[derive(Debug)]
pub struct SetFactionSeatCommand {
    pub id: wv_core::Id,
    pub seat: Option<wv_core::Id>,
    before: std::sync::Mutex<Option<Faction>>,
}

impl SetFactionSeatCommand {
    pub fn new(id: wv_core::Id, seat: Option<wv_core::Id>) -> Self {
        Self {
            id,
            seat,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetFactionSeatCommand {
    fn label(&self) -> String {
        format!("改势力「{}」的驻地", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.seat = self.seat.clone();
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.id, &self.before, "SetFactionSeat")
    }
}

/// 追加一条归属边。同一 target 不同 from 的多条边**不合并**（多对多 + 时间）。
#[derive(Debug)]
pub struct AddMembershipCommand {
    pub faction: wv_core::Id,
    pub membership: Membership,
    before: std::sync::Mutex<Option<Faction>>,
}

impl AddMembershipCommand {
    pub fn new(faction: wv_core::Id, membership: Membership) -> Self {
        Self {
            faction,
            membership,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for AddMembershipCommand {
    fn label(&self) -> String {
        format!(
            "给势力「{}」添加归属 →「{}」",
            self.faction, self.membership.target
        )
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.faction)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.faction)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.memberships.push(self.membership.clone());
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.faction, &self.before, "AddMembership")
    }
}

/// 删除一条归属边（按 `(target, from)` 定位；同 target 可有多条不同 from）。
#[derive(Debug)]
pub struct RemoveMembershipCommand {
    pub faction: wv_core::Id,
    pub target: wv_core::Id,
    pub from: i64,
    before: std::sync::Mutex<Option<Faction>>,
}

impl RemoveMembershipCommand {
    pub fn new(faction: wv_core::Id, target: wv_core::Id, from: i64) -> Self {
        Self {
            faction,
            target,
            from,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveMembershipCommand {
    fn label(&self) -> String {
        format!("删除势力「{}」的归属 →「{}」", self.faction, self.target)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.faction)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.faction)))?;
        let pos = f
            .memberships
            .iter()
            .position(|m| m.target == self.target && m.span.from == self.from)
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "Membership {} -> {} from {}",
                    self.faction, self.target, self.from
                ))
            })?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.memberships.remove(pos);
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.faction, &self.before, "RemoveMembership")
    }
}

/// 用已缓存的整份 `Faction` 快照恢复（恢复全部字段，不只被改的那一个）。
fn restore_faction_snapshot(
    world: &mut World,
    id: &wv_core::Id,
    before: &std::sync::Mutex<Option<Faction>>,
    what: &str,
) -> EditResult<()> {
    let prev = before
        .lock()
        .map_err(|_| EditError::Precondition("锁中毒".into()))?
        .clone()
        .ok_or_else(|| EditError::Precondition(format!("{what} 尚未 apply")))?;
    let f = world
        .factions
        .iter_mut()
        .find(|f| &f.id == id)
        .ok_or_else(|| EditError::TargetMissing(format!("Faction {id}")))?;
    *f = prev;
    Ok(())
}

/// 替换/新增一个势力的几何（按 `(map, owner, span.from, relation)` 视为同一实体的多版本）。
///
/// **同一性**：`GeometrySpan` 自身没有 `id` 字段（看 `map.rs`），但 路线图 §4.1
/// 原则六要求「同一实体同一维度的状态片段不覆盖」——这里把
/// `(map, owner, from, relation)` 四元组视为「实体的某次疆界变更」；四元组相同
/// → 是同一次变更的更新（**只允许一条**），不同 → 是不同次变更（多版本并存）。
/// 加入 `relation` 是 ADR-0027 的必要条件：同一势力同一天可以既有实控又有声称。
///
/// `apply` 若发现同 `(map, owner, from)` 已存在 → 替换；否则追加。`revert` 用
/// 缓存的"原位置 + 旧值"还原。
#[derive(Debug)]
pub struct UpsertFactionGeometryCommand {
    pub span: GeometrySpan,
    /// `apply` 时若发现已存在，原 span 的位置（用于 revert 还原顺序）。
    before_index: std::sync::Mutex<Option<usize>>,
    /// `apply` 时若发现已存在，原 span 的内容（用于 revert 还原值）。
    before_existing: std::sync::Mutex<Option<GeometrySpan>>,
}

impl UpsertFactionGeometryCommand {
    pub fn new(span: GeometrySpan) -> Self {
        Self {
            span,
            before_index: std::sync::Mutex::new(None),
            before_existing: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpsertFactionGeometryCommand {
    fn label(&self) -> String {
        format!("改势力「{}」在 {} 的疆界", self.span.owner, self.span.map)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 校验：owner 必须是已注册势力（V1 守约，悬空 owner 在 IO 加载时已经被
        // `factions_at` 静默跳过；编辑器里**创建新几何**则强制要求先有势力）。
        if !world.factions.iter().any(|f| f.id == self.span.owner) {
            return Err(EditError::Precondition(format!(
                "势力 `{}` 不存在；先创建势力再编辑几何",
                self.span.owner
            )));
        }
        let new_from = self.span.span.from;
        let pos = world.faction_geometry.iter().position(|g| {
            g.map == self.span.map
                && g.owner == self.span.owner
                && g.span.from == new_from
                && g.relation == self.span.relation
        });
        match pos {
            Some(i) => {
                let prev = world.faction_geometry[i].clone();
                world.faction_geometry[i] = self.span.clone();
                *self
                    .before_index
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(i);
                *self
                    .before_existing
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(prev);
            }
            None => {
                world.faction_geometry.push(self.span.clone());
                *self
                    .before_index
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))? = None;
            }
        }
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let idx = *self
            .before_index
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?;
        match idx {
            Some(i) => {
                let prev = self
                    .before_existing
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))?
                    .clone()
                    .ok_or_else(|| EditError::Precondition("UpsertGeometry 尚未 apply".into()))?;
                if i < world.faction_geometry.len() {
                    world.faction_geometry[i] = prev;
                } else {
                    return Err(EditError::Precondition("revert 时几何位置越界".into()));
                }
            }
            None => {
                // 之前是追加；revert 时移除最后一条（apply 必是 push）。
                if world
                    .faction_geometry
                    .last()
                    .map(|g| g == &self.span)
                    .unwrap_or(false)
                {
                    world.faction_geometry.pop();
                } else {
                    return Err(EditError::Precondition(
                        "UpsertGeometry revert 找不到对应 span".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// 删除一个势力的某次几何（按 `(map, owner, from)` 定位）。
///
/// 与 `UpsertFactionGeometryCommand` 的同一性规则对称。
#[derive(Debug)]
pub struct RemoveFactionGeometryCommand {
    pub map: wv_core::Id,
    pub owner: wv_core::Id,
    pub from: i64,
    before: std::sync::Mutex<Option<(usize, GeometrySpan)>>,
}

impl RemoveFactionGeometryCommand {
    pub fn new(map: wv_core::Id, owner: wv_core::Id, from: i64) -> Self {
        Self {
            map,
            owner,
            from,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveFactionGeometryCommand {
    fn label(&self) -> String {
        format!("删除「{}」的某次疆界", self.owner)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .faction_geometry
            .iter()
            .position(|g| g.map == self.map && g.owner == self.owner && g.span.from == self.from)
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "GeometrySpan (map={}, owner={}, from={})",
                    self.map, self.owner, self.from
                ))
            })?;
        let prev = world.faction_geometry.remove(pos);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, prev));
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, prev) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveGeometry 尚未 apply".into()))?;
        let insert_at = pos.min(world.faction_geometry.len());
        world.faction_geometry.insert(insert_at, prev);
        Ok(())
    }
}

/// 划分模式（ADR-0028 D5）：与 `wv_terrain::divide::EdgeMode` 同义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DivisionMode {
    /// 边界吸附地物线（水陆界面 / 河链 / 山脊峰值）。
    #[default]
    Snap,
    /// 无视地貌，保留栅格提取的平滑边框。
    Straight,
}

impl DivisionMode {
    fn edge_mode(self) -> wv_terrain::divide::EdgeMode {
        match self {
            Self::Snap => wv_terrain::divide::EdgeMode::Snap,
            Self::Straight => wv_terrain::divide::EdgeMode::Straight,
        }
    }
}

/// 划分执行前的宿主快照（revert 用）。
#[derive(Debug, Clone, Default)]
struct BeforeDivision {
    host_index: Option<usize>,
    host_before: Option<GeometrySpan>,
    /// 宿主 span 被整体移除（意图吃光宿主）时 `revert` 需要插回。
    host_removed: bool,
    /// 历史切分时新插入的余部 span 下标（`revert` 删除）。
    remainder_index: Option<usize>,
}

/// 从宿主中间划出新势力（ADR-0028 D1/D2，势力交互 M1 Task 5）。
///
/// # 语义
///
/// - `host = Some(id)`：在该势力**生效中的疆界**里切下意图多边形——新势力
///   疆界 = 意图 ∩ 宿主，宿主疆界原位替换为余部（可为带洞环）。两环来自
///   同一次掩膜边界提取，**共边由构造保证**（D3）。
/// - `host = None`：空势力宿主——整张图的**陆格**都是宿主（水域归空势力，
///   D4/Q3）；只写新势力疆界，**不写任何宿主 span**。
///
/// # 原子性
///
/// 单命令单事务：失败（碎块化 / 意图不在宿主内 / 无地形）→ `Precondition`，
/// 世界零改动；`revert` 移除新势力及其全部几何并逐位还原宿主 span。
#[derive(Debug)]
pub struct DivideFactionCommand {
    pub map: wv_core::Id,
    /// 宿主势力；`None` = 空势力。
    pub host: Option<wv_core::Id>,
    /// 意图多边形（图内局部 wu）。
    pub intent: Vec<(f32, f32)>,
    pub mode: DivisionMode,
    /// 新势力（id / name / color 由调用方准备）。
    pub faction: Faction,
    /// 生效日（`TimeSpan::from`）。
    pub from: i64,
    /// 图内一格的世界单位（wu → 格坐标换算）。
    pub cell_size: f32,
    /// 贴地搜索窗口（格）。4 格 ≈ 4×cell_size wu。
    search_cells: f32,
    before: std::sync::Mutex<BeforeDivision>,
    /// `apply` 后置位：贴地是否真的吸附到地物（Q2 退化回执）。
    /// 由调用方经 [`Self::snapped_flag`] 共享读取（命令会被装进 `Box<dyn>`）。
    snapped: std::sync::Arc<std::sync::Mutex<bool>>,
}

impl DivideFactionCommand {
    pub fn new(
        map: wv_core::Id,
        host: Option<wv_core::Id>,
        intent: Vec<(f32, f32)>,
        mode: DivisionMode,
        faction: Faction,
        from: i64,
        cell_size: f32,
    ) -> Self {
        Self {
            map,
            host,
            intent,
            mode,
            faction,
            from,
            cell_size,
            search_cells: 4.0,
            before: std::sync::Mutex::new(BeforeDivision::default()),
            snapped: std::sync::Arc::new(std::sync::Mutex::new(false)),
        }
    }

    /// 共享的「贴地是否吸附到地物」标志（`apply` 之后读）。
    pub fn snapped_flag(&self) -> std::sync::Arc<std::sync::Mutex<bool>> {
        self.snapped.clone()
    }
}

/// `divide` 的领域错误 → 编辑错误（全部是 Precondition：可以直接改意图重试）。
fn divide_edit_error(e: wv_terrain::divide::DivideError) -> EditError {
    use wv_terrain::divide::DivideError;
    match e {
        DivideError::TooFewPoints => {
            EditError::Precondition("意图多边形至少需要 3 个点".into())
        }
        DivideError::NotInsideHost => {
            EditError::Precondition("意图多边形不在宿主（或陆地）范围内".into())
        }
        DivideError::Fragmented { inside, remainder } => EditError::Precondition(format!(
            "这次划分会把领地切成多块（v1 不支持）：新势力 {inside} 块、余部 {remainder} 块；请分步划分"
        )),
    }
}

fn flat_to_pts(flat: &[f32]) -> Vec<(f32, f32)> {
    flat.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// 扁平点列的环（wu）。
type Ring = Vec<(f32, f32)>;

/// 疆界几何 →（外环 + 洞环），wu。非多边形 → `None`。
fn geometry_rings(geo: &GeometryData) -> Option<(Ring, Vec<Ring>)> {
    match geo {
        GeometryData::Polygon(v) => Some((flat_to_pts(v), Vec::new())),
        GeometryData::PolygonWithHoles { outer, holes } => Some((
            flat_to_pts(outer),
            holes.iter().map(|h| flat_to_pts(h)).collect(),
        )),
        GeometryData::Points(_) | GeometryData::Ref(_) => None,
    }
}

/// 划分产物（格坐标）→ 疆界几何（wu）。
fn rings_to_geometry(
    outers: &[Vec<(f32, f32)>],
    holes: &[Vec<Vec<(f32, f32)>>],
    cell_size: f32,
) -> GeometryData {
    let flat = |ring: &[(f32, f32)]| -> Vec<f32> {
        ring.iter()
            .flat_map(|&(x, y)| [x * cell_size, y * cell_size])
            .collect()
    };
    let outer = outers.first().map(|r| flat(r)).unwrap_or_default();
    let hole_rings: Vec<Vec<f32>> = holes
        .first()
        .map(|hs| hs.iter().map(|h| flat(h)).collect())
        .unwrap_or_default();
    if hole_rings.is_empty() {
        GeometryData::Polygon(outer)
    } else {
        GeometryData::PolygonWithHoles {
            outer,
            holes: hole_rings,
        }
    }
}

impl EditCommand for DivideFactionCommand {
    fn label(&self) -> String {
        match &self.host {
            Some(host) => format!("从「{host}」划分出「{}」", self.faction.name),
            None => format!("新建势力「{}」", self.faction.name),
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        use wv_core::time::TimePoint;

        if world.factions.iter().any(|f| f.id == self.faction.id) {
            return Err(EditError::Precondition(format!(
                "势力 `{}` 已存在",
                self.faction.id
            )));
        }
        if self.cell_size <= 0.0 || !self.cell_size.is_finite() {
            return Err(EditError::Precondition("格尺寸必须为正".into()));
        }
        let data = world.terrain_of(&self.map).ok_or_else(|| {
            EditError::Precondition(format!("地图 `{}` 没有地形栅格，无法划分势力", self.map))
        })?;
        let grid = to_grid(data)?;
        let intent_grid: Vec<(f32, f32)> = self
            .intent
            .iter()
            .map(|&(x, y)| (x / self.cell_size, y / self.cell_size))
            .collect();

        let (host_index, host_before, host_removed, remainder_index, new_geometry) = match &self
            .host
        {
            Some(host) => {
                if !world.factions.iter().any(|f| &f.id == host) {
                    return Err(EditError::Precondition(format!("宿主势力 `{host}` 不存在")));
                }
                let t = TimePoint::from_day(self.from);
                // 只认**实控** span（ADR-0027：同一 (map, owner, from) 可同时有
                // 声称与实控；划到声称上会留下实控未被切分、新势力重叠的烂摊子）。
                let idx = world
                    .faction_geometry
                    .iter()
                    .position(|g| {
                        g.map == self.map
                            && &g.owner == host
                            && g.relation.is_none()
                            && g.span.contains(&t)
                    })
                    .ok_or_else(|| {
                        EditError::Precondition(format!(
                            "宿主势力 `{host}` 在 {} 没有生效的实控疆界",
                            self.from
                        ))
                    })?;
                let before = world.faction_geometry[idx].clone();
                let (outer_wu, holes_wu) = geometry_rings(&before.geometry).ok_or_else(|| {
                    EditError::Precondition("宿主疆界不是多边形，无法划分".into())
                })?;
                // **单位换算（终审 C1）**：宿主几何是 wu，内核吃格坐标；
                // 意图在上方已换算，宿主这里同样必须除 cell_size，否则默认
                // 8 wu/格的工程里宿主会被当成「整张图」或直接落到图外。
                let to_cells = |ring: &[(f32, f32)]| -> Vec<(f32, f32)> {
                    ring.iter()
                        .map(|&(x, y)| (x / self.cell_size, y / self.cell_size))
                        .collect()
                };
                let outer = to_cells(&outer_wu);
                let holes: Vec<Vec<(f32, f32)>> = holes_wu.iter().map(|h| to_cells(h)).collect();
                let d = wv_terrain::divide::divide(
                    &grid,
                    &outer,
                    &holes,
                    &intent_grid,
                    self.mode.edge_mode(),
                    self.search_cells,
                )
                .map_err(divide_edit_error)?;
                if let Ok(mut flag) = self.snapped.lock() {
                    *flag = d.snapped;
                }
                let remainder_geometry =
                    rings_to_geometry(&d.remainder_outer, &d.remainder_holes, self.cell_size);
                let has_remainder = !d.remainder_outer.is_empty();
                // 历史切分（终审 I4 / ADR-0028 D6）：宿主 span 早于划分日开始时，
                // 把旧段截到 `from`、余部从 `from` 起新开一段——回拨时间轴仍能
                // 看到划分前的完整版图。
                let split_at_from = before.span.from < self.from
                    && before.span.to.map(|to| to > self.from).unwrap_or(true);
                let (host_index, host_before, host_removed, remainder_index) = if split_at_from {
                    let mut old = before.clone();
                    old.span.to = Some(self.from);
                    world.faction_geometry[idx] = old;
                    let mut rem_idx = None;
                    if has_remainder {
                        world.faction_geometry.push(GeometrySpan {
                            map: before.map.clone(),
                            owner: before.owner.clone(),
                            span: wv_core::time::TimeSpan {
                                from: self.from,
                                to: before.span.to,
                            },
                            geometry: remainder_geometry,
                            relation: before.relation.clone(),
                        });
                        rem_idx = Some(world.faction_geometry.len() - 1);
                    }
                    (Some(idx), Some(before), false, rem_idx)
                } else if has_remainder {
                    world.faction_geometry[idx] = GeometrySpan {
                        map: before.map.clone(),
                        owner: before.owner.clone(),
                        span: before.span,
                        geometry: remainder_geometry,
                        relation: before.relation.clone(),
                    };
                    (Some(idx), Some(before), false, None)
                } else {
                    // 意图吃光宿主：移除宿主 span（留空 Polygon 会让后续划分
                    // 报「意图不在宿主内」的误导错误）。
                    world.faction_geometry.remove(idx);
                    (Some(idx), Some(before), true, None)
                };
                let new_geometry = GeometrySpan {
                    map: self.map.clone(),
                    owner: self.faction.id.clone(),
                    span: wv_core::time::TimeSpan {
                        from: self.from,
                        to: None,
                    },
                    geometry: rings_to_geometry(&d.inside, &d.inside_holes, self.cell_size),
                    relation: None,
                };
                (
                    host_index,
                    host_before,
                    host_removed,
                    remainder_index,
                    new_geometry,
                )
            }
            None => {
                let d = wv_terrain::divide::divide_land_host(
                    &grid,
                    &intent_grid,
                    self.mode.edge_mode(),
                    self.search_cells,
                )
                .map_err(divide_edit_error)?;
                if let Ok(mut flag) = self.snapped.lock() {
                    *flag = d.snapped;
                }
                let new_geometry = GeometrySpan {
                    map: self.map.clone(),
                    owner: self.faction.id.clone(),
                    span: wv_core::time::TimeSpan {
                        from: self.from,
                        to: None,
                    },
                    geometry: rings_to_geometry(&d.inside, &d.inside_holes, self.cell_size),
                    relation: None,
                };
                (None, None, false, None, new_geometry)
            }
        };

        world.factions.push(self.faction.clone());
        world.faction_geometry.push(new_geometry);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = BeforeDivision {
            host_index,
            host_before,
            host_removed,
            remainder_index,
        };
        Ok(faction_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        world.factions.retain(|f| f.id != self.faction.id);
        world
            .faction_geometry
            .retain(|g| g.owner != self.faction.id);
        // 历史切分插入的余部段：按记录下标移除（它先于新势力几何 push，
        // 上面 retain 移除新几何不会移动它）。
        if let Some(i) = before.remainder_index {
            if i < world.faction_geometry.len() {
                world.faction_geometry.remove(i);
            }
        }
        if let (Some(idx), Some(prev)) = (before.host_index, before.host_before) {
            if before.host_removed {
                let at = idx.min(world.faction_geometry.len());
                world.faction_geometry.insert(at, prev);
            } else if idx < world.faction_geometry.len() {
                world.faction_geometry[idx] = prev;
            } else {
                return Err(EditError::Precondition("revert 时宿主疆界位置越界".into()));
            }
        }
        Ok(())
    }
}

/// 标记命令 apply 前的快照（undo 用）。
#[derive(Debug, Default)]
struct BeforeMarker {
    /// 被改动的格（生成分支才非空）。
    cells: Vec<(i32, i32, TerrainCell)>,
}

/// 放置一个生物群系标记（ADR-0029，命名段 M1 Task 3）。
///
/// 单事务完成三件事：地形写格（识别分支不写）、Place 入场（带 `biome` /
/// `envelope`）、生成边界 `GeometrySpan` 入场。`revert` 逐位还原三者。
///
/// 坐标为**图内局部 wu**；内核按 `cell_size` 换格（C1 教训：换算在桥层做）。
#[derive(Debug)]
pub struct PlaceBiomeMarkerCommand {
    pub map: wv_core::Id,
    pub cell_size: f32,
    /// 生效日（`Place` 的 span 与边界 span 同用）。
    pub day: i64,
    /// 待入场的 Place（`biome` 必须为 Some；`envelope` 由命令按 wu 写入）。
    pub place: wv_core::distance::Place,
    /// 意图包络（图内局部 wu，首尾不重复）。
    pub envelope: Vec<(f32, f32)>,
    before: std::sync::Mutex<BeforeMarker>,
}

impl PlaceBiomeMarkerCommand {
    pub fn new(
        map: wv_core::Id,
        cell_size: f32,
        day: i64,
        place: wv_core::distance::Place,
        envelope: Vec<(f32, f32)>,
    ) -> Self {
        Self {
            map,
            cell_size,
            day,
            place,
            envelope,
            before: std::sync::Mutex::new(BeforeMarker::default()),
        }
    }
}

fn marker_error(e: wv_terrain::marker::MarkerError) -> EditError {
    use wv_terrain::marker::MarkerError;
    match e {
        MarkerError::TooFewPoints => EditError::Precondition("标记包络至少需要 3 个点".into()),
        MarkerError::EmptyRegion => {
            EditError::Precondition("标记范围内没有可用的地形格（可能全是水或包络太小）".into())
        }
    }
}

/// 环（格坐标）→ 扁平 wu 环（`[x0, y0, x1, y1, …]`）。
fn ring_wu(ring: &[(f32, f32)], cell_size: f32) -> Vec<f32> {
    ring.iter()
        .flat_map(|&(x, y)| [x * cell_size, y * cell_size])
        .collect()
}

/// 标记区域 → 地界几何（M2：有洞存 `PolygonWithHoles`，无洞单环）。
///
/// 锚点拟合只看外环（`fit_anchor_to_ring`），洞不参与质心/包围盒——与
/// 渲染侧「晕按外环铺、符号避开洞」同一取舍。创建与重生成两命令共用，
/// 保证两条路径的几何形态永不漂移。
fn marker_geometry(region: &wv_terrain::marker::MarkerRegion, cell_size: f32) -> GeometryData {
    let outer = ring_wu(&region.outer, cell_size);
    if region.holes.is_empty() {
        GeometryData::Polygon(outer)
    } else {
        GeometryData::PolygonWithHoles {
            holes: region.holes.iter().map(|h| ring_wu(h, cell_size)).collect(),
            outer,
        }
    }
}

impl EditCommand for PlaceBiomeMarkerCommand {
    fn label(&self) -> String {
        format!("标记「{}」", self.place.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let biome = self
            .place
            .biome
            .ok_or_else(|| EditError::Precondition("标记缺少生物群系类型（biome）".into()))?;
        if world.distances.places.iter().any(|p| p.id == self.place.id) {
            return Err(EditError::Precondition(format!(
                "地点 `{}` 已存在",
                self.place.id
            )));
        }
        if self.cell_size <= 0.0 || !self.cell_size.is_finite() {
            return Err(EditError::Precondition("格尺寸必须为正".into()));
        }
        let data = world.terrain_of(&self.map).ok_or_else(|| {
            EditError::Precondition(format!("地图 `{}` 没有地形栅格，无法放置标记", self.map))
        })?;
        let mut grid = to_grid(data)?;
        let snapshot = grid.cells().to_vec();
        let region =
            wv_terrain::marker::mark_region(&mut grid, &self.envelope, biome, self.cell_size)
                .map_err(marker_error)?;

        // 生成分支才写回地形；识别分支 grid 未被改动。
        let mut changed: Vec<(i32, i32, TerrainCell)> = Vec::new();
        if region.outcome == wv_terrain::marker::MarkerOutcome::Generated {
            for (i, after) in grid.cells().iter().enumerate() {
                if *after != snapshot[i] {
                    let (x, y) = (
                        (i % grid.width() as usize) as i32,
                        (i / grid.width() as usize) as i32,
                    );
                    changed.push((x, y, snapshot[i]));
                }
            }
            world.set_terrain(self.map.clone(), from_grid(grid));
        }

        // Place：写 biome / envelope；锚点按生成边界外环拟合（洞不参与）。
        let mut place = self.place.clone();
        place.map = Some(self.map.clone());
        place.biome = Some(biome);
        place.envelope = Some(self.envelope.iter().flat_map(|&(x, y)| [x, y]).collect());
        place.fit_anchor_to_ring(&ring_wu(&region.outer, self.cell_size));
        let geometry = marker_geometry(&region, self.cell_size);

        world.distances.places.push(place);
        world.place_geometry.push(GeometrySpan {
            map: self.map.clone(),
            owner: self.place.id.clone(),
            span: wv_core::time::TimeSpan {
                from: self.day,
                to: None,
            },
            geometry,
            relation: None,
        });
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? =
            BeforeMarker { cells: changed };

        let mut dirty = place_geometry_dirty_paths();
        dirty.push(format!("maps/{}/terrain.bin", self.map));
        Ok(dirty)
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .cells
            .clone();
        world.distances.places.retain(|p| p.id != self.place.id);
        world
            .place_geometry
            .retain(|g| !(g.map == self.map && g.owner == self.place.id));
        if !before.is_empty() {
            let data = world
                .terrain_of(&self.map)
                .cloned()
                .ok_or_else(|| EditError::Precondition("revert 时地图地形缺失".into()))?;
            let mut grid = to_grid(&data)?;
            for (x, y, cell) in before {
                grid.set(x, y, cell);
            }
            world.set_terrain(self.map.clone(), from_grid(grid));
        }
        Ok(())
    }
}

// 把 `Color` / `GeometryData` 隐式导入给测试用——避免测试模块再加 use。
#[allow(unused_imports)]
use wv_core::{Color as _Color, Faction as _Faction};

/// Place 锚点四元组（x, y, w, h；各自可缺省）。
type Anchor = (Option<f32>, Option<f32>, Option<f32>, Option<f32>);

/// 重生成命令 apply 前的快照（undo 用）。
#[derive(Debug, Default, Clone)]
struct BeforeRegen {
    /// 生成分支被改动的格（旧值）；识别分支恒空。
    cells: Vec<(i32, i32, TerrainCell)>,
    /// 覆盖前的锚点四元组；`None` = 未 apply（revert 须为无操作）。
    anchor: Option<Anchor>,
    /// 被替换掉的旧 span（原索引，升序记录）。
    spans: Vec<(usize, GeometrySpan)>,
}

/// 重生成一个生物群系标记（ADR-0029 D2「重圈重生成」的 M2 落地）。
///
/// 以 Place **已存的** `envelope` + `biome` 为唯一事实源重跑识别/生成，
/// 替换边界 span 与锚点。单事务原子，`revert` 逐位还原格子 / 锚点 / span。
///
/// ## 为什么不带 cell_size / envelope 参数
///
/// 格尺寸由 [`marker_cell_size`] 从目标图 raster **在 apply 内**推导
/// （ADR-0029 终审 C1：捕获时与会话态 `GridConfig` 漂移的教训——干脆不给
/// 调用方传错的机会）；包络读已存值，重生成语义就是「按原意图重算」。
///
/// ## span 语义 = 替换（R2，2026-10-18 拍板）
///
/// 删除 owner 的全部旧 span、写入一条 `[day, None)`。边界是「生成快照」
/// （ADR-0029 D2）而非领域状态区间：地形格本身不分区间版本化，保留旧 span
/// 会在时间轴回拨时出现「旧边界盖新地形」的错位，故替换而非追加历史。
#[derive(Debug)]
pub struct RegenerateBiomeMarkerCommand {
    /// 目标标记（Place）的 ID。
    pub place_id: wv_core::Id,
    /// 新 span 的生效日（调用方取当前时间轴日）。
    pub day: i64,
    before: std::sync::Mutex<BeforeRegen>,
}

impl RegenerateBiomeMarkerCommand {
    pub fn new(place_id: wv_core::Id, day: i64) -> Self {
        Self {
            place_id,
            day,
            before: std::sync::Mutex::new(BeforeRegen::default()),
        }
    }
}

/// 扁平包络 `[x0, y0, x1, y1, …]` → 点列；奇数长度 / 少于 3 点返回 `None`。
fn envelope_points(flat: &[f32]) -> Option<Vec<(f32, f32)>> {
    if flat.len() < 6 || !flat.len().is_multiple_of(2) {
        return None;
    }
    Some(flat.chunks_exact(2).map(|c| (c[0], c[1])).collect())
}

impl EditCommand for RegenerateBiomeMarkerCommand {
    fn label(&self) -> String {
        format!("重生成标记「{}」", self.place_id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let idx = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place_id)
            .ok_or_else(|| EditError::TargetMissing(format!("地点 `{}` 不存在", self.place_id)))?;
        let biome = world.distances.places[idx]
            .biome
            .ok_or_else(|| EditError::Precondition("该地点没有生物群系类型，不是标记".into()))?;
        let envelope_flat = world.distances.places[idx]
            .envelope
            .clone()
            .ok_or_else(|| {
                EditError::Precondition("标记缺少意图包络（envelope），无法重生成".into())
            })?;
        let envelope = envelope_points(&envelope_flat).ok_or_else(|| {
            EditError::Precondition("意图包络格式非法（少于 3 点或坐标不成对）".into())
        })?;
        let map = world.distances.places[idx]
            .map
            .clone()
            .ok_or_else(|| EditError::Precondition("标记未锚定地图".into()))?;
        let cell_size = marker_cell_size(world, &map).ok_or_else(|| {
            EditError::Precondition(format!("地图 `{map}` 没有地形栅格，无法重生成标记"))
        })?;
        let data = world
            .terrain_of(&map)
            .cloned()
            .ok_or_else(|| EditError::Precondition("地图地形在推导格尺寸后消失".into()))?;
        let mut grid = to_grid(&data)?;
        let snapshot = grid.cells().to_vec();
        let region = wv_terrain::marker::mark_region(&mut grid, &envelope, biome, cell_size)
            .map_err(marker_error)?;

        // 生成分支才写回地形；识别分支 grid 未被改动（与创建命令同构）。
        let mut changed: Vec<(i32, i32, TerrainCell)> = Vec::new();
        if region.outcome == wv_terrain::marker::MarkerOutcome::Generated {
            for (i, after) in grid.cells().iter().enumerate() {
                if *after != snapshot[i] {
                    let (x, y) = (
                        (i % grid.width() as usize) as i32,
                        (i / grid.width() as usize) as i32,
                    );
                    changed.push((x, y, snapshot[i]));
                }
            }
            world.set_terrain(map.clone(), from_grid(grid));
        }

        // R2 替换：记录并移除 owner 全部旧 span，新 span 落在首个旧位（无旧位则追加）。
        let spans: Vec<(usize, GeometrySpan)> = world
            .place_geometry
            .iter()
            .enumerate()
            .filter(|(_, g)| g.map == map && g.owner == self.place_id)
            .map(|(i, g)| (i, g.clone()))
            .collect();
        let insert_at = spans
            .first()
            .map(|&(i, _)| i)
            .unwrap_or(world.place_geometry.len());
        world
            .place_geometry
            .retain(|g| !(g.map == map && g.owner == self.place_id));
        world.place_geometry.insert(
            insert_at,
            GeometrySpan {
                map: map.clone(),
                owner: self.place_id.clone(),
                span: wv_core::time::TimeSpan {
                    from: self.day,
                    to: None,
                },
                geometry: marker_geometry(&region, cell_size),
                relation: None,
            },
        );

        // 锚点按新外环重拟合（洞不参与）。
        let place = &mut world.distances.places[idx];
        let anchor = (place.x, place.y, place.w, place.h);
        place.fit_anchor_to_ring(&ring_wu(&region.outer, cell_size));

        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = BeforeRegen {
            cells: changed,
            anchor: Some(anchor),
            spans,
        };

        let mut dirty = place_geometry_dirty_paths();
        if region.outcome == wv_terrain::marker::MarkerOutcome::Generated {
            dirty.push(format!("maps/{}/terrain.bin", map));
        }
        Ok(dirty)
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = (*self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?)
        .clone();
        // 未 apply 过的实例（anchor == None）必须是无操作——不抹锚点、不动 span。
        let Some(anchor) = before.anchor else {
            return Ok(());
        };
        // 先移除新 span（apply 后 owner 名下只有它），再按原索引升序回插旧 span。
        let map = before
            .spans
            .first()
            .map(|(_, g)| g.map.clone())
            .or_else(|| {
                world
                    .distances
                    .places
                    .iter()
                    .find(|p| p.id == self.place_id)
                    .and_then(|p| p.map.clone())
            });
        if let Some(map) = &map {
            world
                .place_geometry
                .retain(|g| !(g.map == *map && g.owner == self.place_id));
        }
        for (i, g) in before.spans {
            let at = i.min(world.place_geometry.len());
            world.place_geometry.insert(at, g);
        }
        if let Some(place) = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.place_id)
        {
            place.x = anchor.0;
            place.y = anchor.1;
            place.w = anchor.2;
            place.h = anchor.3;
        }
        if !before.cells.is_empty() {
            let map = map.ok_or_else(|| {
                EditError::Precondition("revert 时既无旧 span 也无地点锚图，无法还原地形".into())
            })?;
            let data = world
                .terrain_of(&map)
                .cloned()
                .ok_or_else(|| EditError::Precondition("revert 时地图地形缺失".into()))?;
            let mut grid = to_grid(&data)?;
            for (x, y, cell) in before.cells {
                grid.set(x, y, cell);
            }
            world.set_terrain(map.clone(), from_grid(grid));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 街道编辑命令（ADR-0010 待做第 3 项，2026-10-13）
// ---------------------------------------------------------------------------

/// 追加一段手绘街道（虚构路网的供给入口，ADR-0010 决策 ②）。
///
/// 目标图层未加载时**静默新建空文档**再追加——不报错、不追问：街道文档在
/// `World.streets` 里的首次出现往往就是作者画下的第一笔（V1：校验只提示，
/// 编辑不设阻力）；磁盘上对应 `maps/<map>/<source>` 的 YAML，由 `wv-io` 在
/// 落盘时按虚拟脏路径 `streets/<layer_id>` 聚合写出。
#[derive(Debug)]
pub struct AddStreetSegmentCommand {
    /// 目标街道图层 ID（`World.streets` 的键）。
    pub layer: wv_core::Id,
    /// 要追加的段（折线 + 类别，本图局部坐标）。
    pub segment: wv_core::StreetSegment,
    /// `apply` 时缓存的追加前段数：revert 时把文档截回这个长度。
    /// 用 `Mutex` 而非 `RefCell`，使命令可跨线程（与 [`PaintTerrainCommand`] 同款）。
    before_len: std::sync::Mutex<Option<usize>>,
}

impl AddStreetSegmentCommand {
    pub fn new(layer: wv_core::Id, segment: wv_core::StreetSegment) -> Self {
        Self {
            layer,
            segment,
            before_len: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for AddStreetSegmentCommand {
    fn label(&self) -> String {
        format!("画街道段（{:?}）", self.segment.class)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        let before_len = doc.len();
        doc.push(self.segment.clone());
        world.set_streets(self.layer.clone(), doc);
        *(self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(before_len);
        Ok(vec![format!("streets/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before_len = self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .ok_or_else(|| EditError::Precondition("AddStreetSegment 尚未 apply".into()))?;
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        doc.segments.truncate(before_len);
        world.set_streets(self.layer.clone(), doc);
        Ok(())
    }
}

/// 删除一段手绘街道（按索引）。
///
/// 与 [`AddStreetSegmentCommand`] 对偶：apply 缓存被删段，revert 把它插回
/// 原索引。索引越界报 `Precondition`——UI 侧应在点击前用当前段数判断
/// 可删性（按钮置灰），命令层保留兜底。
#[derive(Debug)]
pub struct DeleteStreetSegmentCommand {
    /// 目标街道图层 ID（`World.streets` 的键）。
    pub layer: wv_core::Id,
    /// 要删除的段索引（`segments` 下标）。
    pub index: usize,
    /// `apply` 时缓存的被删段：revert 时插回 `index`。
    before: std::sync::Mutex<Option<wv_core::StreetSegment>>,
}

impl DeleteStreetSegmentCommand {
    pub fn new(layer: wv_core::Id, index: usize) -> Self {
        Self {
            layer,
            index,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for DeleteStreetSegmentCommand {
    fn label(&self) -> String {
        format!("删除街道段 #{}", self.index)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        if self.index >= doc.len() {
            return Err(EditError::Precondition(format!(
                "街道段索引 {} 越界（图层 `{}` 共 {} 段）",
                self.index,
                self.layer.as_str(),
                doc.len()
            )));
        }
        let removed = doc.segments.remove(self.index);
        world.set_streets(self.layer.clone(), doc);
        *(self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(removed);
        Ok(vec![format!("streets/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let removed = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("DeleteStreetSegment 尚未 apply".into()))?;
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        let index = self.index.min(doc.segments.len());
        doc.segments.insert(index, removed);
        world.set_streets(self.layer.clone(), doc);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 地点新增（ADR-0014 v0.1 骨架：HTTP skill `create_place` 的命令入口）
// ---------------------------------------------------------------------------

/// 新增一个 `Place`（距离图节点 + 地图上可选锚点）。
///
/// `Place` 的存储归属在 `World.distances.places`（`wv_core::distance::Place`），
/// 它是 §4.5「每实体一文件」下的一个实体——脏路径 `distances/places`，由
/// `wv-io::save_world` 触发该目录的全量重写 + 删除传播。
///
/// ## 幂等保护
///
/// 与 `AddMapCommand` 同源：目标 ID 已存在时 `apply` 报错，不静默覆盖——
/// Place 是引用网络中的关键节点，悄无声息覆盖会让 `PlaceEdge` / `Unit` 的
/// 起点终点成为悬空引用，校验面板得花更多轮才找得到原因。
#[derive(Debug)]
pub struct AddPlaceCommand {
    pub place: wv_core::distance::Place,
}

impl AddPlaceCommand {
    pub fn new(place: wv_core::distance::Place) -> Self {
        Self { place }
    }
}

impl EditCommand for AddPlaceCommand {
    fn label(&self) -> String {
        format!("新增地点「{}」", self.place.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.distances.places.iter().any(|p| p.id == self.place.id) {
            return Err(EditError::Precondition(format!(
                "地点 `{}` 已存在",
                self.place.id
            )));
        }
        world.distances.places.push(self.place.clone());
        Ok(vec!["distances/places".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
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

/// 新增一条 `PlaceEdge`（距离图的边；`distances/edges/` 落盘）。
///
/// 与 [`AddPlaceCommand`] 同源：目标 ID 已存在时 `apply` 报错，不静默覆盖——
/// `PlaceEdge` 起点终点的 `Place` 引用是关键网络节点，悄无声覆盖会让
/// `DistanceGraph` 的拓扑在不可见处变化。
///
/// 悬空引用（`a` / `b` 指向不存在的 `Place`）按 AGENTS §7.5 V1 规则**不阻止
/// 保存**：作者可以先放一两个 Place 再回头补边，作者主人在工具之上。
#[derive(Debug)]
pub struct AddPlaceEdgeCommand {
    pub edge: wv_core::distance::PlaceEdge,
}

impl AddPlaceEdgeCommand {
    pub fn new(edge: wv_core::distance::PlaceEdge) -> Self {
        Self { edge }
    }
}

impl EditCommand for AddPlaceEdgeCommand {
    fn label(&self) -> String {
        format!("新增边「{}」", self.edge.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.distances.edges.iter().any(|e| e.id == self.edge.id) {
            return Err(EditError::Precondition(format!(
                "边 `{}` 已存在",
                self.edge.id
            )));
        }
        world.distances.edges.push(self.edge.clone());
        Ok(vec!["distances/edges".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .distances
            .edges
            .iter()
            .position(|e| e.id == self.edge.id)
            .ok_or_else(|| EditError::TargetMissing(format!("PlaceEdge {}", self.edge.id)))?;
        world.distances.edges.remove(pos);
        Ok(())
    }
}

// 建筑编辑命令（ADR-0016 §6 落地项，2026-10-14）
// ---------------------------------------------------------------------------

/// 追加一栋建筑（2.5D 建筑挤出的数据入口）。
///
/// 语义与 [`AddStreetSegmentCommand`] 完全同构：目标图层未加载时**静默新建
/// 空文档**再追加（V1：作者画下的第一笔就是文档的诞生，不报错不追问）；
/// 落盘走虚拟脏路径 `buildings/<layer_id>`（`wv-io` 聚合写出）。
#[derive(Debug)]
pub struct AddBuildingCommand {
    /// 目标建筑图层 ID（`World.buildings` 的键）。
    pub layer: wv_core::Id,
    /// 要追加的建筑（轮廓 + 高度，本图局部坐标）。
    pub building: wv_core::Building,
    /// `apply` 时缓存的追加前栋数：revert 时把文档截回这个长度。
    before_len: std::sync::Mutex<Option<usize>>,
}

impl AddBuildingCommand {
    pub fn new(layer: wv_core::Id, building: wv_core::Building) -> Self {
        Self {
            layer,
            building,
            before_len: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for AddBuildingCommand {
    fn label(&self) -> String {
        format!("画建筑（{:.0} 米）", self.building.height_m)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        let before_len = doc.len();
        doc.push(self.building.clone());
        world.set_buildings(self.layer.clone(), doc);
        *(self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(before_len);
        Ok(vec![format!("buildings/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before_len = self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .ok_or_else(|| EditError::Precondition("AddBuilding 尚未 apply".into()))?;
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        doc.buildings.truncate(before_len);
        world.set_buildings(self.layer.clone(), doc);
        Ok(())
    }
}

/// 删除一栋建筑（按索引）。
///
/// 与 [`DeleteStreetSegmentCommand`] 对偶：apply 缓存被删栋，revert 插回原
/// 索引；越界报 `Precondition`（UI 侧点击前置灰，命令层兜底）。
#[derive(Debug)]
pub struct DeleteBuildingCommand {
    /// 目标建筑图层 ID（`World.buildings` 的键）。
    pub layer: wv_core::Id,
    /// 要删除的建筑索引（`buildings` 下标）。
    pub index: usize,
    /// `apply` 时缓存的被删栋：revert 时插回 `index`。
    before: std::sync::Mutex<Option<wv_core::Building>>,
}

impl DeleteBuildingCommand {
    pub fn new(layer: wv_core::Id, index: usize) -> Self {
        Self {
            layer,
            index,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for DeleteBuildingCommand {
    fn label(&self) -> String {
        format!("删除建筑 #{}", self.index)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        if self.index >= doc.len() {
            return Err(EditError::Precondition(format!(
                "建筑索引 {} 越界（图层 `{}` 共 {} 栋）",
                self.index,
                self.layer.as_str(),
                doc.len()
            )));
        }
        let removed = doc.buildings.remove(self.index);
        world.set_buildings(self.layer.clone(), doc);
        *(self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(removed);
        Ok(vec![format!("buildings/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let removed = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("DeleteBuilding 尚未 apply".into()))?;
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        let index = self.index.min(doc.buildings.len());
        doc.buildings.insert(index, removed);
        world.set_buildings(self.layer.clone(), doc);
        Ok(())
    }
}

/// 设置地图朝向（屏幕旋转角，单位：度）。
///
/// `apply` 缓存旧值；`revert` 用缓存恢复。`0.0` = 正北朝上；
/// 正值 = 顺时针偏转。命令层不校验范围——UI 侧应把值夹到 `[0, 360)`。
///
/// 脏路径：`maps/{id}/index.yaml`（地图节点落盘文件）。
#[derive(Debug)]
pub struct SetMapOrientationCommand {
    pub map_id: wv_core::Id,
    pub orientation: f32,
    before: std::sync::Mutex<Option<f32>>,
}

impl SetMapOrientationCommand {
    pub fn new(map_id: wv_core::Id, orientation: f32) -> Self {
        Self {
            map_id,
            orientation,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetMapOrientationCommand {
    fn label(&self) -> String {
        format!("改地图「{}」朝向为 {:.1}°", self.map_id, self.orientation)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.map_id)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map_id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = Some(node.orientation);
        node.orientation = self.orientation;
        Ok(vec![format!("maps/{}/index.yaml", self.map_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("SetMapOrientation 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.map_id)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map_id)))?;
        node.orientation = prev;
        Ok(())
    }
}

/// 重置当前地形到底图（2026-10-16 用户需求）。
///
/// 把整张图的 `TerrainData.cells` 替换为 [`wv_core::map::MapBase::cell()`]
/// 的复制；`base` 为 `None` 的旧地图按 [`MapBase::Ocean`] 兜底——
/// 向后兼容不锁住「重置」入口。
///
/// ## 与 P6（非破坏编辑）的关系
///
/// `apply` 把**整张** `TerrainData` 缓存进 `before`，`revert` 原样恢复——
/// 一次性写满 N² 格（512² ≈ 786 KB）就是这一命令的「diff」体积。
/// 无内存里别处再持有地形 cells 的可变引用，克隆一份独立即可。
///
/// ## 脏路径
///
/// `maps/{id}/terrain.bin`（与 `PaintTerrainCommand` / `IntentTerrainCommand`
/// 同源）。`index.yaml` **不**动（`base` 字段没变）。
///
/// ## 失败语义
///
/// - 地图不存在：`TargetMissing`
/// - 该图无地形数据（如尚未生成栅格的子图）：`Precondition`，
///   提示「地图「X」无地形数据可重置」——子图继承父图底图时常见。
#[derive(Debug)]
pub struct ResetTerrainToBaseCommand {
    pub map: wv_core::Id,
    before: std::sync::Mutex<Option<wv_core::terrain::TerrainData>>,
}

impl ResetTerrainToBaseCommand {
    pub fn new(map: wv_core::Id) -> Self {
        Self {
            map,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for ResetTerrainToBaseCommand {
    fn label(&self) -> String {
        format!("重置地形「{}」到底图", self.map)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 1. 节点 → 决定底图（None 兜底 Ocean，向后兼容旧世界）
        let base = world
            .maps
            .iter()
            .find(|m| m.id == self.map)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map)))?
            .base
            .unwrap_or(wv_core::map::MapBase::Ocean);
        // 2. 备份当前地形（含尺寸，决定替换后的 cells 数量）
        let prev = world.terrain_of(&self.map).cloned().ok_or_else(|| {
            EditError::Precondition(format!("地图「{}」无地形数据可重置", self.map))
        })?;
        let width = prev.width;
        let height = prev.height;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = Some(prev);
        // 3. 整片替换
        let new = wv_core::terrain::TerrainData {
            width,
            height,
            cells: vec![base.cell(); (width as usize) * (height as usize)],
        };
        world.set_terrain(self.map.clone(), new);
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("ResetTerrainToBase 尚未 apply".into()))?;
        world.set_terrain(self.map.clone(), prev);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测量编辑命令（工具产物，每图一份，2026-10-14）
// ---------------------------------------------------------------------------

use wv_core::Measurement;
use wv_core::MeasurementOverlay;

/// 受影响的脏路径：测量走虚拟命名空间 `measurements/<map_id>`——避免
/// `maps/<map_id>/` 前缀把同图其它文件（`index.yaml` / `terrain.bin`）误判脏。
/// 与街道 `streets/<layer_id>` 同款越界防护（见 ADR-0010）。
fn measurement_dirty_path(map: &wv_core::Id) -> Vec<String> {
    vec![format!("measurements/{}", map.as_str())]
}

/// 新增一条测量。
///
/// `apply` 在 `world.measurements[map]` 末尾追加；`revert` 按 ID 移除。
///
/// **为什么按 ID 而非索引定位**：测量是用户手画的对象，**ID 是显式声明**
/// 的稳定键，撤销按 ID 还原比按索引更安全（索引会随增删漂移）。
///
/// **顶替语义**：ID 已存在时 `apply` 报 `Precondition`，**不**静默覆盖
/// （命令层「不擅自做作者的决定」）。
#[derive(Debug)]
pub struct AddMeasurementCommand {
    pub map: wv_core::Id,
    pub measurement: Measurement,
}

impl AddMeasurementCommand {
    pub fn new(map: wv_core::Id, measurement: Measurement) -> Self {
        Self { map, measurement }
    }
}

impl EditCommand for AddMeasurementCommand {
    fn label(&self) -> String {
        format!("新增测量「{}」", self.measurement.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let overlay = world.measurement_of_mut(&self.map);
        if overlay.add(self.measurement.clone()).is_err() {
            return Err(EditError::Precondition(format!(
                "测量 `{}` 已存在",
                self.measurement.id
            )));
        }
        Ok(measurement_dirty_path(&self.map))
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let overlay = world.measurement_of_mut(&self.map);
        if overlay.remove(&self.measurement.id).is_none() {
            return Err(EditError::Precondition(
                "AddMeasurement revert 找不到对应记录".into(),
            ));
        }
        Ok(())
    }
}

/// 删除一条测量（按 ID）。
///
/// `apply` 缓存被删的 measurement（位置 + 内容）；`revert` 用缓存恢复。
#[derive(Debug)]
pub struct RemoveMeasurementCommand {
    pub map: wv_core::Id,
    pub id: wv_core::Id,
    before: std::sync::Mutex<Option<(usize, Measurement)>>,
}

impl RemoveMeasurementCommand {
    pub fn new(map: wv_core::Id, id: wv_core::Id) -> Self {
        Self {
            map,
            id,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveMeasurementCommand {
    fn label(&self) -> String {
        format!("删除测量「{}」", self.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let overlay = world.measurement_of_mut(&self.map);
        let pos = overlay
            .measurements
            .iter()
            .position(|m| m.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Measurement {}", self.id)))?;
        let prev = overlay.measurements.remove(pos);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, prev));
        Ok(measurement_dirty_path(&self.map))
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, prev) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveMeasurement 尚未 apply".into()))?;
        let overlay = world.measurement_of_mut(&self.map);
        let insert_at = pos.min(overlay.measurements.len());
        overlay.measurements.insert(insert_at, prev);
        Ok(())
    }
}

/// 清空某张图的全部测量。
///
/// `apply` 缓存整份旧 overlay（可能为空）；`revert` 整份还原。
/// **仅在该图原本非空时**才记脏路径——空→空不写盘（V1：作者没动就别替他造）。
#[derive(Debug)]
pub struct ClearMeasurementsCommand {
    pub map: wv_core::Id,
    before: std::sync::Mutex<Option<MeasurementOverlay>>,
}

impl ClearMeasurementsCommand {
    pub fn new(map: wv_core::Id) -> Self {
        Self {
            map,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for ClearMeasurementsCommand {
    fn label(&self) -> String {
        format!("清空「{}」的全部测量", self.map)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let existing = world.measurement_of(&self.map).cloned();
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = existing.clone();
        if let Some(doc) = existing {
            if doc.is_empty() {
                return Ok(Vec::new());
            }
            world.set_measurement(self.map.clone(), MeasurementOverlay::new());
            return Ok(measurement_dirty_path(&self.map));
        }
        Ok(Vec::new())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        // 缓存 `None` = `apply` 时该图无 overlay（连键都没有），
        // revert 时也不该留下键。
        // 缓存 `Some(empty_overlay)` = `apply` 时键存在但清单空，
        // revert 还原成空文档。
        // 缓存 `Some(doc)` 且非空 = `apply` 实际清空了一份非空 overlay，
        // revert 整份还原。
        match prev {
            Some(doc) if !doc.is_empty() => {
                world.set_measurement(self.map.clone(), doc);
            }
            Some(_) => {
                world.set_measurement(self.map.clone(), MeasurementOverlay::new());
            }
            None => {
                world.measurements.remove(&self.map);
            }
        }
        Ok(())
    }
}

// ── Region 命令（ADR-0017 §2.2） ─────────────────────────────────────────────

use wv_core::Region;

fn region_dirty_paths() -> Vec<String> {
    vec!["regions".into()]
}

/// 新增一个命名区域。
///
/// `apply` 在 `world.regions` 末尾追加；`revert` 移除（按 ID 定位）。
#[derive(Debug)]
pub struct AddRegionCommand {
    pub region: Region,
}

impl AddRegionCommand {
    pub fn new(region: Region) -> Self {
        Self { region }
    }
}

impl EditCommand for AddRegionCommand {
    fn label(&self) -> String {
        format!("新增区域「{}」", self.region.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.regions.iter().any(|r| r.id == self.region.id) {
            return Err(EditError::Precondition(format!(
                "区域 `{}` 已存在",
                self.region.id
            )));
        }
        world.regions.push(self.region.clone());
        Ok(region_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .regions
            .iter()
            .position(|r| r.id == self.region.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.region.id)))?;
        world.regions.remove(pos);
        Ok(())
    }
}

/// 删除一个命名区域。
///
/// `apply` 缓存被删的 Region（用于 revert 还原）；`revert` 按原位恢复。
#[derive(Debug)]
pub struct RemoveRegionCommand {
    pub id: wv_core::Id,
    before: std::sync::Mutex<Option<(usize, Region)>>,
}

impl RemoveRegionCommand {
    pub fn new(id: wv_core::Id) -> Self {
        Self {
            id,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveRegionCommand {
    fn label(&self) -> String {
        format!("删除区域「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .regions
            .iter()
            .position(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        let prev = world.regions.remove(pos);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, prev));
        Ok(region_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, prev) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveRegion 尚未 apply".into()))?;
        let insert_at = pos.min(world.regions.len());
        world.regions.insert(insert_at, prev);
        Ok(())
    }
}

/// 更新区域名称 / 轮廓。
///
/// `apply` 缓存旧 Region 全字段；`revert` 用缓存恢复。
#[derive(Debug)]
pub struct UpdateRegionCommand {
    pub id: wv_core::Id,
    pub new_name: String,
    pub new_polygon: Vec<f32>,
    before: std::sync::Mutex<Option<Region>>,
}

impl UpdateRegionCommand {
    pub fn new(id: wv_core::Id, new_name: String, new_polygon: Vec<f32>) -> Self {
        Self {
            id,
            new_name,
            new_polygon,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateRegionCommand {
    fn label(&self) -> String {
        format!("改区域「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(r.clone());
        r.name = self.new_name.clone();
        r.polygon = self.new_polygon.clone();
        Ok(region_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateRegion 尚未 apply".into()))?;
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *r = prev;
        Ok(())
    }
}

// ── 事实地点与地界命令（theme overlay） ─────────────────────────────────────

use wv_core::distance::Place;

/// 事实地点创建命令的脏路径：`Place` 本体与地界几何分属两个落盘目录。
///
/// `distances/places` 是 §4.5「每实体一文件」的地点表（全量重写 + 删除传播），
/// `geometry/place` 是地界 span 目录（`wv-io::save_world` 按目录写出）。
fn place_geometry_dirty_paths() -> Vec<String> {
    vec!["distances/places".into(), "geometry/place".into()]
}

/// 从 World 数据推导目标图的格尺寸（wu/格）——标记命令的唯一格口径。
///
/// 逐条镜像渲染层 `grid_sync` 的推导（图幅 extent / 栅格宽），但**纯从
/// World 数据算**：wv-edit 结构上摸不到会话态 `GridConfig`，「捕获时与提交时
/// 格口径漂移」的 C1 类 bug 在这里无法发生。回退链与 `grid_sync` 一致：
/// 本图 extent → 根图 extent → 4096；无地形栅格返回 `None`（调用方拒绝）。
fn marker_cell_size(world: &World, map: &wv_core::Id) -> Option<f32> {
    let data = world.terrain_of(map)?;
    let extent = world
        .maps
        .iter()
        .find(|m| m.id == *map)
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

/// 创建一个「事实地点」并同时写入它的初始地界。
///
/// ## 为什么地点与地界要一条命令
///
/// 地理事实（森林 / 沼泽 / 山脉）不是先有点、再补范围：作者画下一个地界时，
/// 地点与几何是一体诞生的。拆成两条命令会让 Undo 出现「地点还在、地界没了」
/// 的中间态（或反之），而 P6 要求一次编辑可整体回退。
///
/// ## 锚点同步
///
/// `Place.x/y/w/h` 是既有读取方（POI、标签远档判定、`EventLocation::resolve`）
/// 的投影字段，地界多边形才是几何真值。`apply` 在入库前按地界环重拟合锚点，
/// 使新旧两套字段一出生就一致（与 ADR-0022 D2「双字段同步」同口径）。
///
/// ## 幂等保护
///
/// 与 [`AddPlaceCommand`] 同源：ID 冲突时报 `Precondition`，不静默覆盖——
/// Place 是引用网络的关键节点，静默覆盖会让 `PlaceEdge` / `Unit` 的端点变成
/// 悬空引用（V1 会列出，但命令层不应主动制造）。
#[derive(Debug)]
pub struct AddFeaturePlaceCommand {
    /// 要创建的地点（`feature` 为事实类型编号）。
    pub place: Place,
    /// 该地点的初始地界（`owner` 应为 `place.id`，几何为多边形环）。
    pub territory: GeometrySpan,
}

impl AddFeaturePlaceCommand {
    pub fn new(place: Place, territory: GeometrySpan) -> Self {
        Self { place, territory }
    }
}

impl EditCommand for AddFeaturePlaceCommand {
    fn label(&self) -> String {
        format!("新增事实地点「{}」", self.place.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.distances.places.iter().any(|p| p.id == self.place.id) {
            return Err(EditError::Precondition(format!(
                "地点 `{}` 已存在",
                self.place.id
            )));
        }
        let mut place = self.place.clone();
        if let GeometryData::Polygon(ring) = &self.territory.geometry {
            place.fit_anchor_to_ring(ring);
        }
        world.distances.places.push(place);
        world.place_geometry.push(self.territory.clone());
        Ok(place_geometry_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        // 两处定位都先完成、再动世界：任一处缺失时整体失败，不留下
        // 「地点删了、地界还在」的半回退状态（P6：一次编辑整体回退）。
        let place_pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place.id)))?;
        // 按 (map, owner, span.from) 定位——span 的身份由这三元组决定，
        // 与 UpdatePlaceTerritoryCommand 的查找口径一致。
        let geo_pos = world
            .place_geometry
            .iter()
            .position(|g| {
                g.map == self.territory.map
                    && g.owner == self.territory.owner
                    && g.span.from == self.territory.span.from
            })
            .ok_or_else(|| {
                EditError::TargetMissing(format!("Place geometry of {}", self.place.id))
            })?;
        world.distances.places.remove(place_pos);
        world.place_geometry.remove(geo_pos);
        Ok(())
    }
}

/// 改写某地点某个地界 span 的多边形环，并重拟合锚点。
///
/// ## 为什么定位键是 `(map, owner, span.from)`
///
/// 同一地点在同一张图上可以有多个时间区间的地界（边界演化 = 追加 span，
/// P6/P7），因此 `owner` 不足以唯一定位；`from` 是区间的起点，与
/// [`GeometrySpan`] 的 `TimeSpan` 一起构成稳定身份。低层 `GeometrySpan` 没有
/// 独立 ID，这是现有底座下最精确的定位方式。
///
/// ## 回滚
///
/// `apply` 缓存旧 `Place` 全字段与旧 `GeometrySpan`；`revert` 原样写回。
/// 缓存整段 span（而非只缓存旧环）是为了让回滚与 apply 的改动面严格对称——
/// 即使将来 apply 在此处多改一个字段，revert 也不会漏。
#[derive(Debug)]
pub struct UpdatePlaceTerritoryCommand {
    /// 目标地点 ID（同时是 span 的 `owner`）。
    pub place_id: wv_core::Id,
    /// 目标地图 ID（span 的 `map`）。
    pub map: wv_core::Id,
    /// 目标 span 的区间起点（`span.from`）。
    pub span_from: i64,
    /// 新的多边形环（扁平 `[x0, y0, x1, y1, ...]`，首尾不重复）。
    pub polygon: Vec<f32>,
    /// `apply` 时缓存的旧值：`(旧 Place 全字段, 旧 GeometrySpan)`。
    ///
    /// 用 `Mutex` 而非 `RefCell`，使命令满足 `Send + Sync`（Bevy 资源约束）。
    before: std::sync::Mutex<Option<(Place, GeometrySpan)>>,
}

impl UpdatePlaceTerritoryCommand {
    pub fn new(place_id: wv_core::Id, map: wv_core::Id, span_from: i64, polygon: Vec<f32>) -> Self {
        Self {
            place_id,
            map,
            span_from,
            polygon,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdatePlaceTerritoryCommand {
    fn label(&self) -> String {
        format!("改地界「{}」", self.place_id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let place_pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place_id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place_id)))?;
        let geo_pos = world
            .place_geometry
            .iter()
            .position(|g| {
                g.map == self.map && g.owner == self.place_id && g.span.from == self.span_from
            })
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "Place geometry (map {}, owner {}, from {})",
                    self.map, self.place_id, self.span_from
                ))
            })?;

        let old_place = world.distances.places[place_pos].clone();
        let old_span = world.place_geometry[geo_pos].clone();
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((old_place, old_span));

        world.place_geometry[geo_pos].geometry = GeometryData::Polygon(self.polygon.clone());
        world.distances.places[place_pos].fit_anchor_to_ring(&self.polygon);
        Ok(place_geometry_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (old_place, old_span) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdatePlaceTerritory 尚未 apply".into()))?;

        let place_pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place_id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place_id)))?;
        let geo_pos = world
            .place_geometry
            .iter()
            .position(|g| {
                g.map == old_span.map
                    && g.owner == old_span.owner
                    && g.span.from == old_span.span.from
            })
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "Place geometry (map {}, owner {}, from {})",
                    old_span.map, old_span.owner, old_span.span.from
                ))
            })?;

        // 两处定位都成功后再写回，避免半回退。
        world.distances.places[place_pos] = old_place;
        world.place_geometry[geo_pos] = old_span;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::GeometryData;
    use wv_core::time::TimeSpan;
    use wv_core::Id;

    /// 测试用的图 ID——`PaintTerrainCommand` 的栅格是**按图**查的。
    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(size: u32) -> World {
        let grid = TerrainGrid::new(size, size);
        let data = from_grid(grid);
        let mut world = World::default();
        world.set_terrain(test_map(), data);
        world
    }

    #[test]
    fn paint_command_writes_surface_and_biome_then_revert_restores() {
        let mut world = world_with_terrain(32);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint(
            (10.5, 10.5),
            5.0,
            wv_terrain::edit::PaintTarget {
                surface: wv_terrain::SURFACE_LAND,
                biome: wv_terrain::BIOME_GRASSLAND,
            },
        ));
        let cmd = PaintTerrainCommand::new(test_map(), stroke, 1.0);

        let dirty = cmd.apply(&mut world).unwrap();
        assert!(!dirty.is_empty(), "应报告脏文件");
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        assert_ne!(before_snapshot, after, "apply 应改变地形");
        // 中心格地表/群系被写为目标值
        assert_eq!(after.cells[10 * 32 + 10].biome, wv_terrain::BIOME_GRASSLAND);
        assert_eq!(after.cells[10 * 32 + 10].surface, wv_terrain::SURFACE_LAND);

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "revert 应完全还原"
        );
    }

    /// 域对齐不回退（设计稿 D19/Review Focus 3）：画水把高度推过海平面下方。
    #[test]
    fn water_target_pushes_height_below_sea_level() {
        let mut world = world_with_terrain(32);
        // 先铺 +200 m 陆地。
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 200,
                surface: 1,
                biome: 0,
            };
            32 * 32
        ];
        world.set_terrain(
            test_map(),
            TerrainData {
                width: 32,
                height: 32,
                cells: land,
            },
        );

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint(
            (10.5, 10.5),
            5.0,
            wv_terrain::edit::PaintTarget {
                surface: wv_terrain::SURFACE_WATER,
                biome: wv_terrain::BIOME_NONE,
            },
        ));
        PaintTerrainCommand::new(test_map(), stroke, 1.0)
            .apply(&mut world)
            .unwrap();
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        let c = &after.cells[10 * 32 + 10];
        assert_eq!(c.surface, wv_terrain::SURFACE_WATER);
        assert!(c.height < 0, "画水应推到海平面下，得 {}", c.height);
    }

    /// 域对齐对称侧：画陆把海底格抬到海平面上方。
    #[test]
    fn land_target_pushes_height_above_sea_level() {
        let mut world = world_with_terrain(32);
        let sea: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: -200,
                surface: 0,
                biome: 0,
            };
            32 * 32
        ];
        world.set_terrain(
            test_map(),
            TerrainData {
                width: 32,
                height: 32,
                cells: sea,
            },
        );

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint(
            (10.5, 10.5),
            5.0,
            wv_terrain::edit::PaintTarget {
                surface: wv_terrain::SURFACE_LAND,
                biome: wv_terrain::BIOME_GRASSLAND,
            },
        ));
        PaintTerrainCommand::new(test_map(), stroke, 1.0)
            .apply(&mut world)
            .unwrap();
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        let c = &after.cells[10 * 32 + 10];
        assert_eq!(c.surface, wv_terrain::SURFACE_LAND);
        assert!(c.height >= 0, "画陆应抬到海平面上，得 {}", c.height);
    }

    #[test]
    fn painting_one_map_leaves_the_other_maps_grid_untouched() {
        // Phase 2 的关键不变量：栅格按图存放，刷 A 城不能碰到 B 城。
        let mut world = world_with_terrain(32);
        let other = Id::new_unchecked("map_city_other");
        world.set_terrain(other.clone(), from_grid(TerrainGrid::new(16, 16)));
        let other_before = world.terrain_of(&other).cloned().unwrap();

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint(
            (10.5, 10.5),
            5.0,
            wv_terrain::edit::PaintTarget {
                surface: wv_terrain::SURFACE_LAND,
                biome: wv_terrain::BIOME_GRASSLAND,
            },
        ));
        let cmd = PaintTerrainCommand::new(test_map(), stroke, 1.0);
        cmd.apply(&mut world).unwrap();

        assert_eq!(
            world.terrain_of(&other).cloned().unwrap(),
            other_before,
            "另一张图的栅格必须一个格都没动"
        );
        assert_eq!(world.terrains.len(), 2, "不应凭空多出或丢掉栅格");
    }

    #[test]
    fn paint_command_without_terrain_reports_mismatch() {
        let mut world = World::default();
        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint(
            (0.0, 0.0),
            1.0,
            wv_terrain::edit::PaintTarget {
                surface: wv_terrain::SURFACE_WATER,
                biome: wv_terrain::BIOME_NONE,
            },
        ));
        let cmd = PaintTerrainCommand::new(Id::new_unchecked("world_main"), stroke, 1.0);
        assert!(matches!(
            cmd.apply(&mut world),
            Err(EditError::TerrainMismatch(_))
        ));
    }

    // ---- 区域意图命令（2026-10-17 选择工具 M3：复用 IntentTerrainCommand） ----

    /// 区域意图经同一条命令路径：apply 改地形、revert 逐位复原（零新命令面）。
    #[test]
    fn region_intent_command_undo_restores_bitwise() {
        let mut world = world_with_terrain(48);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

        // 用 MountainMassif（fixture 是海面，平原只在陆上生效会 0 改动——
        // 压平带断言由 wv-terrain 的 plains 测试背书）。
        let cmd = IntentTerrainCommand::new(
            test_map(),
            Intent::MountainMassif {
                polygon: vec![(12.0, 12.0), (36.0, 12.0), (36.0, 36.0), (12.0, 36.0)],
            },
            1.0,
        );
        let dirty = cmd.apply(&mut world).unwrap();
        assert_eq!(dirty, vec!["maps/world_main/terrain.bin".to_string()]);
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        assert_ne!(before_snapshot, after, "apply 应在掩膜内隆起山体");

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "revert 后网格逐位复原"
        );
    }

    /// 同一多边形两次构造 → 同 seed（spec：确定性，重放不漂移）。
    #[test]
    fn region_intent_seed_is_deterministic_for_same_polygon() {
        let polygon = vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)];
        let a = IntentTerrainCommand::new(
            test_map(),
            Intent::MountainMassif {
                polygon: polygon.clone(),
            },
            1.0,
        );
        let b = IntentTerrainCommand::new(test_map(), Intent::MountainMassif { polygon }, 1.0);
        assert_eq!(a.seed, b.seed, "同环同图 seed 必须一致");
        let c = IntentTerrainCommand::new(
            test_map(),
            Intent::LakeRegion {
                polygon: vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)],
            },
            1.0,
        );
        assert_ne!(a.seed, c.seed, "不同意图 seed 必须不同");
    }

    // ---- 意图笔刷命令（意图笔刷 plan v0.1） ----

    #[test]
    fn intent_command_apply_then_revert_restores_grid() {
        let mut world = world_with_terrain(48);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

        let cmd = IntentTerrainCommand::new(
            test_map(),
            Intent::MountainRange {
                pts: vec![(10.0, 24.0), (38.0, 24.0)],
                radius: 5.0,
                strength: 1.0,
            },
            1.0,
        );
        let dirty = cmd.apply(&mut world).unwrap();
        assert_eq!(dirty, vec!["maps/world_main/terrain.bin".to_string()]);
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        assert_ne!(before_snapshot, after, "apply 应改变地形");
        // 中段脊线确实隆起（高度远超 0）。
        assert!(
            after.cells[24 * 48 + 24].height > 500,
            "脊线中段应显著隆起，实得 {}",
            after.cells[24 * 48 + 24].height
        );

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "revert 应完全还原"
        );
    }

    #[test]
    fn intent_command_redo_replays_identical_grid() {
        // undo 后 redo：revert 已把 before 写回，re-apply 用同一 seed，
        // 结果必须与首次 apply 逐格一致（确定性契约）。
        let mut world = world_with_terrain(48);
        let cmd = IntentTerrainCommand::new(
            test_map(),
            Intent::Volcano {
                center: (24.0, 24.0),
                radius: 8.0,
                strength: 1.0,
            },
            1.0,
        );
        cmd.apply(&mut world).unwrap();
        let first = world.terrain_of(&test_map()).cloned().unwrap();
        cmd.revert(&mut world).unwrap();
        cmd.apply(&mut world).unwrap();
        let second = world.terrain_of(&test_map()).cloned().unwrap();
        assert_eq!(first.cells, second.cells, "redo 应逐格重现首次结果");
    }

    #[test]
    fn intent_command_without_terrain_reports_mismatch() {
        let mut world = World::default();
        let cmd = IntentTerrainCommand::new(
            Id::new_unchecked("world_main"),
            Intent::Hills {
                pts: vec![(5.0, 5.0)],
                radius: 4.0,
                strength: 1.0,
            },
            1.0,
        );
        assert!(matches!(
            cmd.apply(&mut world),
            Err(EditError::TerrainMismatch(_))
        ));
    }

    // ---- 势力命令（S12 v0.2） ----

    fn f(id: &str, name: &str) -> Faction {
        Faction::new(
            Id::new_unchecked(id),
            name,
            wv_core::color::Color("#112233".into()),
        )
    }

    fn geo(map: &str, owner: &str, from: i64, to: Option<i64>) -> GeometrySpan {
        GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: TimeSpan { from, to },
            geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
            relation: None,
        }
    }

    fn layer(id: &str) -> FactionLayer {
        FactionLayer {
            id: Id::new_unchecked(id),
            name: id.into(),
            z: 0,
            color_mode: FactionColorMode::Own,
            visible: true,
        }
    }

    fn mem(target: &str, from: i64) -> Membership {
        Membership {
            target: Id::new_unchecked(target),
            kind: Some("实控".into()),
            span: TimeSpan::open(from),
        }
    }

    // ---- 势力划分（ADR-0028 / 势力交互 M1 Task 5） ----

    fn square_ring(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<f32> {
        vec![x0, y0, x1, y0, x1, y1, x0, y1]
    }

    /// 一张带方形宿主势力 `faction_host` 的世界（地图 = TEST_MAP，格 1 wu）。
    fn world_with_host_faction() -> World {
        let mut w = world_with_terrain(64);
        w.factions
            .push(f(Id::new_unchecked("faction_host").as_str(), "宿主国"));
        w.factions.last_mut().expect("刚推入").id = Id::new_unchecked("faction_host");
        w.faction_geometry.push(GeometrySpan {
            map: test_map(),
            owner: Id::new_unchecked("faction_host"),
            span: TimeSpan { from: 0, to: None },
            geometry: GeometryData::Polygon(square_ring(4.0, 4.0, 60.0, 60.0)),
            relation: None,
        });
        w
    }

    #[test]
    fn divide_creates_faction_and_shrinks_host_atomically() {
        let mut w = world_with_host_faction();
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        let dirty = cmd.apply(&mut w).expect("划分应成功");
        assert!(dirty.contains(&"factions".to_string()));
        assert_eq!(w.factions.len(), 2, "新势力应入场");
        assert_eq!(w.faction_geometry.len(), 2, "新势力疆界应入场");
        // 宿主余部变成带洞环：1 个洞。
        let host = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_host"))
            .expect("宿主疆界仍在");
        let GeometryData::PolygonWithHoles { holes, .. } = &host.geometry else {
            panic!("宿主余部应为带洞环，实得 {:?}", host.geometry);
        };
        assert_eq!(holes.len(), 1, "宿主余部应有 1 个洞");
        // 共边（D3）：新势力外环 == 宿主洞沿反序（逐点相等）。
        let new_geo = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_new"))
            .expect("新势力疆界");
        let GeometryData::Polygon(new_outer) = &new_geo.geometry else {
            panic!("新势力应为单环，实得 {:?}", new_geo.geometry);
        };
        let mut reversed = new_outer.clone();
        let n = reversed.len() / 2;
        let mut pts: Vec<[f32; 2]> = reversed.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
        pts.reverse();
        reversed = pts.into_iter().flatten().collect();
        assert_eq!(&holes[0], &reversed, "共边必须逐点相等（D3）");
        assert_eq!(n * 2, new_outer.len(), "顶点数自洽");
    }

    #[test]
    fn divide_at_real_cell_size_keeps_remainder_inside_host() {
        // C1 回归：宿主几何是 wu，内核吃格坐标——cell_size=8 时宿主必须
        // 先除再进内核。宿主 80..480 wu（格 10..60），意图 160..320 wu（格 20..40）。
        let mut w = world_with_land_terrain();
        let cell = 8.0;
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            };
            64 * 64
        ];
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells: land,
            },
        );
        w.factions
            .push(f(Id::new_unchecked("faction_host").as_str(), "宿主国"));
        w.factions.last_mut().expect("刚推入").id = Id::new_unchecked("faction_host");
        w.faction_geometry.push(GeometrySpan {
            map: test_map(),
            owner: Id::new_unchecked("faction_host"),
            span: TimeSpan { from: 0, to: None },
            geometry: GeometryData::Polygon(square_ring(80.0, 80.0, 480.0, 480.0)),
            relation: None,
        });
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![
                (160.0, 160.0),
                (320.0, 160.0),
                (320.0, 320.0),
                (160.0, 320.0),
            ],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            cell,
        );
        cmd.apply(&mut w).expect("cell_size=8 的划分应成功");

        let host = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_host"))
            .expect("宿主疆界仍在");
        let bbox = |flat: &[f32]| -> (f32, f32, f32, f32) {
            let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in flat.chunks_exact(2) {
                b.0 = b.0.min(p[0]);
                b.1 = b.1.min(p[1]);
                b.2 = b.2.max(p[0]);
                b.3 = b.3.max(p[1]);
            }
            b
        };
        let host_outer = match &host.geometry {
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            GeometryData::Polygon(v) => v.clone(),
            other => panic!("宿主余部应为多边形，实得 {other:?}"),
        };
        let hb = bbox(&host_outer);
        assert!(
            hb.0 >= 72.0 && hb.1 >= 72.0 && hb.2 <= 488.0 && hb.3 <= 488.0,
            "宿主余部必须留在原宿主 80..480 wu 内，实得 bbox {hb:?}"
        );
        let new_geo = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_new"))
            .expect("新势力疆界");
        let new_outer = match &new_geo.geometry {
            GeometryData::Polygon(v) => v.clone(),
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            other => panic!("新势力应为多边形，实得 {other:?}"),
        };
        let nb = bbox(&new_outer);
        assert!(
            nb.0 >= 152.0 && nb.1 >= 152.0 && nb.2 <= 328.0 && nb.3 <= 328.0,
            "新势力必须落在意图 160..320 wu 附近，实得 bbox {nb:?}"
        );
        // 余部面积应约为宿主减新势力（不是整张图）。
        let area = |flat: &[f32]| -> f32 {
            let n = flat.len() / 2;
            let mut a = 0.0;
            for i in 0..n {
                let (x0, y0) = (flat[2 * i], flat[2 * i + 1]);
                let (x1, y1) = (flat[2 * ((i + 1) % n)], flat[2 * ((i + 1) % n) + 1]);
                a += x0 * y1 - x1 * y0;
            }
            (a / 2.0).abs()
        };
        let host_area = area(&host_outer);
        assert!(
            (150_000.0..=170_000.0).contains(&host_area),
            "宿主余部外环应 ≈400×400=160000 wu²（不是整图），实得 {host_area}"
        );
        let GeometryData::PolygonWithHoles { holes, .. } = &host.geometry else {
            panic!("居中切应产生带洞余部，实得 {:?}", host.geometry);
        };
        let hole_area = area(&holes[0]);
        assert!(
            (24_000.0..=28_000.0).contains(&hole_area),
            "洞应 ≈160×160=25600 wu²，实得 {hole_area}"
        );
    }

    #[test]
    fn divide_undo_restores_host_and_removes_new_faction_bitwise() {
        let mut w = world_with_host_faction();
        let factions_before = w.factions.clone();
        let geometry_before = w.faction_geometry.clone();
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        cmd.apply(&mut w).unwrap();
        cmd.revert(&mut w).unwrap();
        assert_eq!(w.factions, factions_before, "revert 应移除新势力");
        assert_eq!(
            w.faction_geometry, geometry_before,
            "revert 应逐位还原宿主疆界"
        );
    }

    /// 全陆地地形（格 1 wu）：空势力宿主的划分对象。
    fn world_with_land_terrain() -> World {
        let mut w = world_with_terrain(64);
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            };
            64 * 64
        ];
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells: land,
            },
        );
        w
    }

    #[test]
    fn divide_from_null_host_writes_no_host_span() {
        let mut w = world_with_land_terrain();
        let before_count = w.faction_geometry.len();
        let cmd = DivideFactionCommand::new(
            test_map(),
            None,
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        cmd.apply(&mut w).expect("空势力划分应成功");
        assert_eq!(w.factions.len(), 1);
        assert_eq!(w.faction_geometry.len(), before_count + 1, "只加新势力疆界");
    }

    #[test]
    fn null_host_excludes_water_from_the_new_faction() {
        // 意图正中有一片水（湖/海）：新势力应带洞（水域归空势力，D4/Q3）。
        let mut w = world_with_land_terrain();
        let mut cells = w.terrain_of(&test_map()).cloned().unwrap().cells;
        for y in 26..38 {
            for x in 26..38 {
                let c = &mut cells[y * 64 + x];
                c.height = -20;
                c.surface = 0;
                c.biome = 0;
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let cmd = DivideFactionCommand::new(
            test_map(),
            None,
            vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "环湖国"),
            0,
            1.0,
        );
        cmd.apply(&mut w).expect("环湖划分应成功");
        let geo = &w.faction_geometry[0];
        let GeometryData::PolygonWithHoles { holes, .. } = &geo.geometry else {
            panic!("环湖国应带洞，实得 {:?}", geo.geometry);
        };
        assert_eq!(holes.len(), 1, "湖应成为一个洞");
    }

    #[test]
    fn divide_fragmenting_intent_is_precondition_error() {
        let mut w = world_with_host_faction();
        // 宿主改成哑铃形：两间房 + 1 格走廊（横切走廊 → 余部碎块化）。
        w.faction_geometry[0].geometry = GeometryData::Polygon(vec![
            4.0, 4.0, 24.0, 4.0, 24.0, 24.0, 26.0, 24.0, 26.0, 4.0, 46.0, 4.0, 46.0, 44.0, 26.0,
            44.0, 26.0, 24.0, 24.0, 24.0, 24.0, 44.0, 4.0, 44.0,
        ]);
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(22.0, 0.0), (28.0, 0.0), (28.0, 50.0), (22.0, 50.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        let err = cmd.apply(&mut w).expect_err("碎块化必须被拒绝");
        assert!(
            matches!(err, EditError::Precondition(_)),
            "应是 Precondition，实得 {err:?}"
        );
        assert_eq!(w.factions.len(), 1, "失败不得改动世界");
        assert_eq!(w.faction_geometry.len(), 1, "失败不得改动疆界");
    }

    #[test]
    fn divide_duplicate_faction_id_is_precondition_error() {
        let mut w = world_with_host_faction();
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_host").as_str(), "重名"),
            0,
            1.0,
        );
        assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
    }

    #[test]
    fn add_faction_layer_apply_then_revert_removes() {
        let mut w = World::default();
        let cmd = AddFactionLayerCommand::new(layer("layer_admin"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_layers.len(), 1);
        assert!(dirty.contains(&"faction_layers".to_string()));
        cmd.revert(&mut w).unwrap();
        assert!(w.faction_layers.is_empty());
    }

    #[test]
    fn update_faction_layer_round_trip() {
        let mut w = World::default();
        AddFactionLayerCommand::new(layer("layer_admin"))
            .apply(&mut w)
            .unwrap();
        let cmd = UpdateFactionLayerCommand::new(
            Id::new_unchecked("layer_admin"),
            "行政区".into(),
            5,
            FactionColorMode::Inherit,
            false,
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_layers[0].name, "行政区");
        assert_eq!(w.faction_layers[0].z, 5);
        assert_eq!(w.faction_layers[0].color_mode, FactionColorMode::Inherit);
        assert!(!w.faction_layers[0].visible);
        cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_layers[0].name, "layer_admin");
        assert_eq!(w.faction_layers[0].z, 0);
    }

    #[test]
    fn remove_faction_layer_reassigns_and_undo_restores() {
        let mut w = World::default();
        AddFactionLayerCommand::new(layer("layer_admin"))
            .apply(&mut w)
            .unwrap();
        let mut faction = f("f1", "F1");
        faction.layer = Some(Id::new_unchecked("layer_admin"));
        w.factions.push(faction);

        let cmd = RemoveFactionLayerCommand::new(Id::new_unchecked("layer_admin"));
        cmd.apply(&mut w).unwrap();
        assert!(w.faction_layers.is_empty());
        assert_eq!(w.factions[0].layer, None, "删层不得删势力，成员回落默认层");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_layers.len(), 1);
        assert_eq!(
            w.factions[0].layer.as_ref().map(|i| i.as_str()),
            Some("layer_admin")
        );
    }

    #[test]
    fn set_faction_layer_rank_and_seat_round_trip() {
        let mut w = World::default();
        w.factions.push(f("f1", "F1"));

        let set_layer = SetFactionLayerCommand::new(
            Id::new_unchecked("f1"),
            Some(Id::new_unchecked("layer_admin")),
        );
        set_layer.apply(&mut w).unwrap();
        assert_eq!(
            w.factions[0].layer.as_ref().map(|i| i.as_str()),
            Some("layer_admin")
        );
        set_layer.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].layer, None);

        let set_rank = SetFactionRankCommand::new(Id::new_unchecked("f1"), Some("province".into()));
        set_rank.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].rank.as_deref(), Some("province"));
        set_rank.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].rank, None);

        let set_seat =
            SetFactionSeatCommand::new(Id::new_unchecked("f1"), Some(Id::new_unchecked("p1")));
        set_seat.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].seat.as_ref().map(|i| i.as_str()), Some("p1"));
        set_seat.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].seat, None);
    }

    #[test]
    fn add_membership_keeps_same_target_with_different_from() {
        let mut w = World::default();
        w.factions.push(f("f1", "F1"));
        AddMembershipCommand::new(Id::new_unchecked("f1"), mem("f2", 1))
            .apply(&mut w)
            .unwrap();
        AddMembershipCommand::new(Id::new_unchecked("f1"), mem("f2", 2))
            .apply(&mut w)
            .unwrap();
        assert_eq!(
            w.factions[0].memberships.len(),
            2,
            "同一 target 不同 from 是两条独立边，不得合并"
        );
    }

    #[test]
    fn remove_membership_by_target_and_from_round_trip() {
        let mut w = World::default();
        let mut faction = f("f1", "F1");
        faction.memberships.push(mem("f2", 1));
        faction.memberships.push(mem("f2", 2));
        w.factions.push(faction);

        let cmd = RemoveMembershipCommand::new(Id::new_unchecked("f1"), Id::new_unchecked("f2"), 1);
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].memberships.len(), 1);
        assert_eq!(w.factions[0].memberships[0].span.from, 2);
        cmd.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].memberships.len(), 2);
    }

    #[test]
    fn upsert_geometry_keeps_claim_and_control_with_same_from() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let mut control = geo("m", "f1", 1, None);
        control.relation = None;
        let mut claim = geo("m", "f1", 1, None);
        claim.relation = Some("claim".into());
        UpsertFactionGeometryCommand::new(control)
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(claim)
            .apply(&mut w)
            .unwrap();
        assert_eq!(
            w.faction_geometry.len(),
            2,
            "同 map+owner+from 的 claim/control 必须并存（ADR-0027 D5）"
        );
    }

    #[test]
    fn add_faction_apply_then_revert_removes() {
        let mut w = World::default();
        let cmd = AddFactionCommand::new(f("f1", "F1"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.factions.len(), 1);
        assert!(dirty.contains(&"factions".to_string()));
        cmd.revert(&mut w).unwrap();
        assert!(w.factions.is_empty());
    }

    #[test]
    fn add_faction_duplicate_id_is_precondition_error() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let err = AddFactionCommand::new(f("f1", "F1-again")).apply(&mut w);
        assert!(matches!(err, Err(EditError::Precondition(_))));
    }

    #[test]
    fn remove_faction_also_clears_its_geometry() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 2);

        let rm = RemoveFactionCommand::new(Id::new_unchecked("f1"));
        rm.apply(&mut w).unwrap();
        assert!(w.factions.is_empty(), "Faction 自身被删");
        assert!(w.faction_geometry.is_empty(), "名下几何连带清空");

        rm.revert(&mut w).unwrap();
        assert_eq!(w.factions.len(), 1, "Faction 还原");
        assert_eq!(w.faction_geometry.len(), 2, "几何还原");
    }

    #[test]
    fn remove_faction_missing_is_target_error() {
        let mut w = World::default();
        let cmd = RemoveFactionCommand::new(Id::new_unchecked("ghost"));
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    #[test]
    fn update_faction_round_trips_via_revert() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let upd = UpdateFactionCommand::new(
            Id::new_unchecked("f1"),
            "F1-Renamed".into(),
            wv_core::color::Color("#FF0000".into()),
        );
        upd.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].name, "F1-Renamed");
        assert_eq!(w.factions[0].color.as_str(), "#FF0000");
        upd.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].name, "F1");
        assert_eq!(w.factions[0].color.as_str(), "#112233");
    }

    #[test]
    fn upsert_geometry_inserts_when_new() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let upsert = UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)));
        let dirty = upsert.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 1);
        assert!(dirty.contains(&"geometry/faction".to_string()));
        // revert：应能移除刚 push 的那条
        upsert.revert(&mut w).unwrap();
        assert!(w.faction_geometry.is_empty());
    }

    #[test]
    fn upsert_geometry_replaces_when_same_key() {
        // (map, owner, from) 一致 → 替换。`from` 不同的算作"另一版本" → 共存。
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(20)))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 1, "from 相同 → 替换");
        assert_eq!(w.faction_geometry[0].span.to, Some(20));
        UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 2, "from 不同 → 共存");
    }

    #[test]
    fn upsert_geometry_revert_restores_replaced_value() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let original = geo("m", "f1", 1, Some(10));
        UpsertFactionGeometryCommand::new(original.clone())
            .apply(&mut w)
            .unwrap();
        let updated = geo("m", "f1", 1, Some(99));
        let upd_cmd = UpsertFactionGeometryCommand::new(updated);
        upd_cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry[0].span.to, Some(99));
        upd_cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_geometry[0].span.to, Some(10), "应恢复为 10");
    }

    #[test]
    fn upsert_geometry_rejects_orphan_owner() {
        let mut w = World::default();
        // 没有 add Faction。
        let cmd = UpsertFactionGeometryCommand::new(geo("m", "f_ghost", 1, Some(10)));
        assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
    }

    #[test]
    fn remove_geometry_apply_then_revert_restores_at_original_index() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
            .apply(&mut w)
            .unwrap();
        // 删中间那条 (from=20)。
        let rm =
            RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 20);
        rm.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 2);
        assert!(w.faction_geometry.iter().all(|g| g.span.from != 20));
        rm.revert(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 3);
        assert_eq!(w.faction_geometry[1].span.from, 20, "按原位置 1 还原");
    }

    #[test]
    fn remove_geometry_missing_is_target_error() {
        let mut w = World::default();
        let cmd =
            RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 999);
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    // ---------------------- 街道命令（ADR-0010 ③） ----------------------

    fn seg(class: wv_core::RoadClass) -> wv_core::StreetSegment {
        wv_core::StreetSegment::straight(class, 10.0, 20.0, 30.0, 40.0)
    }

    const TEST_LAYER: &str = "layer_city_streets";

    fn test_layer() -> Id {
        Id::new_unchecked(TEST_LAYER)
    }

    // ---------------------- 建筑命令（ADR-0016 §6） ----------------------

    const TEST_BUILDING_LAYER: &str = "layer_city_buildings";

    fn test_building_layer() -> Id {
        Id::new_unchecked(TEST_BUILDING_LAYER)
    }

    fn sample_building(h: f32) -> wv_core::Building {
        wv_core::Building::rect(0.0, 0.0, 8.0, 8.0, h)
    }

    #[test]
    fn add_building_then_revert_restores_the_document() {
        let mut w = World::default();
        let mut doc = wv_core::BuildingLayer::new();
        doc.push(sample_building(6.0));
        w.set_buildings(test_building_layer(), doc);

        let cmd = AddBuildingCommand::new(test_building_layer(), sample_building(12.0));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("buildings/{}", TEST_BUILDING_LAYER)],
            "脏路径应为虚拟命名空间 `buildings/<layer>`"
        );
        assert_eq!(w.building_of(&test_building_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        let doc = w.building_of(&test_building_layer()).unwrap();
        assert_eq!(doc.len(), 1, "revert 应截回追加前长度");
        assert!(
            (doc.buildings[0].height_m - 6.0).abs() < 1e-6,
            "被截掉的是新画的那栋，旧栋原样（高度实得 {}）",
            doc.buildings[0].height_m
        );
    }

    #[test]
    fn delete_building_reverts_by_reinserting_at_the_same_index() {
        let mut w = World::default();
        let mut doc = wv_core::BuildingLayer::new();
        doc.push(sample_building(6.0));
        doc.push(sample_building(12.0));
        doc.push(sample_building(18.0));
        w.set_buildings(test_building_layer(), doc);

        let cmd = DeleteBuildingCommand::new(test_building_layer(), 1);
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.building_of(&test_building_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        let doc = w.building_of(&test_building_layer()).unwrap();
        assert_eq!(doc.len(), 3, "revert 应插回原索引");
        assert!(
            (doc.buildings[1].height_m - 12.0).abs() < 1e-6,
            "插回的应是原 #1（高度实得 {}）",
            doc.buildings[1].height_m
        );
    }

    #[test]
    fn delete_building_out_of_range_is_a_precondition_error() {
        let mut w = World::default();
        let err = DeleteBuildingCommand::new(test_building_layer(), 5)
            .apply(&mut w)
            .unwrap_err();
        assert!(
            matches!(err, crate::EditError::Precondition(_)),
            "越界删除应报 Precondition，实得 {err:?}"
        );
    }

    #[test]
    fn add_street_segment_then_revert_restores_the_document() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        w.set_streets(test_layer(), doc);

        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Service));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("streets/{}", TEST_LAYER)],
            "脏路径应为虚拟命名空间 `streets/<layer>`"
        );
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            1,
            "revert 应截回追加前长度"
        );
        assert_eq!(
            w.street_of(&test_layer()).unwrap().segments[0].class,
            wv_core::RoadClass::Primary,
            "被截掉的是新画的那段，旧段原样"
        );
    }

    #[test]
    fn drawing_on_an_unloaded_street_layer_creates_it_silently() {
        let mut w = World::default();
        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Residential));
        cmd.apply(&mut w).unwrap();
        let doc = w.street_of(&test_layer()).expect("首次落笔应新建文档");
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.segments[0].class, wv_core::RoadClass::Residential);
    }

    #[test]
    fn reverting_before_apply_is_a_precondition_error() {
        let mut w = World::default();
        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Path));
        assert!(matches!(
            cmd.revert(&mut w),
            Err(EditError::Precondition(_))
        ));
    }

    #[test]
    fn two_street_commands_undo_independently() {
        let mut w = World::default();
        let a = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Primary));
        let b = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Path));
        a.apply(&mut w).unwrap();
        b.apply(&mut w).unwrap();
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);
        b.revert(&mut w).unwrap();
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 1);
        a.revert(&mut w).unwrap();
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            0,
            "逐条撤销后文档应为空"
        );
    }

    #[test]
    fn deleting_a_segment_then_reverting_restores_it_at_the_same_index() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        doc.push(seg(wv_core::RoadClass::Path));
        doc.push(seg(wv_core::RoadClass::Service));
        w.set_streets(test_layer(), doc);

        let cmd = DeleteStreetSegmentCommand::new(test_layer(), 1);
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("streets/{}", TEST_LAYER)],
            "脏路径应为虚拟命名空间 `streets/<layer>`"
        );
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);
        assert_eq!(
            w.street_of(&test_layer()).unwrap().segments[1].class,
            wv_core::RoadClass::Service,
            "中间那段应被删掉"
        );

        cmd.revert(&mut w).unwrap();
        let doc = w.street_of(&test_layer()).unwrap();
        assert_eq!(doc.len(), 3, "revert 应把段插回原索引");
        assert_eq!(
            doc.segments[1].class,
            wv_core::RoadClass::Path,
            "插回的必须是原来那一段"
        );
    }

    #[test]
    fn deleting_an_out_of_range_segment_is_a_precondition_error() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        w.set_streets(test_layer(), doc);

        let cmd = DeleteStreetSegmentCommand::new(test_layer(), 1);
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "越界索引必须被拒绝而不是 panic"
        );
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            1,
            "失败的命令不得改动文档"
        );
    }

    #[test]
    fn move_city_translates_holed_outline_including_holes() {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let mut w = World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("map_city"),
            name: "城".into(),
            level: MapLevel::City,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: Some(GeometryData::PolygonWithHoles {
                outer: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
                holes: vec![vec![4.0, 4.0, 6.0, 4.0, 6.0, 6.0, 4.0, 6.0]],
            }),
        });
        let cmd = MoveCityCommand::new(
            Id::new_unchecked("map_city"),
            wv_core::Point::new(5.0, -3.0),
        );
        cmd.apply(&mut w).expect("带洞轮廓应可平移");
        let GeometryData::PolygonWithHoles { outer, holes } =
            w.maps[0].outline_polygon.clone().expect("轮廓仍在")
        else {
            panic!("变体不应变化");
        };
        assert_eq!(outer[0], 5.0, "外环 x 应 +5，实得 {}", outer[0]);
        assert_eq!(outer[1], -3.0, "外环 y 应 -3，实得 {}", outer[1]);
        assert_eq!(holes[0][0], 9.0, "洞环 x 也应 +5，实得 {}", holes[0][0]);
        assert_eq!(holes[0][1], 1.0, "洞环 y 也应 -3，实得 {}", holes[0][1]);
        cmd.revert(&mut w).expect("revert");
        let GeometryData::PolygonWithHoles { outer, .. } =
            w.maps[0].outline_polygon.clone().expect("轮廓仍在")
        else {
            panic!("变体不应变化");
        };
        assert_eq!(outer[0], 0.0, "revert 应还原");
    }

    #[test]
    fn set_map_orientation_applies_and_reverts() {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let mut w = World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });

        let cmd = SetMapOrientationCommand::new(Id::new_unchecked("world_main"), 90.0);
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec!["maps/world_main/index.yaml".to_string()],
            "脏路径应为 maps/<id>/index.yaml"
        );
        assert_eq!(w.maps[0].orientation, 90.0, "apply 后朝向应为 90.0");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.maps[0].orientation, 0.0, "revert 后朝向应恢复为 0.0");
    }

    #[test]
    fn set_map_orientation_missing_map_is_a_target_missing_error() {
        let mut w = World::default();
        let cmd = SetMapOrientationCommand::new(Id::new_unchecked("ghost_map"), 45.0);
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "不存在的地图必须报 TargetMissing"
        );
    }

    // -----------------------------------------------------------------------
    // AddMapCommand（新建地图对话框的落点）
    // -----------------------------------------------------------------------

    fn map_node(
        id: &str,
        parent: Option<&str>,
        level: wv_core::map::MapLevel,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: format!("地图{id}"),
            level,
            parent: parent.map(Id::new_unchecked),
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        }
    }

    fn flat_terrain(biome: u8, height: i16) -> TerrainData {
        TerrainData {
            width: 4,
            height: 4,
            cells: vec![
                TerrainCellData {
                    height,
                    surface: if biome == 0 { 0 } else { 1 },
                    biome,
                };
                16
            ],
        }
    }

    #[test]
    fn add_map_command_links_parent_and_prefills_terrain() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));

        let cmd = AddMapCommand::new(
            map_node("map_a", Some("world_main"), wv_core::map::MapLevel::City),
            Some(flat_terrain(0, -40)),
        );
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(
            dirty.contains(&"maps".to_owned())
                && dirty.contains(&"maps/map_a/terrain.bin".to_owned()),
            "脏路径应含节点目录与栅格文件，实得 {dirty:?}"
        );
        // 节点入树 + 父引用回填 + 栅格落位，三件都在。
        assert!(w.maps.iter().any(|m| m.id == Id::new_unchecked("map_a")));
        assert!(w
            .maps
            .iter()
            .find(|m| m.id == Id::new_unchecked("world_main"))
            .unwrap()
            .children
            .contains(&Id::new_unchecked("map_a")));
        assert_eq!(
            w.terrain_of(&Id::new_unchecked("map_a")).unwrap().cells[0].surface,
            0
        );

        // revert 对称拆三件套。
        cmd.revert(&mut w).unwrap();
        assert!(!w.maps.iter().any(|m| m.id == Id::new_unchecked("map_a")));
        assert!(!w
            .maps
            .iter()
            .find(|m| m.id == Id::new_unchecked("world_main"))
            .unwrap()
            .children
            .contains(&Id::new_unchecked("map_a")));
        assert!(w.terrain_of(&Id::new_unchecked("map_a")).is_none());
    }

    #[test]
    fn add_map_command_rejects_duplicate_id_instead_of_overwriting() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        w.maps.push(map_node(
            "map_a",
            Some("world_main"),
            wv_core::map::MapLevel::City,
        ));
        let before = w.maps.len();

        let cmd = AddMapCommand::new(
            map_node("map_a", Some("world_main"), wv_core::map::MapLevel::Region),
            None,
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "重复 ID 必须报错，不能静默覆盖已有地图"
        );
        assert_eq!(w.maps.len(), before, "失败的 apply 不得改动地图表");
    }

    #[test]
    fn add_map_command_rejects_dangling_parent() {
        let mut w = World::default();
        let cmd = AddMapCommand::new(
            map_node("map_a", Some("ghost_parent"), wv_core::map::MapLevel::City),
            None,
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "悬空父引用必须报 TargetMissing"
        );
        assert!(w.maps.is_empty(), "失败的 apply 不得留下半张图");
    }

    #[test]
    fn root_map_adds_without_parent_and_revert_removes_it() {
        // 空编辑器的「第一张图」：parent = None，revert 后世界回到空。
        let mut w = World::default();
        let cmd = AddMapCommand::new(
            map_node("world_main", None, wv_core::map::MapLevel::World),
            Some(flat_terrain(3, 80)),
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.maps.len(), 1);
        cmd.revert(&mut w).unwrap();
        assert!(w.maps.is_empty());
        assert!(w.terrains.is_empty());
    }

    // -----------------------------------------------------------------------
    // 测量命令（2026-10-14）
    // -----------------------------------------------------------------------

    use wv_core::MeasurementKind;

    fn m_line(id: &str) -> Measurement {
        Measurement::polyline(vec![
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(10.0, 0.0),
            wv_core::Point::new(10.0, 5.0),
        ])
        .with_id(Id::new_unchecked(id))
    }

    fn m_rect(id: &str) -> Measurement {
        Measurement::rectangle(
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(20.0, 10.0),
        )
        .with_id(Id::new_unchecked(id))
    }

    fn m_kind(m: &Measurement) -> MeasurementKind {
        m.kind
    }

    #[test]
    fn add_measurement_pushes_and_reports_dirty_path() {
        let mut w = World::default();
        let cmd = AddMeasurementCommand::new(Id::new_unchecked("world_main"), m_line("m1"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec!["measurements/world_main".to_string()],
            "脏路径应为虚拟命名空间 measurements/<map>"
        );
        let overlay = w.measurement_of(&Id::new_unchecked("world_main")).unwrap();
        assert_eq!(overlay.len(), 1);
        assert_eq!(overlay.measurements[0].id.as_str(), "m1");
        assert_eq!(m_kind(&overlay.measurements[0]), MeasurementKind::Polyline);

        cmd.revert(&mut w).unwrap();
        let overlay = w.measurement_of(&Id::new_unchecked("world_main")).unwrap();
        assert!(overlay.is_empty(), "revert 后该图 overlay 应清空");
    }

    #[test]
    fn add_measurement_rejects_duplicate_id() {
        let mut w = World::default();
        AddMeasurementCommand::new(Id::new_unchecked("m1"), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        // 同 ID 二次添加 = Precondition 错。
        let err = AddMeasurementCommand::new(Id::new_unchecked("m1"), m_rect("m1")).apply(&mut w);
        assert!(matches!(err, Err(EditError::Precondition(_))));
        // overlay 长度仍是 1（失败的 apply 不得增改）
        assert_eq!(w.measurement_of(&Id::new_unchecked("m1")).unwrap().len(), 1);
    }

    #[test]
    fn add_measurement_revert_before_apply_is_precondition() {
        let mut w = World::default();
        let cmd = AddMeasurementCommand::new(Id::new_unchecked("m1"), m_line("m1"));
        assert!(matches!(
            cmd.revert(&mut w),
            Err(EditError::Precondition(_))
        ));
    }

    #[test]
    fn remove_measurement_then_revert_restores_at_same_index() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        AddMeasurementCommand::new(map.clone(), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        AddMeasurementCommand::new(map.clone(), m_rect("m2"))
            .apply(&mut w)
            .unwrap();
        AddMeasurementCommand::new(map.clone(), m_line("m3"))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.measurement_of(&map).unwrap().len(), 3);

        // 删中间那条 m2。
        let rm = RemoveMeasurementCommand::new(map.clone(), Id::new_unchecked("m2"));
        let dirty = rm.apply(&mut w).unwrap();
        assert_eq!(dirty, vec!["measurements/world_main".to_string()]);
        let overlay = w.measurement_of(&map).unwrap();
        assert_eq!(overlay.len(), 2);
        assert!(overlay.find(&Id::new_unchecked("m2")).is_none());

        rm.revert(&mut w).unwrap();
        let overlay = w.measurement_of(&map).unwrap();
        assert_eq!(overlay.len(), 3);
        assert_eq!(
            overlay.measurements[1].id.as_str(),
            "m2",
            "应按原位置 1 还原"
        );
        assert_eq!(
            m_kind(&overlay.measurements[1]),
            MeasurementKind::Rectangle,
            "还原的应是矩形那条"
        );
    }

    #[test]
    fn remove_measurement_missing_is_target_error() {
        let mut w = World::default();
        let cmd = RemoveMeasurementCommand::new(
            Id::new_unchecked("world_main"),
            Id::new_unchecked("ghost"),
        );
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    #[test]
    fn clear_measurements_drops_and_revert_restores_full_overlay() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        AddMeasurementCommand::new(map.clone(), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        AddMeasurementCommand::new(map.clone(), m_rect("m2"))
            .apply(&mut w)
            .unwrap();
        let before = w.measurement_of(&map).unwrap().measurements.clone();

        let cmd = ClearMeasurementsCommand::new(map.clone());
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(!dirty.is_empty(), "非空→空 应记脏路径");
        assert!(w.measurement_of(&map).unwrap().is_empty());

        cmd.revert(&mut w).unwrap();
        let overlay = w.measurement_of(&map).unwrap();
        assert_eq!(overlay.measurements, before, "整份还原");
    }

    #[test]
    fn clear_measurements_on_empty_or_missing_overlay_reports_no_dirty() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        // 第一次：根本没 overlay（连 BTreeMap 键都没）→ apply 应记空脏路径。
        let cmd = ClearMeasurementsCommand::new(map.clone());
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(dirty.is_empty(), "原本就空→应不写盘");
        // revert 也应合法（缓存的「None」可还原）。
        cmd.revert(&mut w).unwrap();

        // 第二次：键存在但内部空 → apply 空。revert 不报错。
        w.measurement_of_mut(&map);
        let cmd = ClearMeasurementsCommand::new(map.clone());
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(dirty.is_empty());
        cmd.revert(&mut w).unwrap();
    }

    #[test]
    fn three_measurement_commands_undo_independently() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        let a = AddMeasurementCommand::new(map.clone(), m_line("m1"));
        let b = AddMeasurementCommand::new(map.clone(), m_rect("m2"));
        a.apply(&mut w).unwrap();
        b.apply(&mut w).unwrap();
        assert_eq!(w.measurement_of(&map).unwrap().len(), 2);

        b.revert(&mut w).unwrap();
        assert_eq!(w.measurement_of(&map).unwrap().len(), 1);
        a.revert(&mut w).unwrap();
        // 两个 revert 之后：BTreeMap 键里仍留有该键，但 measurements 为空。
        let overlay = w.measurement_of(&map).unwrap();
        assert!(overlay.is_empty(), "overlay 应留空文档兜底");
    }

    #[test]
    fn measurement_dirty_path_does_not_collide_with_streets_or_maps() {
        // 关键不变量：脏路径不与 streets/、maps/ 共享前缀 → 不应误触发
        // terrain.bin / index.yaml / roads.yaml 重写。
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        AddMeasurementCommand::new(map.clone(), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        let dirty_paths = RemoveMeasurementCommand::new(map.clone(), Id::new_unchecked("m1"))
            .apply(&mut w)
            .unwrap();
        assert!(dirty_paths.iter().all(|p| p.starts_with("measurements/")));
        assert!(!dirty_paths.iter().any(|p| p.starts_with("maps/")));
        assert!(!dirty_paths.iter().any(|p| p.starts_with("streets/")));
    }

    // ── Region 命令测试 ────────────────────────────────────────────────────

    use wv_core::Region;

    fn sample_region(id: &str, name: &str) -> Region {
        Region {
            id: Id::new_unchecked(id),
            name: name.into(),
            map: Id::new_unchecked(TEST_MAP),
            polygon: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
        }
    }

    #[test]
    fn add_region_pushes_to_world_and_reverts() {
        let mut w = World::default();
        let cmd = AddRegionCommand::new(sample_region("r1", "关中平原"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.regions.len(), 1);
        assert_eq!(w.regions[0].id.as_str(), "r1");
        assert!(dirty.iter().any(|p| p == "regions"));

        cmd.revert(&mut w).unwrap();
        assert!(w.regions.is_empty(), "revert 应移除区域");
    }

    #[test]
    fn add_region_rejects_duplicate_id() {
        let mut w = World::default();
        w.regions.push(sample_region("r1", "A"));
        let cmd = AddRegionCommand::new(sample_region("r1", "B"));
        assert!(
            cmd.apply(&mut w).is_err(),
            "重复 ID 应返回 Precondition 错误"
        );
    }

    #[test]
    fn remove_region_removes_and_reverts_to_original_position() {
        let mut w = World::default();
        w.regions.push(sample_region("r0", "先"));
        w.regions.push(sample_region("r1", "目标"));
        w.regions.push(sample_region("r2", "后"));

        let cmd = RemoveRegionCommand::new(Id::new_unchecked("r1"));
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.regions.len(), 2);
        assert_eq!(w.regions[0].id.as_str(), "r0");
        assert_eq!(w.regions[1].id.as_str(), "r2");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.regions.len(), 3);
        assert_eq!(w.regions[1].id.as_str(), "r1", "应恢复到原位置");
        assert_eq!(w.regions[1].name, "目标");
    }

    #[test]
    fn remove_region_on_missing_id_fails() {
        let mut w = World::default();
        let cmd = RemoveRegionCommand::new(Id::new_unchecked("ghost"));
        assert!(cmd.apply(&mut w).is_err(), "不存在的 ID 应报 TargetMissing");
    }

    #[test]
    fn update_region_changes_name_and_polygon_and_reverts() {
        let mut w = World::default();
        w.regions.push(sample_region("r1", "旧名"));

        let cmd = UpdateRegionCommand::new(
            Id::new_unchecked("r1"),
            "新名".into(),
            vec![0.0, 0.0, 5.0, 0.0, 5.0, 5.0],
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.regions[0].name, "新名");
        assert_eq!(w.regions[0].polygon, vec![0.0, 0.0, 5.0, 0.0, 5.0, 5.0]);

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.regions[0].name, "旧名");
        assert_eq!(
            w.regions[0].polygon,
            vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            "revert 应恢复原始 polygon"
        );
    }

    #[test]
    fn update_region_on_missing_id_fails() {
        let mut w = World::default();
        let cmd = UpdateRegionCommand::new(Id::new_unchecked("ghost"), "X".into(), vec![]);
        assert!(cmd.apply(&mut w).is_err());
    }

    #[test]
    fn region_dirty_path_is_regions_prefix() {
        let mut w = World::default();
        let dirty = AddRegionCommand::new(sample_region("r1", "R"))
            .apply(&mut w)
            .unwrap();
        assert!(dirty.iter().all(|p| p.starts_with("regions")));
        assert!(!dirty.iter().any(|p| p.starts_with("maps/")));
        assert!(!dirty.iter().any(|p| p.starts_with("geometry/")));
    }

    // ====== 城市子图命令（ADR-0022 D2 / D5）========================

    fn city_node(
        id: &str,
        parent: Option<&str>,
        outline: Option<GeometryData>,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked(id),
            name: format!("城{id}"),
            level: wv_core::map::MapLevel::City,
            parent: parent.map(wv_core::Id::new_unchecked),
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            outline_polygon: outline,
            ..wv_core::map::MapNode::default()
        }
    }

    fn rect_polygon(w: f32, h: f32) -> GeometryData {
        GeometryData::Polygon(vec![0.0, 0.0, w, 0.0, w, h, 0.0, h])
    }

    #[test]
    fn add_city_pushes_node_and_icon_and_marks_dirty() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 100.0)),
        );
        let cmd = AddCityCommand::new(
            city.clone(),
            wv_core::Id::new_unchecked("icon_city_a"),
            wv_core::map::IconShape::Diamond,
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        // 城市节点 + 父图 icons 都该落地。
        assert_eq!(w.maps.len(), 2, "世界图 + 城市图");
        assert!(
            w.maps
                .iter()
                .any(|m| m.id == wv_core::Id::new_unchecked("map_city_a")),
            "城市子图应在"
        );
        let parent = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("world_main"))
            .expect("父图应在");
        assert_eq!(parent.children.len(), 1, "父图 children 应含城市");
        assert_eq!(parent.icons.len(), 1, "父图 icons 应含钻入图标");
        assert_eq!(
            parent.icons[0].drill_into.as_ref().map(|i| i.as_str()),
            Some("map_city_a"),
            "图标 drill_into 应指向城市"
        );
        // 脏路径：maps + 父图 index + 子图 index
        assert!(dirty.contains(&"maps".to_owned()));
        assert!(dirty.contains(&"maps/world_main/index.yaml".to_owned()));
        assert!(dirty.contains(&"maps/map_city_a/index.yaml".to_owned()));
    }

    #[test]
    fn add_city_revert_removes_node_and_parent_icon() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        let cmd = AddCityCommand::new(
            city_node("map_city_a", Some("world_main"), None),
            wv_core::Id::new_unchecked("icon_city_a"),
            wv_core::map::IconShape::Diamond,
        );
        cmd.apply(&mut w).expect("apply");
        assert_eq!(w.maps.len(), 2);
        cmd.revert(&mut w).expect("revert");
        assert_eq!(w.maps.len(), 1, "revert 应移除城市");
        let parent = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("world_main"))
            .expect("父图应在");
        assert!(parent.children.is_empty(), "父图 children 应清空");
        assert!(parent.icons.is_empty(), "父图 icons 应清空");
    }

    #[test]
    fn add_city_rejects_duplicate_id() {
        let mut w = World::default();
        w.maps
            .push(map_node("map_city_a", None, wv_core::map::MapLevel::World));
        let cmd = AddCityCommand::new(
            city_node("map_city_a", Some("map_city_a"), None),
            wv_core::Id::new_unchecked("icon_dup"),
            wv_core::map::IconShape::Diamond,
        );
        assert!(cmd.apply(&mut w).is_err(), "重复 ID 应 Precondition");
    }

    #[test]
    fn add_city_requires_parent() {
        let mut w = World::default();
        let mut city = city_node("map_orphan", None, None);
        city.parent = None;
        let cmd = AddCityCommand::new(
            city,
            wv_core::Id::new_unchecked("icon_orphan"),
            wv_core::map::IconShape::Diamond,
        );
        assert!(cmd.apply(&mut w).is_err(), "无 parent 应 Precondition");
    }

    #[test]
    fn move_city_translates_origin_and_outline_polygon() {
        let mut w = World::default();
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 200.0)),
        );
        w.maps.push(city.clone());
        let delta = wv_core::geometry::Point::new(10.0, 20.0);
        let cmd = MoveCityCommand::new(wv_core::Id::new_unchecked("map_city_a"), delta);
        cmd.apply(&mut w).expect("apply");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        // extent.origin: None → (10, 20)
        let origin = after.extent.as_ref().unwrap().origin.unwrap();
        assert!(
            (origin.x - 10.0).abs() < 1e-6,
            "x 应 +10，实得 {}",
            origin.x
        );
        assert!(
            (origin.y - 20.0).abs() < 1e-6,
            "y 应 +20，实得 {}",
            origin.y
        );
        // outline_polygon 各顶点平移
        let poly = match after.outline_polygon.as_ref().unwrap() {
            GeometryData::Polygon(v) => v.clone(),
            _ => panic!("outline 应是 Polygon"),
        };
        assert_eq!(
            poly,
            vec![10.0, 20.0, 110.0, 20.0, 110.0, 220.0, 10.0, 220.0]
        );
    }

    #[test]
    fn move_city_revert_restores_origin_and_polygon() {
        let mut w = World::default();
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 200.0)),
        );
        w.maps.push(city.clone());
        let delta = wv_core::geometry::Point::new(10.0, 20.0);
        let cmd = MoveCityCommand::new(wv_core::Id::new_unchecked("map_city_a"), delta);
        cmd.apply(&mut w).expect("apply");
        cmd.revert(&mut w).expect("revert");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        // 初始 extent 由 Extent::sized() 构造（origin = None）；revert 应回 None。
        assert!(
            after.extent.as_ref().unwrap().origin.is_none(),
            "revert 应把 origin 还原为 None（city_node 默认用 Extent::sized）"
        );
        let poly = match after.outline_polygon.as_ref().unwrap() {
            GeometryData::Polygon(v) => v.clone(),
            _ => panic!("outline 应是 Polygon"),
        };
        assert_eq!(poly, vec![0.0, 0.0, 100.0, 0.0, 100.0, 200.0, 0.0, 200.0]);
    }

    #[test]
    fn move_city_no_origin_no_outline_is_noop() {
        let mut w = World::default();
        let mut city = city_node("map_bare", None, None);
        city.extent = None;
        city.outline_polygon = None;
        w.maps.push(city);
        let cmd = MoveCityCommand::new(
            wv_core::Id::new_unchecked("map_bare"),
            wv_core::geometry::Point::new(10.0, 20.0),
        );
        // 既无 extent 又无 outline：apply 不应 panic，应走「无字段可动」通路。
        cmd.apply(&mut w).expect("apply 静默成功");
        cmd.revert(&mut w).expect("revert");
    }

    #[test]
    fn move_city_missing_id_errors() {
        let mut w = World::default();
        let cmd = MoveCityCommand::new(
            wv_core::Id::new_unchecked("ghost"),
            wv_core::geometry::Point::new(1.0, 1.0),
        );
        assert!(cmd.apply(&mut w).is_err(), "不存在应 TargetMissing");
    }

    #[test]
    fn set_city_outline_replaces_polygon_and_reverts_to_old() {
        let mut w = World::default();
        let old_poly = rect_polygon(100.0, 100.0);
        let city = city_node("map_city_a", Some("world_main"), Some(old_poly.clone()));
        w.maps.push(city);
        let new_poly = GeometryData::Polygon(vec![5.0, 5.0, 50.0, 5.0, 50.0, 50.0]);
        let cmd = SetCityOutlineCommand::new(
            wv_core::Id::new_unchecked("map_city_a"),
            Some(new_poly.clone()),
        );
        cmd.apply(&mut w).expect("apply");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        assert_eq!(after.outline_polygon.as_ref().unwrap(), &new_poly);
        cmd.revert(&mut w).expect("revert");
        let reverted = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        assert_eq!(reverted.outline_polygon.as_ref().unwrap(), &old_poly);
    }

    #[test]
    fn set_city_outline_to_none_erases_polygon() {
        let mut w = World::default();
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 100.0)),
        );
        w.maps.push(city);
        let cmd = SetCityOutlineCommand::new(wv_core::Id::new_unchecked("map_city_a"), None);
        cmd.apply(&mut w).expect("apply");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        assert!(after.outline_polygon.is_none(), "擦除后应为 None");
    }

    #[test]
    fn set_city_outline_rejects_too_few_vertices() {
        let mut w = World::default();
        let city = city_node("map_city_a", Some("world_main"), None);
        w.maps.push(city);
        let bad = GeometryData::Polygon(vec![0.0, 0.0, 1.0, 1.0]); // 仅 2 顶点
        let cmd = SetCityOutlineCommand::new(wv_core::Id::new_unchecked("map_city_a"), Some(bad));
        assert!(
            cmd.apply(&mut w).is_err(),
            "<3 顶点的 polygon 应 Precondition 拒绝"
        );
    }

    // --- AddPlaceEdgeCommand 测试 ----------------------------------------

    fn edge(id: &str, a: &str, b: &str) -> wv_core::distance::PlaceEdge {
        wv_core::distance::PlaceEdge {
            id: wv_core::Id::new_unchecked(id),
            a: wv_core::Id::new_unchecked(a),
            b: wv_core::Id::new_unchecked(b),
            distance_km: 12.5,
            terrain: "road".into(),
            bidirectional: true,
        }
    }

    #[test]
    fn add_place_edge_appends_and_marks_dirty() {
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"));
        let dirty = cmd.apply(&mut w).expect("apply");
        assert_eq!(dirty, vec!["distances/edges".to_string()]);
        assert_eq!(w.distances.edges.len(), 1);
        assert_eq!(w.distances.edges[0].id.as_str(), "edge_a_b");
    }

    #[test]
    fn add_place_edge_rejects_duplicate_id() {
        let mut w = World::default();
        AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"))
            .apply(&mut w)
            .expect("first apply");
        let err = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_c", "place_d"))
            .apply(&mut w)
            .expect_err("second apply with same id must fail");
        assert!(matches!(err, EditError::Precondition(_)));
    }

    #[test]
    fn add_place_edge_revert_removes_the_edge() {
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"));
        cmd.apply(&mut w).expect("apply");
        assert_eq!(w.distances.edges.len(), 1);
        cmd.revert(&mut w).expect("revert");
        assert!(w.distances.edges.is_empty());
    }

    #[test]
    fn add_place_edge_revert_target_missing_when_never_applied() {
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"));
        let err = cmd.revert(&mut w).expect_err("revert without apply");
        assert!(matches!(err, EditError::TargetMissing(_)));
    }

    #[test]
    fn add_place_edge_accepts_dangling_endpoints_per_v1() {
        // AGENTS §7.5 V1：悬空引用只提示不阻止——本测试锁住这条契约。
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_orphan", "place_does_not_exist", "place_x"));
        cmd.apply(&mut w).expect("V1 允许悬空 endpoint");
        assert_eq!(w.distances.edges.len(), 1);
    }

    // ---- 大陆生成宏命令（大陆生成 UI plan Task 1） ----

    fn continent_envelope() -> Vec<Vec<(f32, f32)>> {
        vec![vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)]]
    }

    #[test]
    fn generate_continent_apply_then_undo_restores_grid_cell_for_cell() {
        let mut world = world_with_terrain(48);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();
        let cmd = GenerateContinentCommand::new(
            test_map(),
            continent_envelope(),
            ContinentParams::default(),
            7,
            1.0,
        );
        let dirty = cmd.apply(&mut world).unwrap();
        assert_eq!(dirty, vec!["maps/world_main/terrain.bin".to_string()]);
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        assert_ne!(before_snapshot, after, "apply 应改变地形");
        assert!(
            after.cells.iter().any(|c| c.height > 0),
            "方形包络内应长出陆地"
        );

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "撤销应逐格复原"
        );
    }

    #[test]
    fn generate_continent_missing_terrain_reports_mismatch_not_panic() {
        let mut world = World::default();
        let cmd = GenerateContinentCommand::new(
            Id::new_unchecked("world_main"),
            continent_envelope(),
            ContinentParams::default(),
            7,
            1.0,
        );
        assert!(matches!(
            cmd.apply(&mut world),
            Err(EditError::TerrainMismatch(_))
        ));
    }

    #[test]
    fn generate_continent_empty_envelopes_is_a_noop_command() {
        let mut world = world_with_terrain(32);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();
        let cmd =
            GenerateContinentCommand::new(test_map(), vec![], ContinentParams::default(), 7, 1.0);
        cmd.apply(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "空包络不得改动网格"
        );
        cmd.revert(&mut world).expect("空命令的 undo 不得炸");
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot
        );
    }

    // ── 事实地点 / 地界命令测试（theme overlay）────────────────────────────
    // ── 事实地点 / 地界命令测试（theme overlay）────────────────────────────

    fn feature_place(id: &str) -> wv_core::distance::Place {
        wv_core::distance::Place {
            id: Id::new_unchecked(id),
            name: format!("地点{id}"),
            feature: Some(5),
            biome: None,
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
        }
    }

    fn territory(
        map: &str,
        owner: &str,
        from: i64,
        to: Option<i64>,
        ring: Vec<f32>,
    ) -> wv_core::map::GeometrySpan {
        wv_core::map::GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: TimeSpan { from, to },
            geometry: GeometryData::Polygon(ring),
            relation: None,
        }
    }

    // ---- 生物群系标记（ADR-0029 / 命名段 M1 Task 3） ----

    fn marker_place(id: &str, name: &str, biome: u8) -> wv_core::distance::Place {
        wv_core::distance::Place {
            id: wv_core::Id::new_unchecked(id),
            name: name.into(),
            feature: None,
            biome: Some(biome),
            envelope: None,
            map: Some(test_map()),
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
        }
    }

    fn land_world(size: u32) -> World {
        let mut w = world_with_terrain(size);
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            };
            (size * size) as usize
        ];
        w.set_terrain(
            test_map(),
            TerrainData {
                width: size,
                height: size,
                cells: land,
            },
        );
        w
    }

    #[test]
    fn marker_command_generates_cells_place_and_span_atomically() {
        let mut w = land_world(64);
        // 草原格（biome 3）换成森林（1）：低于识别阈值 → 生成。
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        let dirty = cmd.apply(&mut w).expect("标记应成功");
        assert!(dirty.contains(&"distances/places".to_string()));
        assert!(
            dirty.iter().any(|d| d.starts_with("maps/")),
            "写格子必须报地形脏路径：{dirty:?}"
        );
        // 格子：包络内变森林。
        let t = w.terrain_of(&test_map()).expect("地形");
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "包络内应生成森林");
        // Place：biome + envelope 落库，锚点已拟合。
        let p = w
            .distances
            .places
            .iter()
            .find(|p| p.id == wv_core::Id::new_unchecked("place_dark_forest"))
            .expect("Place 应入场");
        assert_eq!(p.biome, Some(1));
        assert_eq!(p.envelope.as_ref().map(|e| e.len()), Some(8));
        assert!(p.x.is_some() && p.w.is_some(), "锚点应从生成边界拟合");
        // 边界 span：外环在包络附近。
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == wv_core::Id::new_unchecked("place_dark_forest"))
            .expect("边界应入场");
        let GeometryData::Polygon(ring) = &span.geometry else {
            panic!("v1 边界应为单环，实得 {:?}", span.geometry);
        };
        let xs: Vec<f32> = ring.chunks_exact(2).map(|c| c[0]).collect();
        let min_x = xs.iter().cloned().fold(f32::MAX, f32::min);
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            (9.0..=11.0).contains(&min_x) && (29.0..=31.0).contains(&max_x),
            "边界应贴包络 10..30 wu，实得 {min_x}..{max_x}"
        );
    }

    #[test]
    fn marker_command_identify_leaves_cells_bitwise() {
        let mut w = land_world(64);
        // 先铺一片森林（>阈值），再按森林标记 → 识别，不写格子。
        let mut cells = w.terrain_of(&test_map()).unwrap().cells.clone();
        for y in 10..30 {
            for x in 10..30 {
                cells[y * 64 + x].biome = 1;
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let before = w.terrain_of(&test_map()).cloned().unwrap();
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(8.0, 8.0), (34.0, 8.0), (34.0, 34.0), (8.0, 34.0)],
        );
        cmd.apply(&mut w).expect("识别应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            before,
            "识别分支不得改动地形"
        );
    }

    #[test]
    fn marker_command_undo_restores_cells_place_and_span_bitwise() {
        let mut w = land_world(64);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        cmd.apply(&mut w).unwrap();
        cmd.revert(&mut w).unwrap();
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_before,
            "revert 应逐位还原地形"
        );
        assert!(w.distances.places.is_empty(), "revert 应移除 Place");
        assert!(w.place_geometry.is_empty(), "revert 应移除边界");
    }

    #[test]
    fn marker_command_at_real_cell_size_uses_grid_units() {
        // cell_size=8：包络 80..320 wu = 格 10..40。若单位换算缺失，
        // 包络会被当成格 80..320（越界）→ 空区域错误。
        let mut w = land_world(64);
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            8.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(80.0, 80.0), (320.0, 80.0), (320.0, 320.0), (80.0, 320.0)],
        );
        cmd.apply(&mut w).expect("cell_size=8 的标记应成功");
        let t = w.terrain_of(&test_map()).unwrap();
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "格 (20,20) 应被标记");
        assert_eq!(t.cells[2 * 64 + 2].biome, 3, "包络外格不应被动");
    }

    // ---- 标记重生成 M2（feat/marker-regen Task 1/4） ----

    fn map_with_extent(id: &str, extent: Option<(f32, f32)>) -> wv_core::map::MapNode {
        let mut n = map_node(id, None, wv_core::map::MapLevel::World);
        n.extent = extent.map(|(w, h)| wv_core::map::Extent::sized(w, h));
        n
    }

    #[test]
    fn marker_cell_size_derives_from_map_extent_over_raster_width() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((512.0, 512.0))));
        let cs = marker_cell_size(&w, &test_map()).expect("有地形就应推出格尺寸");
        assert!(
            (cs - 8.0).abs() < 1e-6,
            "extent 512 / 栅格 64 格应得 8.0 wu/格，实得 {cs}"
        );
    }

    #[test]
    fn marker_cell_size_falls_back_to_root_extent_then_default() {
        // 目标图未声明 extent → 根图 extent 兜底；再无 → 4096（grid_sync 同链）。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, None));
        w.maps
            .push(map_with_extent("root_map", Some((2048.0, 2048.0))));
        w.meta = Some(wv_core::WorldMeta {
            name: "t".into(),
            version: "1".into(),
            schema_version: 2,
            root_map: Id::new_unchecked("root_map"),
            timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
        });
        let cs = marker_cell_size(&w, &test_map()).expect("应回退根图 extent");
        assert!(
            (cs - 32.0).abs() < 1e-6,
            "根图 2048 / 64 格应得 32.0，实得 {cs}"
        );

        // 连根图也没声明 → 4096 / 64 = 64。
        let mut bare = land_world(64);
        bare.maps.push(map_with_extent(TEST_MAP, None));
        let cs = marker_cell_size(&bare, &test_map()).expect("无 extent 应回退 4096");
        assert!(
            (cs - 64.0).abs() < 1e-6,
            "缺省 4096 / 64 格应得 64.0，实得 {cs}"
        );
    }

    #[test]
    fn marker_cell_size_is_none_without_terrain() {
        let w = World::default();
        assert!(
            marker_cell_size(&w, &test_map()).is_none(),
            "无地形栅格的图不该有格尺寸（调用方应拒绝重生成）"
        );
    }

    #[test]
    fn marker_command_stores_holes_when_generated_region_encloses_water() {
        // 陆群系包络套住一片水：生成分支掩膜带洞（水域格不写）→ 几何存洞。
        let mut w = land_world(64);
        let mut cells = w.terrain_of(&test_map()).unwrap().cells.clone();
        for y in 24..40 {
            for x in 24..40 {
                cells[y * 64 + x] = TerrainCellData {
                    height: -20,
                    surface: 0,
                    biome: 0,
                };
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_ring_woods", "环湖林地", 1),
            vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
        );
        cmd.apply(&mut w).expect("生成应成功");
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("place_ring_woods"))
            .expect("边界应入场");
        let GeometryData::PolygonWithHoles { outer, holes } = &span.geometry else {
            panic!("包络套水应存带洞几何，实得 {:?}", span.geometry);
        };
        assert!(outer.len() >= 6, "外环应成环，实得 {outer:?}");
        assert!(!holes.is_empty(), "水域芯应至少成一个洞环，实得 {holes:?}");
        // 洞应贴着水域块 24..40 格（cell_size=1 → 24..40 wu，±1 格容差）。
        let hx: Vec<f32> = holes[0].chunks_exact(2).map(|c| c[0]).collect();
        let (min_x, max_x) = (
            hx.iter().cloned().fold(f32::MAX, f32::min),
            hx.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(
            (23.0..=25.0).contains(&min_x) && (39.0..=41.0).contains(&max_x),
            "洞应贴水域 24..40 wu，实得 {min_x}..{max_x}"
        );
    }

    #[test]
    fn marker_command_identify_stores_holes_of_matched_mask() {
        // 森林环 + 草地芯：识别分支的命中掩膜同样是环 → 边界带洞，地形逐位不动。
        let mut w = land_world(64);
        let mut cells = w.terrain_of(&test_map()).unwrap().cells.clone();
        for y in 10..30 {
            for x in 10..30 {
                if (16..24).contains(&x) && (16..24).contains(&y) {
                    continue; // 草地芯
                }
                cells[y * 64 + x].biome = 1;
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let before = w.terrain_of(&test_map()).cloned().unwrap();
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_hollow_forest", "空心森林", 1),
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        cmd.apply(&mut w).expect("识别应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            before,
            "识别分支不得改动地形"
        );
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("place_hollow_forest"))
            .expect("边界应入场");
        let GeometryData::PolygonWithHoles { holes, .. } = &span.geometry else {
            panic!("草地芯应使识别边界带洞，实得 {:?}", span.geometry);
        };
        assert!(!holes.is_empty(), "草地芯应至少成一个洞环，实得 {holes:?}");
    }

    // ---- 重生成命令（M2 Task 4） ----

    fn create_marker(
        w: &mut World,
        id: &str,
        cell_size: f32,
        envelope: Vec<(f32, f32)>,
    ) -> wv_core::Id {
        let place_id = Id::new_unchecked(id);
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            cell_size,
            5,
            marker_place(id, id, 1),
            envelope,
        );
        cmd.apply(w).expect("创建标记应成功");
        place_id
    }

    fn set_biome_block(w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32, biome: u8) {
        let mut data = w.terrain_of(&test_map()).cloned().unwrap();
        for y in y0..y1 {
            for x in x0..x1 {
                data.cells[(y * data.width as i32 + x) as usize].biome = biome;
            }
        }
        w.set_terrain(test_map(), data);
    }

    fn span_ring_of(w: &World, owner: &Id) -> Vec<f32> {
        let span = w
            .place_geometry
            .iter()
            .find(|g| &g.owner == owner)
            .unwrap_or_else(|| panic!("标记 {owner} 应有边界 span"));
        match &span.geometry {
            GeometryData::Polygon(ring) => ring.clone(),
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            other => panic!("意外的几何形态：{other:?}"),
        }
    }

    #[test]
    fn regenerate_rewrites_stale_boundary_after_brush_without_touching_terrain() {
        // ②主场景：笔刷吃掉半片森林 → 重生成走识别分支，边界收缩、地形逐位不动。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        // 笔刷把 x ≥ 20 的森林改回草原（模拟作者手改 / 后续涂刷）。
        set_biome_block(&mut w, 20, 0, 64, 64, 3);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();

        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("重生成应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_before,
            "识别分支的重生成不得改动地形格子"
        );
        // span 替换且只剩一条，from 取重生成日。
        let spans: Vec<&GeometrySpan> = w.place_geometry.iter().filter(|g| g.owner == id).collect();
        assert_eq!(spans.len(), 1, "重生成后 owner 名下应恰好 1 条 span");
        assert_eq!(spans[0].span.from, 9, "新 span.from 应取重生成日");
        // 边界收缩到剩余森林（≈8..20 wu）。
        let ring = span_ring_of(&w, &id);
        let xs: Vec<f32> = ring.chunks_exact(2).map(|c| c[0]).collect();
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            (18.0..=23.0).contains(&max_x),
            "边界应收缩到剩余森林（max_x≈20），实得 {max_x}"
        );
        // 锚点随新边界移动（原 8..32 中点 20 → 剩余 8..20 中点 ≈14）。
        let place = w.distances.places.iter().find(|p| p.id == id).unwrap();
        let x = place.x.expect("锚点应已重拟合");
        assert!(
            (x - 14.0).abs() < 3.0,
            "锚点应随收缩后的边界重拟合（≈14），实得 {x}"
        );
    }

    #[test]
    fn regenerate_generated_branch_rewrites_cells_and_undo_restores_bitwise() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        // 清空包络内森林 → 重生成必须走生成分支重新写格。
        set_biome_block(&mut w, 10, 10, 30, 30, 3);
        let terrain_cleared = w.terrain_of(&test_map()).cloned().unwrap();
        let (anchor_before, span_before) = {
            let p = w.distances.places.iter().find(|p| p.id == id).unwrap();
            (p.x, w.place_geometry.clone())
        };

        let cmd = RegenerateBiomeMarkerCommand::new(id.clone(), 9);
        let dirty = cmd.apply(&mut w).expect("生成分支重生成应成功");
        assert!(
            dirty.contains(&format!("maps/{}/terrain.bin", test_map())),
            "写格子必须报地形脏路径：{dirty:?}"
        );
        let t = w.terrain_of(&test_map()).unwrap();
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "包络内应重新生成森林");
        assert_eq!(t.cells[2 * 64 + 2].biome, 3, "包络外格不应被动");

        cmd.revert(&mut w).expect("revert 应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_cleared,
            "revert 应逐位还原到 apply 前的（已清空）地形"
        );
        let p = w.distances.places.iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.x, anchor_before, "revert 应还原锚点");
        assert_eq!(w.place_geometry, span_before, "revert 应逐条还原 span 列表");
    }

    #[test]
    fn regenerate_replaces_span_instead_of_appending() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        // 裸实例（未 apply）revert 必须无操作：不抹锚点、不动 span。
        RegenerateBiomeMarkerCommand::new(id.clone(), 99)
            .revert(&mut w)
            .expect("未 apply 的 revert 应无操作成功");
        assert!(
            w.place_geometry.iter().any(|g| g.owner == id),
            "无操作 revert 不得动 span"
        );
        assert!(
            w.distances
                .places
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .x
                .is_some(),
            "无操作 revert 不得抹锚点"
        );

        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("第一次重生成");
        RegenerateBiomeMarkerCommand::new(id.clone(), 12)
            .apply(&mut w)
            .expect("第二次重生成");
        let spans: Vec<&GeometrySpan> = w.place_geometry.iter().filter(|g| g.owner == id).collect();
        assert_eq!(
            spans.len(),
            1,
            "两次重生成后 owner 名下仍应恰好 1 条 span（R2 替换），实得 {}",
            spans.len()
        );
        assert_eq!(spans[0].span.from, 12, "最新 span.from 应取第二次重生成日");
    }

    #[test]
    fn regenerate_uses_raster_derived_cell_size() {
        // extent 512 / 栅格 64 格 → 推导 cell_size=8；包络 80..320 wu = 格 10..40。
        // 若格尺寸漂移（如按 1.0 算），包络会越界成 80..320 格 → 空区域错误。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((512.0, 512.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            8.0,
            vec![(80.0, 80.0), (320.0, 80.0), (320.0, 320.0), (80.0, 320.0)],
        );
        set_biome_block(&mut w, 10, 10, 40, 40, 3);
        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("重生成应按 raster 推导的格尺寸成功（C1 回归）");
        let t = w.terrain_of(&test_map()).unwrap();
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "格 (20,20) 应重新生成森林");
        assert_eq!(t.cells[5 * 64 + 5].biome, 3, "包络外格不应被动");
    }

    #[test]
    fn regenerate_errors_without_biome_or_envelope() {
        let mut w = land_world(64);
        let mut p = marker_place("place_plain", "普通地点", 1);
        p.biome = None;
        p.map = Some(test_map());
        w.distances.places.push(p);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_plain"), 9)
            .apply(&mut w)
            .expect_err("缺 biome 的地点不是标记");
        assert!(
            matches!(err, EditError::Precondition(_)),
            "应为前置条件错误，实得 {err:?}"
        );
        assert_eq!(w.terrain_of(&test_map()).cloned().unwrap(), terrain_before);

        w.distances.places[0].biome = Some(1);
        w.distances.places[0].envelope = None;
        let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_plain"), 9)
            .apply(&mut w)
            .expect_err("缺 envelope 无法重生成");
        assert!(
            matches!(err, EditError::Precondition(_)),
            "应为前置条件错误，实得 {err:?}"
        );
        assert!(w.place_geometry.is_empty(), "失败不得写入任何 span");

        let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_ghost"), 9)
            .apply(&mut w)
            .expect_err("不存在的地点应报 TargetMissing");
        assert!(matches!(err, EditError::TargetMissing(_)), "实得 {err:?}");
    }

    #[test]
    fn regenerate_errors_when_envelope_now_all_water_leaving_world_untouched() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        // 沧海桑田：包络整体沉成海（陆群系无处可写 → EmptyRegion）。
        let mut data = w.terrain_of(&test_map()).cloned().unwrap();
        for y in 10..30 {
            for x in 10..30 {
                data.cells[y * 64 + x] = TerrainCellData {
                    height: -30,
                    surface: 0,
                    biome: 0,
                };
            }
        }
        w.set_terrain(test_map(), data);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        let spans_before = w.place_geometry.clone();
        let places_before = w.distances.places.clone();

        let err = RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect_err("全水包络 + 陆群系应空区域失败");
        assert!(matches!(err, EditError::Precondition(_)), "实得 {err:?}");
        assert_eq!(w.terrain_of(&test_map()).cloned().unwrap(), terrain_before);
        assert_eq!(w.place_geometry, spans_before, "失败不得改动 span");
        assert_eq!(w.distances.places, places_before, "失败不得改动地点");
    }

    #[test]
    fn regenerate_stores_holes_as_polygon_with_holes() {
        // 生成时是实心林块；后来中间挖出湖 → 重生成识别分支命中掩膜成环 → 带洞。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
        );
        let mut data = w.terrain_of(&test_map()).cloned().unwrap();
        for y in 24..40 {
            for x in 24..40 {
                data.cells[y * 64 + x] = TerrainCellData {
                    height: -20,
                    surface: 0,
                    biome: 0,
                };
            }
        }
        w.set_terrain(test_map(), data);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("重生成应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_before,
            "识别分支不得改动地形"
        );
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == id)
            .expect("span 应存在");
        let GeometryData::PolygonWithHoles { holes, .. } = &span.geometry else {
            panic!("湖芯应使重生成边界带洞，实得 {:?}", span.geometry);
        };
        assert!(!holes.is_empty(), "应至少一个洞环，实得 {holes:?}");
    }

    #[test]
    fn add_feature_place_sets_anchors_from_ring_and_revert_removes_both() {
        let mut w = World::default();
        let ring = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let cmd = AddFeaturePlaceCommand::new(
            feature_place("place_forest"),
            territory(TEST_MAP, "place_forest", 0, None, ring.clone()),
        );

        let dirty = cmd.apply(&mut w).expect("apply");
        assert_eq!(
            dirty,
            vec!["distances/places".to_string(), "geometry/place".to_string()],
            "脏路径应同时覆盖地点表与地界目录"
        );
        assert_eq!(w.distances.places.len(), 1, "应写入 1 个地点");
        assert_eq!(w.place_geometry.len(), 1, "应写入 1 条地界");
        let p = &w.distances.places[0];
        assert_eq!(p.x, Some(5.0), "质心 X 应为 5，实际 {:?}", p.x);
        assert_eq!(p.y, Some(5.0), "质心 Y 应为 5，实际 {:?}", p.y);
        assert_eq!(p.w, Some(10.0), "包围盒宽应为 10，实际 {:?}", p.w);
        assert_eq!(p.h, Some(10.0), "包围盒高应为 10，实际 {:?}", p.h);
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(ring),
            "地界几何应原样写入"
        );

        cmd.revert(&mut w).expect("revert");
        assert!(w.distances.places.is_empty(), "revert 应移除地点");
        assert!(w.place_geometry.is_empty(), "revert 应移除地界");
    }

    #[test]
    fn add_feature_place_rejects_duplicate_id() {
        let mut w = World::default();
        w.distances.places.push(feature_place("place_forest"));
        let cmd = AddFeaturePlaceCommand::new(
            feature_place("place_forest"),
            territory(
                TEST_MAP,
                "place_forest",
                0,
                None,
                vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
            ),
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "重复 ID 应返回 Precondition"
        );
        assert_eq!(w.distances.places.len(), 1, "失败的 apply 不应写入地点");
        assert!(w.place_geometry.is_empty(), "失败的 apply 不应写入地界");
    }

    #[test]
    fn update_place_territory_replaces_ring_and_resyncs_anchors() {
        let mut w = World::default();
        let old_ring = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        AddFeaturePlaceCommand::new(
            feature_place("place_forest"),
            territory(TEST_MAP, "place_forest", 0, None, old_ring.clone()),
        )
        .apply(&mut w)
        .expect("prepare");
        let old_place = w.distances.places[0].clone();

        let new_ring = vec![0.0, 0.0, 20.0, 0.0, 20.0, 10.0, 0.0, 10.0];
        let cmd = UpdatePlaceTerritoryCommand::new(
            Id::new_unchecked("place_forest"),
            test_map(),
            0,
            new_ring.clone(),
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        assert_eq!(
            dirty,
            vec!["distances/places".to_string(), "geometry/place".to_string()],
            "脏路径应同时覆盖地点表与地界目录"
        );
        assert_eq!(w.place_geometry.len(), 1, "改地界不得追加 span");
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(new_ring),
            "地界应替换为新环"
        );
        let p = &w.distances.places[0];
        assert_eq!(p.x, Some(10.0), "新质心 X 应为 10，实际 {:?}", p.x);
        assert_eq!(p.y, Some(5.0), "新质心 Y 应为 5，实际 {:?}", p.y);
        assert_eq!(p.w, Some(20.0), "新宽应为 20，实际 {:?}", p.w);
        assert_eq!(p.h, Some(10.0), "新高应为 10，实际 {:?}", p.h);

        cmd.revert(&mut w).expect("revert");
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(old_ring),
            "revert 应恢复旧环"
        );
        assert_eq!(
            w.distances.places[0], old_place,
            "revert 应恢复 Place 全字段（含旧锚点 x/y/w/h）"
        );
    }

    #[test]
    fn update_place_territory_missing_span_is_an_error() {
        let mut w = World::default();
        AddFeaturePlaceCommand::new(
            feature_place("place_forest"),
            territory(
                TEST_MAP,
                "place_forest",
                0,
                None,
                vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            ),
        )
        .apply(&mut w)
        .expect("prepare");

        let cmd = UpdatePlaceTerritoryCommand::new(
            Id::new_unchecked("place_forest"),
            test_map(),
            99,
            vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "不存在的 span 应返回 TargetMissing"
        );
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]),
            "失败的 apply 不应改动地界"
        );
        assert_eq!(
            w.distances.places[0].x,
            Some(5.0),
            "失败的 apply 不应改动锚点"
        );
    }
}
