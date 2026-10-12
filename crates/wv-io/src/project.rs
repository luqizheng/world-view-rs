//! 工程目录的加载与保存。
//!
//! 目录布局见项目文档 §6。本模块只实现**阶段一**所需的最小闭环：
//! 读 `world.yaml` → 解析元数据 → 校验目录存在性 → 打印摘要。
//! 完整加载（地图/人物/事件/距离）按里程碑逐步补齐。

use std::path::{Path, PathBuf};

use wv_core::time::TimePoint;
use wv_core::{World, WorldMeta};

use crate::diagnostics::{derive_hints, Diagnostics};
use crate::error::{IoError, IoResult};

/// 把 `Vec<T>` 写到一个目录下：每个实体一个 `<id>.yaml`。
///
/// **删除传播**（AGENTS §7.5V1 的镜像应用）：扫一遍目录中所有 `.yaml`，
/// 凡是不在 `items` 里的——视为「被命令删了」，移除文件。
/// 这保证 `load(save(w))` round-trip 与 `RemoveFactionCommand` 协同正确。
///
/// 若 `items` 为空且目录本身也不存在，**不**创建空目录（V1：作者没动就别替他造东西）。
///
/// `key` 给实体的"文件 id"——对 `Faction` 等有 `id` 字段的实体，直接返回；
/// 对 `GeometrySpan` 这种没 `id` 的，由调用点用 `map+owner+from` 合成。
/// **v0.1 妥协**：合成 id 在 IO 层算，不污染领域模型（§4.5 不要求几何有 id）。
fn save_entities<T, F>(dir: &Path, items: &[T], key: F) -> IoResult<()>
where
    T: serde::Serialize,
    F: Fn(&T) -> String,
{
    // 收集 ground truth 的 id 集合。
    let keep: std::collections::HashSet<String> = items.iter().map(&key).collect();
    // 写每个实体的 YAML。
    if !items.is_empty() {
        std::fs::create_dir_all(dir)?;
    }
    for item in items {
        let id = key(item);
        let path = dir.join(format!("{id}.yaml"));
        let text = serde_yaml::to_string(item).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })?;
        std::fs::write(&path, text)?;
    }
    // 删除传播：扫目录删不在 keep 里的 yaml。
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("yaml") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|x| x.to_str()) else {
                continue;
            };
            if !keep.contains(name) {
                std::fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

/// 写地图节点：每个 `MapNode` 写到 `maps/<id>/index.yaml`。
///
/// 地图天然有子目录（约定），与 `save_entities` 路径形态不同——单独一个函数。
fn save_maps(dir: &Path, maps: &[wv_core::map::MapNode]) -> IoResult<()> {
    if maps.is_empty() {
        // 删除传播：空 maps 列表 → 清空 maps/ 下所有子目录的 index.yaml。
        // v0.1 简化：不主动 rm 子目录，留给作者手动。
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    for m in maps {
        let map_dir = dir.join(m.id.as_str());
        std::fs::create_dir_all(&map_dir)?;
        let path = map_dir.join("index.yaml");
        let text = serde_yaml::to_string(m).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })?;
        std::fs::write(&path, text)?;
    }
    Ok(())
}

/// 写工程元数据 `world.yaml`（阶段七 v0.4）。
///
/// `world.yaml` 是工程的**头部文件**——不写它，整个保存链路就是跛的：
/// - `Project::open` 需要它才能定位元数据；
/// - `root_map` 与 `timeline_range` 都在这里声明。
///
/// 因此本函数**不**走 dirty 路由：写 `world.yaml` 永远便宜（一个 YAML，
/// 几十字节），任何编辑最终都要走到这一步。元数据本身的更新由上游命令
/// （`EditCommand`）触发 `dirty_set` 累计；写盘时**总是**写。
///
/// 写完后必须**同步** `Project.meta`，否则同进程内的下一次 `open` 仍
/// 看到旧值（虽然 `Project::open` 是 IO 的入口，但应用层可能基于
/// `self.meta()` 做断言）。
fn save_meta(root: &Path, meta: &WorldMeta) -> IoResult<()> {
    let path = root.join("world.yaml");
    let text = serde_yaml::to_string(meta).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}

