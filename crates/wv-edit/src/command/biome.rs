//! biome 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

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
pub(crate) fn ring_wu(ring: &[(f32, f32)], cell_size: f32) -> Vec<f32> {
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
    /// 展示名（动作层构造时从 World 捕获；`label` 无 World 访问权，
    /// 缺省回退 id——撤销栈给人看，不该是 `place_marker_176…` 机器串）。
    name: Option<String>,
    before: std::sync::Mutex<BeforeRegen>,
}

impl RegenerateBiomeMarkerCommand {
    pub fn new(place_id: wv_core::Id, day: i64) -> Self {
        Self {
            place_id,
            day,
            name: None,
            before: std::sync::Mutex::new(BeforeRegen::default()),
        }
    }

    /// 带展示名构造（撤销栈 / 通知文案用）。
    pub fn named(place_id: wv_core::Id, day: i64, name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::new(place_id, day)
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
        format!(
            "重生成标记「{}」",
            self.name.as_deref().unwrap_or(self.place_id.as_str())
        )
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
