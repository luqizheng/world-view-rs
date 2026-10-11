//! 工程目录的加载与保存。
//!
//! 目录布局见项目文档 §6。本模块只实现**阶段一**所需的最小闭环：
//! 读 `world.yaml` → 解析元数据 → 校验目录存在性 → 打印摘要。
//! 完整加载（地图/人物/事件/距离）按里程碑逐步补齐。

use std::path::{Path, PathBuf};

use wv_core::time::TimePoint;
use wv_core::{World, WorldMeta};

use crate::error::{IoError, IoResult};

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
    /// 当前范围：元数据 + 时间轴 + 事件 + 人物 + 地图 + 距离图 + 事态 + 轨迹。
    /// 事态与轨迹加载后**立即做引用完整性校验**（`AGENTS.md` §7.2 硬约束）。
    pub fn load_world(&self) -> IoResult<World> {
        use crate::situation_io::{load_dir, validate_situations_and_tracks};

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
        let maps: Vec<wv_core::map::MapNode> = load_maps(&self.root)?;
        let distances: wv_core::distance::DistanceGraph = {
            let path = self.root.join("distances/places.yaml");
            if path.is_file() {
                let text = std::fs::read_to_string(&path)?;
                let places: Vec<wv_core::distance::Place> =
                    serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
                        path: path.display().to_string(),
                        source,
                    })?;
                wv_core::distance::DistanceGraph {
                    places,
                    ..Default::default()
                }
            } else {
                Default::default()
            }
        };

        // 引用完整性校验（基线 §2.21）：悬空引用必须报错，不得静默忽略。
        let event_ids: Vec<wv_core::Id> = events.iter().map(|e| e.id.clone()).collect();
        let map_ids: Vec<wv_core::Id> = maps.iter().map(|m| m.id.clone()).collect();
        let character_ids: Vec<wv_core::Id> = characters.iter().map(|c| c.id.clone()).collect();
        let place_ids: Vec<wv_core::Id> = distances.places.iter().map(|p| p.id.clone()).collect();
        validate_situations_and_tracks(
            &situations,
            &tracks,
            &event_ids,
            &map_ids,
            &character_ids,
            &place_ids,
        )?;

        // 地形栅格：若地图声明了 raster，加载 terrain.bin。
        let terrain = maps
            .iter()
            .find_map(|m| m.raster.as_ref())
            .map(|r| crate::terrain_bin::read_terrain(&self.root, r))
            .transpose()?;

        Ok(World {
            meta: Some(self.meta.clone()),
            maps,
            timeline,
            characters,
            events,
            relations: Vec::new(),
            distances,
            situations,
            tracks,
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

    /// 校验工程内部引用完整性。
    ///
    /// 两级：
    /// 1. 目录结构检查（本文档下方）；
    /// 2. **引用完整性检查**（`AGENTS.md` §7.2 硬约束）——通过 `load_world()` 触发，
    ///    悬空引用会以 `CoreError::DanglingReference` 报错。
    pub fn validate(&self) -> IoResult<Vec<String>> {
        let expected = ["maps", "timeline", "characters", "relations", "distances"];
        let mut warnings = Vec::new();
        for dir in expected {
            if !self.root.join(dir).exists() {
                warnings.push(format!("缺少预期目录: {dir}/"));
            }
        }
        // 触发引用完整性校验（失败即向上传播错误）。
        self.load_world()?;
        Ok(warnings)
    }
}
