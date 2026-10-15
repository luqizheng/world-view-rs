//! 工程目录的加载与保存。
//!
//! 目录布局见项目文档 §6。本模块只实现**阶段一**所需的最小闭环：
//! 读 `world.yaml` → 解析元数据 → 校验目录存在性 → 打印摘要。
//! 完整加载（地图/人物/事件/距离）按里程碑逐步补齐。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wv_core::time::TimePoint;
use wv_core::{BuildingLayer, LabelLayerConfig, MeasurementOverlay, StreetLayer, World, WorldMeta};

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

/// 写时间轴定义 `timeline/axis.yaml`（阶段七 v0.5）。
///
/// 与 `world.yaml` 同级——是工程的**结构性单文件**，作者手改不会被覆盖
/// （V1 守约镜像）：`timeline = None` 时**不**触碰磁盘上的 axis.yaml。
///
/// **不**走 dirty 路由：与 meta 同理，单文件、永远便宜，编辑触发的
/// `dirty_set` 累计在元数据维度统一管理。
fn save_timeline(root: &Path, timeline: &wv_core::time::Timeline) -> IoResult<()> {
    let path = root.join("timeline").join("axis.yaml");
    let text = serde_yaml::to_string(timeline).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::create_dir_all(root.join("timeline"))?;
    std::fs::write(&path, text)?;
    Ok(())
}

/// 阶段七 v0.3 / Phase 2：把 `World.terrains` 里**每张图各自的**栅格写成
/// `maps/<map_id>/terrain.bin`，并把新的 [`RasterRef`] 写回那张图的 `raster`
/// 字段（后续 `save_maps` 把它序列化进 `index.yaml`）。
///
/// 行为约束（与 `AGENTS.md` §5/§7.5 一致）：
/// - 某张图**没有**栅格 → **不**触碰它的 `.bin`，也不改它的 `raster`（V1 守约：
///   作者可能手改 .bin，工具不擅自删）；
/// - 栅格键指向一张**不存在的图**（异常状态）→ 跳过该键（不写 .bin、不 panic）。
///   这是「meta/maps 与内存不一致」这一类问题，交给校验面板报，不在这里抢答；
/// - 图存在但原 `raster = None` → 新 `RasterRef` 替换进去（首次落盘）。
///
/// **脏路径过滤**（`dirty_paths` / `full`）：只有被判为脏的那张图才重写
/// `.bin`。否则「刷一笔城市地形」会把工程里**所有**城市的地形各 zstd 一遍
/// （4096² 一图 ≈ 50 MB 原始数据），白白拖慢每一次落盘。
fn write_terrains(
    project_root: &Path,
    maps: &[wv_core::map::MapNode],
    terrains: &BTreeMap<wv_core::Id, wv_core::terrain::TerrainData>,
    dirty_paths: &[String],
    full: bool,
) -> IoResult<Vec<wv_core::map::MapNode>> {
    if terrains.is_empty() {
        return Ok(maps.to_vec());
    }
    let mut out: Vec<wv_core::map::MapNode> = maps.to_vec();
    for (map_id, data) in terrains {
        if !terrain_is_dirty(map_id, dirty_paths, full) {
            continue;
        }
        let Some(slot) = out.iter_mut().find(|m| m.id == *map_id) else {
            // 异常：有栅格却没有对应的图节点。不写 .bin，也不动 maps。
            continue;
        };
        // 写 .bin 并得到新的 RasterRef（自带校验和 + 尺寸）。
        let new_raster = crate::terrain_bin::write_terrain(project_root, map_id.as_str(), data)?;
        slot.raster = Some(new_raster);
    }
    Ok(out)
}

/// 这张图的地形栅格这次落盘是否被触及。
///
/// `full`（首次落盘 / 手动全量写，即 `dirty_paths` 为空）→ 全部都要写；
/// 否则按**脏路径前缀**匹配。要求分隔符，避免 `maps/map_city` 误命中
/// `maps/map_city_wuchang/terrain.bin`。
fn terrain_is_dirty(map_id: &wv_core::Id, dirty_paths: &[String], full: bool) -> bool {
    if full {
        return true;
    }
    let prefix = format!("maps/{}", map_id.as_str());
    let sub = format!("{prefix}/");
    dirty_paths
        .iter()
        .any(|d| d == &prefix || d.starts_with(&sub))
}