/// 阶段七 v0.3：若 `terrain` 存在，写 `maps/<root_map>/terrain.bin`，并把
/// 对应的 [`RasterRef`] 写回 root_map 节点的 `raster` 字段（后续 `save_maps`
/// 把它序列化进 `index.yaml`）。
///
/// 行为约束（与 `AGENTS.md` §5/§7.5 一致）：
/// - `terrain = None` → **不** 触碰磁盘上的 .bin，也不修改 maps 列表（V1 守约：
///   作者可能手改 .bin，工具不擅自删）；
/// - root_map 不在 `maps` 里（异常状态）→ 跳过栅格写、maps 列表原样返回（让
///   `save_maps` 报错而不是让本函数先 panic）；
/// - root_map 存在但其原 `raster = None` → 新 `RasterRef` 替换进去（首次落盘）。
fn write_terrain_if_any(
    project_root: &Path,
    maps: &[wv_core::map::MapNode],
    terrain: Option<&wv_core::terrain::TerrainData>,
    root_map_id: Option<&wv_core::Id>,
) -> IoResult<Vec<wv_core::map::MapNode>> {
    let Some(data) = terrain else {
        return Ok(maps.to_vec());
    };
    let Some(root_id) = root_map_id else {
        // 没 meta 就别瞎写——`save_world` 通常伴随 meta，没有就走默认路径。
        return Ok(maps.to_vec());
    };
    // 写 .bin 并得到新的 RasterRef（自带校验和 + 尺寸）。
    let new_raster = crate::terrain_bin::write_terrain(project_root, root_id.as_str(), data)?;
    // 在 maps 列表中找到 root_map 并替换 raster 字段。
    let mut out: Vec<wv_core::map::MapNode> = maps.to_vec();
    if let Some(slot) = out.iter_mut().find(|m| m.id == *root_id) {
        slot.raster = Some(new_raster);
    } else {
        // 异常：meta 指向不存在的 map。沿用 V1 守约：写盘已发生，但 index.yaml
        // 拿不到新 raster。作者重开工程时会看到「meta 与 maps 不一致」的诊断。
        // 不在这里主动删 .bin。
    }
    Ok(out)
}

/// 写 `PlaceEdge` 列表到单文件 `distances/edges.yaml`。
///
/// PlaceEdge 当前没有 `id` 字段，暂存单文件；等引入 id 后切到 `save_entities` 模式。
fn save_placedges(path: &Path, edges: &[wv_core::distance::PlaceEdge]) -> IoResult<()> {
    if edges.is_empty() {
        // 边缘情形：作者把全部 edge 删了 → 删文件。
        if path.is_file() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(edges).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::write(path, text)?;
    Ok(())
}

/// 加载 `maps/<id>/index.yaml` 下的全部地图节点（一层目录扫描）。
///
/// 地图树是多层引用（P4），但每个节点的**索引文件**是扁平的，
/// 因此这里按 `maps/*/index.yaml` 收集即可，父子关系由 `parent` / `children` 字段表达。
fn load_maps(root: &Path) -> IoResult<Vec<wv_core::map::MapNode>> {
    let maps_dir = root.join("maps");
    let mut out = Vec::new();
    if !maps_dir.is_dir() {
        return Ok(out);
    }
    let mut entries: Vec<_> = std::fs::read_dir(&maps_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("index.yaml").is_file())
        .collect();
    entries.sort();
    for dir in entries {
        let idx = dir.join("index.yaml");
        let text = std::fs::read_to_string(&idx)?;
        let node: wv_core::map::MapNode =
            serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
                path: idx.display().to_string(),
                source,
            })?;
        out.push(node);
    }
    Ok(out)
}

