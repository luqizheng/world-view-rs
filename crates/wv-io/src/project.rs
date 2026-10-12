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