/// 写**街道图层文档**到各 `Layer.source` 指向的 YAML（ADR-0010 待做第 3 项）。
///
/// 与 [`write_terrains`] 同款脏过滤，但脏路径走**虚拟命名空间**
/// `streets/<layer_id>`：街道文件物理上在 `maps/<map_id>/<source>`，可若把
/// 脏路径也写成这个形态，`terrain_is_dirty` 的 `maps/<map>/` 前缀匹配会把
/// 「画一段街道」误判成「整张图的地形脏了」而白白重写 `.bin`。虚拟路径在
/// 本函数内映射回真实文件；编辑命令只管发 `streets/<layer>`，不感知落盘位置。
///
/// 返回 `maps` 原样（街道几何不往 `index.yaml` 写任何元数据，ADR-0010 决策 ⑤）。
fn write_streets_layers(
    project_root: &Path,
    maps: &[wv_core::map::MapNode],
    streets: &BTreeMap<wv_core::Id, wv_core::StreetLayer>,
    dirty_paths: &[String],
    full: bool,
) -> IoResult<Vec<wv_core::map::MapNode>> {
    if streets.is_empty() {
        return Ok(maps.to_vec());
    }
    for (layer_id, doc) in streets {
        if !street_is_dirty(layer_id, dirty_paths, full) {
            continue;
        }
        // 找到声明了这个图层的图与其 source 路径；找不到就跳过（不写盘、
        // 不报错——「meta/maps 与内存不一致」类问题交给校验面板）。
        let owner = maps.iter().find_map(|m| {
            m.layers
                .iter()
                .find(|l| l.id == *layer_id && l.kind == wv_core::map::LayerKind::Road)
                .and_then(|l| l.source.clone().map(|src| (m.id.clone(), src)))
        });
        let Some((_map_id, rel_path)) = owner else {
            continue;
        };
        self::write_streets_at(project_root, &rel_path, doc)?;
    }
    Ok(maps.to_vec())
}

/// 这个街道图层这次落盘是否被触及。虚拟脏路径精确匹配 `streets/<layer_id>`。
fn street_is_dirty(layer_id: &wv_core::Id, dirty_paths: &[String], full: bool) -> bool {
    if full {
        return true;
    }
    let expected = format!("streets/{}", layer_id.as_str());
    dirty_paths.iter().any(|d| d == &expected)
}