/// 检出地图图标里 `drill_into` 指向不存在地图的悬空引用。
///
/// 与 [`crate::situation_io::collect_situations_and_tracks_dangling`] 同一套纪律：
/// **一次性检出全部**，不按遇到第一个就停；结果交给
/// [`Project::diagnose`] 呈现为 Error 级诊断，**不阻断加载**
/// （`AGENTS.md` §7.5V1：校验只提示，不阻止）。
///
/// 为什么单独一个函数而不塞进 situation_io：那个模块管的是「事态/轨迹引用谁」，
/// 而这里管的是「地图图标的钻入目标」，两条链路互不相干，混在一个函数里
/// 会让签名和文档都越来越难读。
pub fn collect_map_icons_dangling(world: &World) -> Vec<wv_core::CoreError> {
    use wv_core::CoreError;

    let map_ids: Vec<wv_core::Id> = world.maps.iter().map(|m| m.id.clone()).collect();
    let has = |ids: &[wv_core::Id], id: &wv_core::Id| ids.iter().any(|x| x == id);

    let mut out = Vec::new();
    for map in &world.maps {
        for icon in &map.icons {
            let Some(target) = &icon.drill_into else {
                continue;
            };
            if !has(&map_ids, target) {
                out.push(CoreError::DanglingReference {
                    referrer: format!("map.{}.icons[{}] (field `drill_into`)", map.id, icon.id),
                    missing: target.to_string(),
                });
            }
        }
    }
    out
}

/// 一个已定位的工程。持有根目录路径与已解析的元数据。
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    meta: WorldMeta,
}

