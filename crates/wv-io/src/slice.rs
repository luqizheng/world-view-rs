//! 底图切片（README §12-24 落地项）：把**来源图**栅格中被子图覆盖的那一块，
//! 采样成子图自己的 `TerrainData`，用于「按父图 bbox 一键生成子图栅格骨架」。
//!
//! ## 为什么放在 `wv-io`
//! 切片是**文件派生**（读已加载的 `World` → 产出可落盘的 `TerrainData`），
//! 不碰渲染、不做编辑决策；调用方拿结果走 `World::set_terrain` +
//! `Project::save_world`（脏路径 `maps/<id>/terrain.bin`），复用既有落盘管道
//! （含 `raster` 槽位回写）。`wv-core` 保持零文件依赖，`wv-app` 不新增职责。
//!
//! ## 采样口径（V1，最容易回退）
//! 最近邻：子图第 `(x, y)` 格取**格心**（`(x+0.5)·w/N`）映射到世界坐标，
//! 再落入来源图取整格。落在来源图外的格子取**海平面**（高 0 / 类 0）——
//! 城市贴着世界图边缘时宁可出现海岸，也不把对侧的地形镜像过来。
//!
//! 坐标换算只走 [`wv_core::resolve_frame_in`] 一条链（ADR-0009 决策 ④），
//! 因此「城市贴着世界图哪个位置」完全由既有 `origin`/图标派生口径决定，
//! 本模块不重复任何位置语义。

use wv_core::coord::{resolve_frame_in, MapFrame};
use wv_core::id::Id;
use wv_core::map::MapNode;
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::World;

use crate::error::{IoError, IoResult};

/// 来源图上溯深度上限：环靠步数截断（与 `wv-app` 的 `active_terrain` 同口径）。
fn source_terrain<'w>(world: &'w World, start: &Id) -> Option<(&'w MapNode, &'w TerrainData)> {
    let mut id = start.clone();
    for _ in 0..=world.maps.len() {
        let node = world.maps.iter().find(|m| m.id == id)?;
        if let Some(data) = world.terrain_of(&id) {
            return Some((node, data));
        }
        id = node.parent.clone()?;
    }
    None
}

/// 从来源图栅格切出 `map_id` 所指子图的覆盖区域，采样为 `size × size` 网格。
///
/// 返回 [`IoError::Binary`] 携带原因（子图不存在 / 无 `extent` / 无 `parent` /
/// 链上无栅格 / 尺寸超上限）。
/// 错误文案带上下文，CLI 直接打印即可。
pub fn slice_terrain(world: &World, map_id: &Id, size: u32) -> IoResult<TerrainData> {
    if size == 0 || (size as usize) * (size as usize) > crate::terrain_bin::MAX_TERRAIN_CELLS {
        return Err(IoError::Binary(format!(
            "切片尺寸 {size} 非法（1..={}）",
            (crate::terrain_bin::MAX_TERRAIN_CELLS as f64).sqrt() as u32
        )));
    }
    let target = world
        .maps
        .iter()
        .find(|m| &m.id == map_id)
        .ok_or_else(|| IoError::Binary(format!("地图 `{map_id}` 不存在")))?;
    let target_extent = target.extent.ok_or_else(|| {
        IoError::Binary(format!("地图 `{map_id}` 未声明 extent，无从确定覆盖区域"))
    })?;
    if target.parent.is_none() {
        return Err(IoError::Binary(format!(
            "地图 `{map_id}` 是根图，切片只对子图有意义（根图本身就是来源）"
        )));
    }
    let target_frame = resolve_frame_in(&world.maps, map_id)
        .ok_or_else(|| IoError::Binary(format!("地图 `{map_id}` 无法还原帧（父链断裂）")))?;
    let (src_node, src_data) =
        source_terrain(world, target.parent.as_ref().unwrap()).ok_or_else(|| {
            IoError::Binary(format!(
                "地图 `{map_id}` 的父链上没有任何一张自带栅格的图，无从切片"
            ))
        })?;
    let src_frame: MapFrame = resolve_frame_in(&world.maps, &src_node.id)
        .ok_or_else(|| IoError::Binary(format!("来源图 `{}` 无法还原帧", src_node.id)))?;
    // 局部坐标是「图单位」不是「格」：格宽 = 来源图 extent / 栅格列数。
    // 来源图没声明 extent（遗留工程）时按「一格 = 一单位」退化——与
    // `grid.cell_size` 旧口径一致，只在数据本就不完整时出现。
    let (src_cell_w, src_cell_h) = match src_node.extent.as_ref() {
        Some(e) => (
            e.width / src_data.width as f32,
            e.height / src_data.height as f32,
        ),
        None => (1.0, 1.0),
    };

    let tw = target_extent.width;
    let th = target_extent.height;
    let mut cells = Vec::with_capacity((size as usize) * (size as usize));
    for gy in 0..size {
        for gx in 0..size {
            // 子图格心 → 本图局部 → 世界 → 来源图局部。
            let local = wv_core::geometry::Point::new(
                (gx as f32 + 0.5) * tw / size as f32,
                (gy as f32 + 0.5) * th / size as f32,
            );
            let world_pt = target_frame.local_to_world(local);
            let src = src_frame.world_to_local(world_pt);
            let sx = (src.x / src_cell_w).floor() as i64;
            let sy = (src.y / src_cell_h).floor() as i64;
            let cell = if sx >= 0
                && sy >= 0
                && (sx as u32) < src_data.width
                && (sy as u32) < src_data.height
            {
                src_data.cells[sy as usize * src_data.width as usize + sx as usize]
            } else {
                TerrainCellData::default()
            };
            cells.push(cell);
        }
    }
    Ok(TerrainData {
        width: size,
        height: size,
        cells,
    })
}