/// [`Project::write_streets`](Project::write_streets) 的自由函数形态（无
/// `&self` 依赖，供上面的落盘聚合复用）。
fn write_streets_at(
    project_root: &Path,
    rel_path: &str,
    doc: &wv_core::StreetLayer,
) -> IoResult<()> {
    let path = project_root.join(rel_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(doc).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}

/// 写**建筑图层文档**到各 `Layer.source` 指向的 YAML（ADR-0016 §6 落地项）。
///
/// 与 [`write_streets_layers`] 完全同构：虚拟脏命名空间 `buildings/<layer_id>`
/// （建筑文件物理上在 `maps/<map_id>/<source>`，虚拟化防止 `maps/<map>/` 前缀
/// 把「改一栋楼」误判成「整图地形脏了」）；返回 `maps` 原样。
fn write_buildings_layers(
    project_root: &Path,
    maps: &[wv_core::map::MapNode],
    buildings: &BTreeMap<wv_core::Id, wv_core::BuildingLayer>,
    dirty_paths: &[String],
    full: bool,
) -> IoResult<Vec<wv_core::map::MapNode>> {
    if buildings.is_empty() {
        return Ok(maps.to_vec());
    }
    for (layer_id, doc) in buildings {
        if !building_is_dirty(layer_id, dirty_paths, full) {
            continue;
        }
        let owner = maps.iter().find_map(|m| {
            m.layers
                .iter()
                .find(|l| l.id == *layer_id && l.kind == wv_core::map::LayerKind::Building)
                .and_then(|l| l.source.clone().map(|src| (m.id.clone(), src)))
        });
        let Some((_map_id, rel_path)) = owner else {
            continue;
        };
        self::write_buildings_at(project_root, &rel_path, doc)?;
    }
    Ok(maps.to_vec())
}

/// 这个建筑图层这次落盘是否被触及。虚拟脏路径精确匹配 `buildings/<layer_id>`。
fn building_is_dirty(layer_id: &wv_core::Id, dirty_paths: &[String], full: bool) -> bool {
    if full {
        return true;
    }
    let expected = format!("buildings/{}", layer_id.as_str());
    dirty_paths.iter().any(|d| d == &expected)
}

/// [`Project::write_buildings`](Project::write_buildings) 的自由函数形态。
fn write_buildings_at(
    project_root: &Path,
    rel_path: &str,
    doc: &wv_core::BuildingLayer,
) -> IoResult<()> {
    let path = project_root.join(rel_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(doc).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}
// ---------------------------------------------------------------------------
// 测量文档（每图一份 `maps/<map_id>/measurements.yaml`，2026-10-14）
// ---------------------------------------------------------------------------

/// 测量文件路径（相对工程根）：`maps/<map_id>/measurements.yaml`。
///
/// **不**像街道那样按 `Layer.source` 派生——测量**没有**图层抽象，
/// 落点就是当前图（按 map_id 走）。这把「地图级辅助产物」与
/// 「图层级产物」清晰区分：街道几何随图层走，测量随地图走。
fn measurement_rel_path(map_id: &wv_core::Id) -> String {
    format!("maps/{}/measurements.yaml", map_id.as_str())
}

/// 加载某张图的测量叠加（文件不存在返空文档）。
///
/// 与 `load_streets` 的差别：街道是「按需读」，测量是「每图都有」——
/// 因此 `not found` **不算错**，直接返 `MeasurementOverlay::default()`，
/// 让空图作者不用手造一个文件。
fn load_measurements_at(project_root: &Path, map_id: &wv_core::Id) -> IoResult<MeasurementOverlay> {
    let path = project_root.join(measurement_rel_path(map_id));
    if !path.is_file() {
        return Ok(MeasurementOverlay::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|source| IoError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;
    serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })
}

/// 写入某张图的测量叠加；空文档**不**写盘（V1：作者没动就别替他造）。
fn write_measurements_at(
    project_root: &Path,
    map_id: &wv_core::Id,
    overlay: &MeasurementOverlay,
) -> IoResult<()> {
    let rel = measurement_rel_path(map_id);
    let path = project_root.join(&rel);
    if overlay.is_empty() {
        // 删除传播：与 `save_entities` 同口径——磁盘文件不应留有
        // 当前 World 不承认的内容。
        if path.is_file() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(overlay).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}

/// 测量这次落盘是否被触及。虚拟脏路径精确匹配 `measurements/<map_id>`。
fn measurement_is_dirty(map_id: &wv_core::Id, dirty_paths: &[String], full: bool) -> bool {
    if full {
        return true;
    }
    let expected = format!("measurements/{}", map_id.as_str());
    dirty_paths.iter().any(|d| d == &expected)
}

/// 落盘聚合：遍历 `world.measurements`，仅写出判为脏的那一份 YAML。
///
/// 选写的图必须有 `MapNode`（即 `world.maps` 中存在该 ID）——否则跳过
/// 「meta/maps 与内存不一致」的报错。**全部为空**时本函数什么都不做
/// （让 `delete_propagation` 在 `save_world` 末尾统一清理）。
fn write_measurements_layers(
    project_root: &Path,
    maps: &[wv_core::map::MapNode],
    measurements: &BTreeMap<wv_core::Id, MeasurementOverlay>,
    dirty_paths: &[String],
    full: bool,
) -> IoResult<()> {
    for (map_id, overlay) in measurements {
        if !measurement_is_dirty(map_id, dirty_paths, full) {
            continue;
        }
        // 找到那张 MapNode 才会写盘——找不到（数据状态不一致）
        // 跳过，不报错。
        if !maps.iter().any(|m| m.id == *map_id) {
            continue;
        }
        write_measurements_at(project_root, map_id, overlay)?;
    }
    Ok(())
}

/// 写 `PlaceEdge` 列表到单文件 `distances/edges.yaml`。
///
/// 读单文件列表（YAML），文件不存在时返回空 `Vec`。
///
/// 用于 `load_world` 的回退路径：阶段七 v0.1 的 `save_placedges` 把 edges
/// 写成单文件 `distances/edges.yaml`，而 `load_dir("distances/edges")` 期待
/// 目录形态——两者不对称会让 save→load 静默丢失所有 edges。loader 显式尝试
/// 单文件回退，把对齐放在**读**而不是**写**侧，避免污染作者的新工程。
///
/// **泛型约束**：`T: serde::de::DeserializeOwned`——所有 `wv_core` 领域模型
/// 都满足。`deserialize` 出错时映射成 `IoError::Yaml`，与 `load_dir` 行为一致。
fn read_single_file_list<T>(path: &Path) -> IoResult<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
        path: path.display().to_string(),
        source,
    })
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

    /// 在空目录（或尚不存在的目录）里**新建**一个最小工程并落盘 `world.yaml`。
    ///
    /// 与 [`Self::open`] 对称：`open` 负责「读已有工程」，`create` 负责
    /// 「白手起家」。编辑器「新建工程 → 首次保存」链路依赖本方法——
    /// 旧实现里新建的空白工程 `project_dir = None`，`Ctrl+S` 直接报
    /// 「没有工程目录」，链路在保存这一环断掉。
    ///
    /// ## 行为
    ///
    /// - 目录不存在则创建（含父目录）；已存在也允许（用户的空文件夹也算）。
    /// - **拒绝覆盖**：若 `world.yaml` 已存在，返回 [`IoError::AlreadyAProject`]
    ///   ——想往已有工程里写，请走 [`Self::open`] + [`Self::save_world`]。
    /// - 元数据取最小合法集：`schema_version` 用当前版本、`root_map` 暂定
    ///   `world`、`timeline_range` 给 `[0, 1]` 占位（作者随后可改）。
    ///
    /// 实体目录**不预建**——`save_world` 的 V1 守约是「作者没动就别替他
    /// 造东西」，首次全量保存会按需创建。
    pub fn create(root: impl AsRef<Path>, name: &str) -> IoResult<Self> {
        let root = root.as_ref().to_path_buf();
        let index = root.join("world.yaml");
        if index.is_file() {
            return Err(IoError::AlreadyAProject(root.display().to_string()));
        }

        std::fs::create_dir_all(&root)?;
        let meta = WorldMeta {
            name: name.to_owned(),
            version: "0.1".to_owned(),
            schema_version: crate::CURRENT_SCHEMA_VERSION,
            root_map: wv_core::Id::new_unchecked("world"),
            timeline_range: wv_core::TimeSpan::new(0, 1).map_err(IoError::Core)?,
        };
        let text = serde_yaml::to_string(&meta).map_err(|source| IoError::Yaml {
            path: index.display().to_string(),
            source,
        })?;
        std::fs::write(&index, text)?;

        Ok(Self { root, meta })
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
            let mut places: Vec<wv_core::distance::Place> =
                load_dir(&self.root, "distances/places")?;
            let mut edges: Vec<wv_core::distance::PlaceEdge> =
                load_dir(&self.root, "distances/edges")?;
            let mut vehicles: Vec<wv_core::distance::Vehicle> =
                load_dir(&self.root, "distances/vehicles")?;
            // 兼容旧布局：单文件 `distances/{places,edges,vehicles}.yaml`。
            // 阶段七 v0.1 的 `save_world` 把 edges 写成单文件（PlaceEdge 暂无
            // id 字段，§4.4 不要求），所以**必须**让 loader 接受单文件回退，
            // 否则 save→load 会**静默丢失**所有 edges。
            // 新工程仍然推荐用目录形态（每实体一个 YAML，与 §4.5 一致）。
            if places.is_empty() {
                places = read_single_file_list(&self.root.join("distances/places.yaml"))?;
            }
            if edges.is_empty() {
                edges = read_single_file_list(&self.root.join("distances/edges.yaml"))?;
            }
            if vehicles.is_empty() {
                vehicles = read_single_file_list(&self.root.join("distances/vehicles.yaml"))?;
            }
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

        // 地形栅格：**每张**声明了 `raster` 的图各读自己的 `terrain.bin`。
        //
        // 追加（2026-10-13，Phase 2）：原先这里是
        // `maps.iter().find_map(|m| m.raster.as_ref())`——只认**第一张**，
        // 于是城市图即便在 `index.yaml` 里声明了自己的 `raster`，它的地形
        // 也永远读不进来（这是「到达城市级别」的硬阻塞之一）。ADR-0009 决策 ③
        // 要求「每级自带栅格」，故改为逐图读入，键 = 图 ID。
        //
        // 失败语义不变：仍然只在「数据压根读不出来」时失败（文件缺失/损坏/超限），
        // 与上面列的三类一致——这是 I/O 与格式问题，不是「作者还没写完」。
        let mut terrains: BTreeMap<wv_core::Id, wv_core::terrain::TerrainData> = BTreeMap::new();
        for m in &maps {
            if let Some(r) = m.raster.as_ref() {
                let data = crate::terrain_bin::read_terrain(&self.root, r)?;
                terrains.insert(m.id.clone(), data);
            }
        }

        // 街道图层：每个 `kind: road` 且声明了 `source` 的图层各读自己的 YAML
        // （ADR-0010 待做第 3 项，2026-10-13）。键 = 图层 ID。
        //
        // 失败语义分级（V1：作者还没写完不算坏工程）：
        // - `source` 指向的文件**不存在** → 跳过（文档留空，渲染层会兜底 +
        //   提示）。既有工程里本就有悬空声明（如 minimal-world 的
        //   `maps/world/index.yaml` 写了 `roads.yaml` 而文件从未创建），
        //   不能因补齐读取链路就突然拒绝整个工程；
        // - 文件存在但**读不出/解析不了** → 照旧报错（I/O 与格式问题）。
        let mut streets: BTreeMap<wv_core::Id, wv_core::StreetLayer> = BTreeMap::new();
        for m in &maps {
            for l in &m.layers {
                if l.kind == wv_core::map::LayerKind::Road {
                    if let Some(src) = l.source.as_deref() {
                        if !self.root.join(src).is_file() {
                            continue;
                        }

                        // 建筑：ADR-0016 §6 落地项。失败语义分级与街道同款（V1）。
                        let mut buildings: BTreeMap<wv_core::Id, wv_core::BuildingLayer> =
                            BTreeMap::new();
                        for m in &maps {
                            for l in &m.layers {
                                if l.kind == wv_core::map::LayerKind::Building {
                                    if let Some(src) = l.source.as_deref() {
                                        if !self.root.join(src).is_file() {
                                            continue;
                                        }
                                        let doc = self.load_buildings(src)?;
                                        buildings.insert(l.id.clone(), doc);
                                    }
                                }
                            }
                        }
                        let doc = self.load_streets(src)?;
                        streets.insert(l.id.clone(), doc);
                    }
                }
            }
        }

        // 测量：每图一份 `maps/<id>/measurements.yaml`，文件不存在即跳过（空图）。
        let mut measurements: BTreeMap<wv_core::Id, MeasurementOverlay> = BTreeMap::new();
        for m in &maps {
            let overlay = self::load_measurements_at(&self.root, &m.id)?;
            if !overlay.is_empty() {
                measurements.insert(m.id.clone(), overlay);
            }
        }

        // 势力：路线图 S12（2026-10-12）。
        // - `factions/`：每势力一个 YAML（§4.5）。
        // - `geometry/faction/`：每势力几何一个 YAML（`GeometrySpan`）。
        // 两个目录都**可缺失**——S12 是新维度，旧工程直接得空 Vec。
        let factions: Vec<wv_core::Faction> = load_dir(&self.root, "factions")?;
        let faction_geometry: Vec<wv_core::map::GeometrySpan> =
            load_dir(&self.root, "geometry/faction")?;

        // 命名区域（ADR-0017 §2.2）：`regions/` 每区域一个 YAML，几何内联。
        // 目录可缺失——旧工程直接得空 Vec（零迁移）。
        let regions: Vec<wv_core::Region> = load_dir(&self.root, "regions")?;

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
            regions,
            terrains,
            streets,
            measurements,
            buildings: BTreeMap::new(),
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
        if wants("regions") {
            save_entities(&self.root.join("regions"), &world.regions, |r| {
                r.id.as_str().to_string()
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
            // PlaceEdge 有 id 字段（自 ADR-0004 后），每实体一文件——与 §4.5 一致。
            // 旧单文件 distances/edges.yaml 由 loader 的 read_single_file_list 回退。
            save_entities(
                &self.root.join("distances/edges"),
                &world.distances.edges,
                |e| e.id.as_str().to_string(),
            )?;
            // 旧单文件清理：一旦开始写新格式，就把同名的旧单文件删掉，
            // 避免留 cruft。下次 loader 不再走 fallback，路径彻底稳定。
            let legacy = self.root.join("distances/edges.yaml");
            if legacy.is_file() {
                std::fs::remove_file(&legacy)?;
            }
        }
        if wants("distances/vehicles") {
            save_entities(
                &self.root.join("distances/vehicles"),
                &world.distances.vehicles,
                |v| v.id.as_str().to_string(),
            )?;
        }
        if wants("maps") {
            // 阶段七 v0.3 / Phase 2：把 `World.terrains` 里每张图的栅格写成
            // `maps/<id>/terrain.bin`，并把更新后的 `RasterRef` 写回那张图的
            // `index.yaml`。某张图没有栅格则不写（V1 守约：不动作者手改的 .bin）。
            // 只有被判为脏的图才重写 .bin——见 `terrain_is_dirty`。
            let updated_maps =
                write_terrains(&self.root, &world.maps, &world.terrains, dirty_paths, full)?;
            save_maps(&self.root.join("maps"), &updated_maps)?;
        }
        // 街道文档：落盘位置在 `maps/<map>/<source>`，但脏路径走**虚拟命名
        // 空间** `streets/<layer>`（避免 `maps/<map>/` 前缀把地形误判脏，
        // 见 `write_streets_layers`），所以路由也**独立于** `wants("maps")`：
        // 只画一笔街道时不应触发 index.yaml / terrain.bin 的重写。
        if wants("streets") || wants("maps") {
            write_streets_layers(&self.root, &world.maps, &world.streets, dirty_paths, full)?;
        }
        // 建筑文档：与街道同款虚拟命名空间 + 独立路由（ADR-0016 §6）。
        if wants("buildings") || wants("maps") {
            write_buildings_layers(&self.root, &world.maps, &world.buildings, dirty_paths, full)?;
        }
        // 测量：虚拟命名空间 `measurements/<map>`，独立路由（避免 `wants("maps")`
        // 把画一笔测量 → 重写 index.yaml / terrain.bin）。仅判 dirty 时写，
        // 空 overlay 走 `write_measurements_at` 内部的删除传播。
        if wants("measurements") || wants("maps") {
            self::write_measurements_layers(
                &self.root,
                &world.maps,
                &world.measurements,
                dirty_paths,
                full,
            )?;
        }
        // 阶段七 v0.4：写 `world.yaml`。这条**不**走 dirty 路由——
        // meta 是工程头部，永远写；fall back 到 `self.meta`（处理「刚加载、
        // world.meta 还没被任何命令填回」的边界）。
        let meta_to_write = world.meta.as_ref().unwrap_or(&self.meta);
        save_meta(&self.root, meta_to_write)?;
        // 同步内存里的 Project.meta，避免同进程内对 `self.meta()` 的断言看到旧值。
        self.meta = meta_to_write.clone();
        // 阶段七 v0.5：写 `timeline/axis.yaml`（若 world.timeline 存在）。
        // 与 meta 同源逻辑：结构性单文件、永远便宜、`None` 不动既有 axis.yaml
        //（V1 守约：作者手改的我们不擅自删）。
        if let Some(timeline) = world.timeline.as_ref() {
            save_timeline(&self.root, timeline)?;
        }
        Ok(())
    }

    /// 校验工程内部结构（**文字摘要**，供 `wv load <dir>` 与命令行用）。
    ///
    /// 读取一份街道图层文件（ADR-0010：`LayerKind::Road` 图层的
    /// `Layer.source` 指向的 YAML，文档模型是 [`StreetLayer`]）。
    ///
    /// 路径相对工程根（与 `Layer.source` 的存储口径一致）。
    /// 解析失败按惯例带路径报 [`IoError::Yaml`]；文件不存在则原样透传
    /// 文件系统错误——调用方按 V1 自行决定「提示但不阻止」。
    pub fn load_streets(&self, rel_path: &str) -> IoResult<StreetLayer> {
        let path = self.root.join(rel_path);
        let text = std::fs::read_to_string(&path).map_err(|source| IoError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })
    }

    /// 写入一份街道图层文件（[`Self::load_streets`] 的逆操作）。
    ///
    /// 父目录不存在则创建（街道编辑器首次保存 `maps/<id>/roads.yaml` 时
    /// 该图的目录可能还没建过）。返回值是写入路径的显示形态，供调用方
    /// 记日志 / 报告。
    pub fn write_streets(&self, rel_path: &str, layer: &StreetLayer) -> IoResult<String> {
        let path = self.root.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_yaml::to_string(layer).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })?;
        std::fs::write(&path, text)?;
        Ok(path.display().to_string())
    }

    /// 读取一份建筑图层文件（ADR-0016 §6：`LayerKind::Building` 图层的
    /// `Layer.source` 指向的 YAML，文档模型是 [`BuildingLayer`]）。
    /// 失败语义与 [`Self::load_streets`] 同款。
    pub fn load_buildings(&self, rel_path: &str) -> IoResult<BuildingLayer> {
        let path = self.root.join(rel_path);
        let text = std::fs::read_to_string(&path).map_err(|source| IoError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })
    }

    /// 写入一份建筑图层文件（[`Self::load_buildings`] 的逆操作）。
    pub fn write_buildings(&self, rel_path: &str, layer: &BuildingLayer) -> IoResult<String> {
        let path = self.root.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_yaml::to_string(layer).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })?;
        std::fs::write(&path, text)?;
        Ok(path.display().to_string())
    }

    /// 读取一份标签层配置（ADR-0017）。
    ///
    /// 路径相对工程根（与 `Layer.source` 的存储口径一致）。
    /// 解析失败按惯例带路径报 [`IoError::Yaml`]；文件不存在则原样透传
    /// 文件系统错误——调用方按 V1 自行决定「提示但不阻止」。
    ///
    /// 标签层是**只存配置不存文本**的容器（详见 `wv_core::label::LabelLayerConfig`），
    /// 故 `default()` = 空配置 = 白名单为空 = 该层不显示任何标签。
    pub fn load_labels(&self, rel_path: &str) -> IoResult<LabelLayerConfig> {
        let path = self.root.join(rel_path);
        let text = std::fs::read_to_string(&path).map_err(|source| IoError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })
    }

    /// 写入一份标签层配置（[`Self::load_labels`] 的逆操作）。
    ///
    /// 父目录不存在则创建（标签层首次保存 `maps/<id>/labels.yaml` 时
    /// 该图的目录可能还没建过）。返回值是写入路径的显示形态。
    pub fn write_labels(&self, rel_path: &str, config: &LabelLayerConfig) -> IoResult<String> {
        let path = self.root.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_yaml::to_string(config).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })?;
        std::fs::write(&path, text)?;
        Ok(path.display().to_string())
    }

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