impl Project {
    /// 打开一个工程目录。
    ///
    /// 只读取 `world.yaml` 并做基本校验；不递归加载全部实体。
    /// 这样 `wv load` 在大工程上仍是常数级开销。
    pub fn open(root: impl AsRef<Path>) -> IoResult<Self> {
        let root = root.as_ref().to_path_buf();
        let index = root.join("world.yaml");
        if !index.is_file() {
            return Err(IoError::NotAProject(root.display().to_string()));
        }

        let text = std::fs::read_to_string(&index)?;
        let meta: WorldMeta = serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
            path: index.display().to_string(),
            source,
        })?;

        if meta.schema_version > crate::CURRENT_SCHEMA_VERSION {
            return Err(IoError::SchemaTooNew {
                found: meta.schema_version,
                current: crate::CURRENT_SCHEMA_VERSION,
            });
        }
        // 基线 §2.16：schema_version 1 → 2（时间模型重做），不做自动迁移，给出指引。
        if meta.schema_version < crate::CURRENT_SCHEMA_VERSION {
            return Err(IoError::SchemaTooOld {
                found: meta.schema_version,
                current: crate::CURRENT_SCHEMA_VERSION,
                path: index.display().to_string(),
            });
        }

        Ok(Self { root, meta })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn meta(&self) -> &WorldMeta {
        &self.meta
    }

    /// 加载完整世界。
    ///
    /// 当前范围：元数据 + 时间轴 + 事件 + 人物 + 地图 + 距离图 + 事态 + 单位 + 轨迹。
    ///
    /// ## **加载不做引用把关**（2026-10-11 用户裁定 H5 甲）
    ///
    /// 本方法**只管把数据读进内存**，悬空引用**不会**让它失败——工程一定能打开。
    /// 依据是 `AGENTS.md` §7.5V1「校验只提示，不阻止」：作者的裁定原话是
    /// **「报错但允许存在」**。旧实现在这里`?` 传播悬空引用，
    /// 语义上等于「报错且禁止存在，连看都看不到」——那不是宽容，
    /// 是让作者**没有机会去改**。
    ///
    /// 把关入口是 [`Self::diagnose`]：悬空引用以 `DiagLevel::Error` 级诊断呈现，
    /// 在校验面板里列出，作者可正常编辑、保存。
    pub fn load_world(&self) -> IoResult<World> {
        use crate::situation_io::load_dir;

        let timeline = {
            let path = self.root.join("timeline/axis.yaml");
            if path.is_file() {
                let text = std::fs::read_to_string(&path)?;
                Some(serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
                    path: path.display().to_string(),
                    source,
                })?)
            } else {
                None
            }
        };

        let events: Vec<wv_core::event::Event> = load_dir(&self.root, "timeline/events")?;
        let characters: Vec<wv_core::character::Character> = load_dir(&self.root, "characters")?;
        let situations: Vec<wv_core::situation::Situation> =
            load_dir(&self.root, "timeline/situations")?;
        let tracks: Vec<wv_core::track::Track> = load_dir(&self.root, "tracks")?;
        let units: Vec<wv_core::unit::Unit> = load_dir(&self.root, "units")?;
        let narrations: Vec<wv_core::narration::Narration> =
            load_dir(&self.root, "timeline/narrations")?;
        let maps: Vec<wv_core::map::MapNode> = load_maps(&self.root)?;
        let distances: wv_core::distance::DistanceGraph = {
            // 阶段六 v0.1：`distances/` 目录下的 places / edges / vehicles 各自 YAML。
            // 旧实现只读单文件 places.yaml、把 edges/vehicles 留空，导致
            // `DistanceGraph::shortest_journey` 退化为无路径。三个分量都从同目录
            // 各自读全——加载层不再持有距离维度的「结论」（AGENTS.md §4.2）。
            let places: Vec<wv_core::distance::Place> = load_dir(&self.root, "distances/places")?;
            let edges: Vec<wv_core::distance::PlaceEdge> = load_dir(&self.root, "distances/edges")?;
            let vehicles: Vec<wv_core::distance::Vehicle> =
                load_dir(&self.root, "distances/vehicles")?;
            // 兼容旧布局：单文件 `distances/places.yaml`。新工程一律用目录。
            let (places, edges, vehicles) = if places.is_empty() {
                let path = self.root.join("distances/places.yaml");
                if path.is_file() {
                    let text = std::fs::read_to_string(&path)?;
                    let list: Vec<wv_core::distance::Place> =
                        serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
                            path: path.display().to_string(),
                            source,
                        })?;
                    (list, edges, vehicles)
                } else {
                    (places, edges, vehicles)
                }
            } else {
                (places, edges, vehicles)
            };
            wv_core::distance::DistanceGraph {
                places,
                edges,
                vehicles,
            }
        };

        // 注意：这里**不做**引用完整性把关。校验是 `diagnose()` 的职责，
        // 加载只负责「把数据读进来」。理由见本方法文档。
        //
        // 加载期**仍会失败**的情形仅限「数据压根读不出来」——文件缺失、
        // YAML 语法错误、schema 版本过旧、栅格文件损坏。这些是 I/O 与格式问题，
        // 不是「作者还没写完」，无法靠提示解决。

        // 地形栅格：若地图声明了 raster，加载 terrain.bin。
        let terrain = maps
            .iter()
            .find_map(|m| m.raster.as_ref())
            .map(|r| crate::terrain_bin::read_terrain(&self.root, r))
            .transpose()?;

        // 势力：路线图 S12（2026-10-12）。
        // - `factions/`：每势力一个 YAML（§4.5）。
        // - `geometry/faction/`：每势力几何一个 YAML（`GeometrySpan`）。
        // 两个目录都**可缺失**——S12 是新维度，旧工程直接得空 Vec。
        let factions: Vec<wv_core::Faction> = load_dir(&self.root, "factions")?;
        let faction_geometry: Vec<wv_core::map::GeometrySpan> =
            load_dir(&self.root, "geometry/faction")?;

        Ok(World {
            meta: Some(self.meta.clone()),
            maps,
            timeline,
            characters,
            events,
            relations: Vec::new(),
            distances,
            situations,
            units,
            tracks,
            narrations,
            factions,
            faction_geometry,
            terrain,
        })
    }

    /// 生成人类可读的工程摘要，供 `wv load <dir>` 打印。
    pub fn summary(&self) -> String {
        let m = &self.meta;
        let from = TimePoint::from_day(m.timeline_range.from);
        let to = match m.timeline_range.to {
            Some(t) => TimePoint::from_day(t).to_string(),
            None => "至今".to_string(),
        };
        format!(
            "工程: {name}\n  版本:     {ver}\n  schema:   v{schema}\n  时间轴:   {from} .. {to}\n  根地图:   {root_map}",
            name = m.name,
            ver = m.version,
            schema = m.schema_version,
            from = from,
            to = to,
            root_map = m.root_map,
        )
    }

    /// 把 `World` 写回工程目录。
    ///
    /// 阶段七 v0.1（2026-10-12）：分片落盘，与 [`Self::load_world`] 对称。
    ///
    /// ## 行为
    ///
    /// - **每实体一文件**（§4.5）：每个 `Faction` / `Character` / `Event` /
    ///   `Place` / `PlaceEdge` / `Vehicle` / `Unit` / `Track` / `Situation` /
    ///   `Narration` / `GeometrySpan` 各自一个 `<id>.yaml`。
    /// - **map 节点**仍写 `maps/<id>/index.yaml`（已存在的索引约定）。
    /// - **删除传播**（AGENTS §4.5 + §7.5V1）：以 `World` 为 ground truth——
    ///   磁盘上存在但 `World` 中不含的文件被**删除**（覆盖命令删实体的情况）。
    /// - **不写**`world.yaml`（它由 [`Self::open`] 负责，`meta` 字段不在本方法范围）。
    /// - **不写**`terrain.bin`（栅格二进制是独立流水线；v0.2 再加）。
    /// - 不会写**空目录**——目录缺失就保持缺失（V1：作者没动就别替他造东西）。
    ///
    /// ## dirty 路由
    ///
    /// `dirty_paths` 是 `EditCommand::apply` 返回的目录前缀列表（如
    /// `"factions"`、`"geometry/faction"`）。本方法对每个**非空**目录：
    /// 先**全量重写**该目录下 `World` 的全部实体，再做删除传播。
    /// dirty 目录之外的**不**触碰（作者手改的 YAML 不被覆盖）。
    ///
    /// **为什么不做"只重写脏文件"的细粒度优化**：v0.1 数据规模小（典型工程
    /// < 200 文件），全量重写一个目录的 IO 是亚毫秒级；细粒度需要为每个
    /// 命令记录"它改的是哪几个 ID"——复杂度换不来收益，留作 v0.2。
    ///
    /// ## 失败语义
    ///
    /// 任一文件 IO 失败立即返回错误，**不**做事务回滚——已写的文件保留。
    /// v0.1 数据规模小，原子性不构成实际问题。
    pub fn save_world(&mut self, world: &World, dirty_paths: &[String]) -> IoResult<()> {
        // dirty 路由：未列出的目录**跳过**。注意空 Vec = "全量写"——这对应
        // "第一次落盘"或"开发者手动调 save_world"，区别于"编辑触发的增量写"。
        //
        // **匹配语义**（v0.3 修订）：按**前缀**匹配，让 `maps/<id>/terrain.bin`
        // 这样的具体路径能触发 `maps` 目录的全量重写。`wants("xxx")` ↔ true
        // 当且仅当某个 dirty 路径与本目录**互为前缀**（即同一子树）。
        // 这条规则同时拒绝跨子树的误命中：
        // - `dirty=["factions"]` 不会触发 `wants("characters")`
        // - `dirty=["geometry/faction"]` 不会触发 `wants("factions")`
        let full = dirty_paths.is_empty();
        let wants = |p: &str| {
            full || dirty_paths
                .iter()
                .any(|d| d == p || d.starts_with(p) || p.starts_with(d))
        };

        if wants("factions") {
            save_entities(&self.root.join("factions"), &world.factions, |f| {
                f.id.as_str().to_string()
            })?;
        }
        if wants("geometry/faction") {
            save_entities(
                &self.root.join("geometry/faction"),
                &world.faction_geometry,
                |g| {
                    // 合成 id：map + owner + from。`--` 避开 ID 字符集，文件名
                    // 用这个 key，文件内容里仍是原始字段。
                    format!("{}__{}__{}", g.map.as_str(), g.owner.as_str(), g.span.from)
                },
            )?;
        }
        if wants("characters") {
            save_entities(&self.root.join("characters"), &world.characters, |c| {
                c.id.as_str().to_string()
            })?;
        }
        if wants("timeline/events") {
            save_entities(&self.root.join("timeline/events"), &world.events, |e| {
                e.id.as_str().to_string()
            })?;
        }
        if wants("timeline/situations") {
            save_entities(
                &self.root.join("timeline/situations"),
                &world.situations,
                |s| s.id.as_str().to_string(),
            )?;
        }
        if wants("timeline/narrations") {
            save_entities(
                &self.root.join("timeline/narrations"),
                &world.narrations,
                |n| n.id.as_str().to_string(),
            )?;
        }
        if wants("units") {
            save_entities(&self.root.join("units"), &world.units, |u| {
                u.id.as_str().to_string()
            })?;
        }
        if wants("tracks") {
            save_entities(&self.root.join("tracks"), &world.tracks, |t| {
                t.id.as_str().to_string()
            })?;
        }
        if wants("distances/places") {
            save_entities(
                &self.root.join("distances/places"),
                &world.distances.places,
                |p| p.id.as_str().to_string(),
            )?;
        }
        if wants("distances/edges") {
            // PlaceEdge 无顶层 id——`key` 由 (from, to, terrain) 复合。
            // v0.1 不为它造合成 id，存为单一文件 distances/edges.yaml。
            // 等 PlaceEdge 引入 id 字段再切到每实体一文件。
            save_placedges(
                &self.root.join("distances/edges.yaml"),
                &world.distances.edges,
            )?;
        }
        if wants("distances/vehicles") {
            save_entities(
                &self.root.join("distances/vehicles"),
                &world.distances.vehicles,
                |v| v.id.as_str().to_string(),
            )?;
        }
        if wants("maps") {
            // 阶段七 v0.3：把 `World.terrain` 写成 `maps/<root_map>/terrain.bin`，
            // 并把更新后的 `RasterRef` 写回 root_map 的 `index.yaml`。
            // 若 `world.terrain` 为 None 则不写（V1 守约：不动作者手改的 .bin）。
            // 若 root_map 已在 maps 中找不到（异常状态），跳过栅格。
            let root_map_id = world.meta.as_ref().map(|m| m.root_map.clone());
            let updated_maps = write_terrain_if_any(
                &self.root,
                &world.maps,
                world.terrain.as_ref(),
                root_map_id.as_ref(),
            )?;
            save_maps(&self.root.join("maps"), &updated_maps)?;
        }
        // 阶段七 v0.4：写 `world.yaml`。这条**不**走 dirty 路由——
        // meta 是工程头部，永远写；fall back 到 `self.meta`（处理「刚加载、
        // world.meta 还没被任何命令填回」的边界）。
        let meta_to_write = world.meta.as_ref().unwrap_or(&self.meta);
        save_meta(&self.root, meta_to_write)?;
        // 同步内存里的 Project.meta，避免同进程内对 `self.meta()` 的断言看到旧值。
        self.meta = meta_to_write.clone();
        Ok(())
    }

    /// 校验工程内部结构（**文字摘要**，供 `wv load <dir>` 与命令行用）。
    ///
    /// 只报「目录结构缺失」这类**看得见的目录约定**问题。
    /// **引用完整性不在这里把关** —— 见 [`Self::diagnose`]（那是唯一的把关入口）。
    pub fn validate(&self) -> IoResult<Vec<String>> {
        let expected = ["maps", "timeline", "characters", "relations", "distances"];
        let mut warnings = Vec::new();
        for dir in expected {
            if !self.root.join(dir).exists() {
                warnings.push(format!("缺少预期目录: {dir}/"));
            }
        }
        Ok(warnings)
    }

    /// 结构化诊断——**引用完整性的唯一把关入口**（ADR-0005 + ADR-0006）。
    ///
    /// ## 强度模型（2026-10-11 用户裁定 H5 甲）
    ///
    /// | 级别 | 含义 | 对作者的影响 |
    /// |---|---|---|
    /// | `Error` | 悬空引用（数据真的坏了） | **不阻止**进入 / 编辑 / 保存，只列出 |
    /// | `Hint` | 相遇却未写事件（可能有意的留白） | 只列出，**永不阻止** |
    ///
    /// 「报错但允许存在」是用户原话。因此本方法**唯一的失败情形**是
    /// 「数据压根读不出来」（YAML 语法错、schema 版本过旧、栅格损坏）——
    /// 那是 I/O 问题，不是「作者还没写完」，提示解决不了。
    ///
    /// ## 与 [`Self::validate`] 的分工
    ///
    /// `validate()` 只看目录约定，返回文字；`diagnose()` 看**数据内容**，
    /// 返回结构化条目（带稳定指纹与定位锚点，UI 才能做回访）。
    /// 两者**不再共用**引用校验逻辑——因为把关点只剩这一个，没有分歧的可能。
    pub fn diagnose(&self) -> IoResult<Diagnostics> {
        use crate::situation_io::{collect_situations_and_tracks_dangling, KnownIds};

        let world = self.load_world()?;

        // 引用完整性 → Error 级诊断。**一次性检出全部**，不是遇到第一个就停。
        let event_ids: Vec<wv_core::Id> = world.events.iter().map(|e| e.id.clone()).collect();
        let map_ids: Vec<wv_core::Id> = world.maps.iter().map(|m| m.id.clone()).collect();
        let character_ids: Vec<wv_core::Id> =
            world.characters.iter().map(|c| c.id.clone()).collect();
        let place_ids: Vec<wv_core::Id> = world
            .distances
            .places
            .iter()
            .map(|p| p.id.clone())
            .collect();
        let unit_ids: Vec<wv_core::Id> = world.units.iter().map(|u| u.id.clone()).collect();

        let mut d = Diagnostics::default();
        let mut dangling: Vec<wv_core::CoreError> = collect_situations_and_tracks_dangling(
            &world.situations,
            &world.tracks,
            KnownIds {
                events: &event_ids,
                maps: &map_ids,
                characters: &character_ids,
                places: &place_ids,
                units: &unit_ids,
            },
        );
        // 地图图标的 `drill_into` 悬空（README §12 第 12 条）。
        dangling.extend(collect_map_icons_dangling(&world));
        // 事件 → 旁白引用的悬空（旁白是独立实体，事件只存 ID；2026-01 用户裁定）。
        for e in &world.events {
            for nid in &e.narrations {
                if !world.narrations.iter().any(|n| &n.id == nid) {
                    dangling.push(wv_core::CoreError::DanglingReference {
                        referrer: format!("event.{} (field `narrations`)", e.id),
                        missing: nid.as_str().to_owned(),
                    });
                }
            }
        }
        for err in dangling {
            // 悬空引用的指纹取自错误文案本身——它已含 `referrer` 与 `missing`，
            // 因此同一处悬空在多次诊断间保持同一指纹，可被「已读/已忽略」认领。
            d.items.push(crate::diagnostics::error(
                format!("dangling:{}", err),
                err.to_string(),
            ));
        }

        // 派生提示（相遇却未写事件）——**永不**产生 Error。
        d.extend(derive_hints(&world));
        d.dedup_by_fingerprint();
        Ok(d)
    }
}
