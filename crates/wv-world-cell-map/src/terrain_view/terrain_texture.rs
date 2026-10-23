//! `TerrainTexture` 资源 + 脏块追踪 + 分辨率推算。
//!
//! ## 职责
//! - `TerrainTexture` —— Bevy 资源（句柄 + 脏标记 + 上次承载图 id + 模式/淡化镜像）。
//! - `mark_all_dirty` / `mark_dirty` —— 整图 / 矩形脏标记。
//! - `rect_union` —— 矩形并集（私有）。
//! - `terrain_res_for` —— 每格纹理像素数推算（私有）。
//!
//! ## 边界（不做什么）
//! - **不持有 Image 像素**——`patch_terrain_image` 在 [`super::cells`] 里改。
//!
//! ## 主要导出
//! - [`TerrainTexture`]

use bevy::prelude::*;

use crate::shade::ShadeMode;
use wv_core::terrain::TerrainData;

/// 地形纹理句柄 + 脏块追踪。
#[derive(Resource)]
pub struct TerrainTexture {
    pub handle: Handle<Image>,
    /// 当前纹理覆盖的**逻辑格数**（每边）。像素边长 = `size × res`。
    pub size: u32,
    /// 当前纹理覆盖的**逻辑格行数**（高）。非方形地图（16:9 缺省）下
    /// `height != size`；方形图两者相等。
    pub height: u32,
    /// 每逻辑格的纹理像素数（1 = 纯色平涂，>1 = 瓦片直铺）。
    pub res: u32,
    /// 上次上传后累计的脏矩形（像素坐标 `[x0,y0,x1,y1]`，半开）。
    ///
    /// 由 `flush_terrain_texture`（wv-app::edit） 消费清零——它排在
    /// [`crate::terrain_chunks::sync_terrain_chunks`] **之前**运行，
    /// 因此 chunk 层另有一份副本（[`Self::chunks_dirty`]）。
    pub dirty: Option<[u32; 4]>,
    /// chunk 层专用的脏矩形副本（逻辑格，半开）。
    ///
    /// 与 `dirty` 在 [`Self::mark_dirty`] / [`Self::mark_all_dirty`] 里同步标记，
    /// 但由 [`crate::terrain_chunks::sync_terrain_chunks`] 消费清零。
    /// 拆分原因：system 链上 flush 先于 chunks 跑，`dirty` 到 chunks 时已是 `None`。
    pub chunks_dirty: Option<[u32; 4]>,
    /// **场渲染（ADR-0053）专用的脏矩形副本**（逻辑格，半开）。
    ///
    /// 与 `chunks_dirty` 同一个理由拆出来：场渲染的 `sync_terrain_field` 排在
    /// `sync_terrain_chunks` **之后**（它要读「帧末还活着的 chunk 实体数」），
    /// 那时 `chunks_dirty` 已被消费成 `None`。若让场渲染去抢那个旗标，就会
    /// 把 chunk 路径的增量刷新饿死（两条底图通路必须各自拿到完整信号）。
    ///
    /// 消费方：[`Self::take_field_dirty`]（由 `sync_terrain_field` 调用）。
    /// `None` 的语义是「这一帧没有可信的脏区」——场渲染此时**必须**走全量
    /// 重打（例如载图 / 撤销全表），不许猜一个矩形去补丁。
    pub field_dirty: Option<[u32; 4]>,
    /// 当前纹理内容所用的视图模式。
    ///
    /// **仅供「模式是否真的变了」的比较**（基线 §2.11：不要每帧重建整图）。
    /// 用 `Option` 是为了区分「尚未构建过」：`None` 表示纹理还从未按任一模式
    /// 生成过——此时即使数据稍后才可用，也应重建一次。
    pub last_mode: Option<ShadeMode>,
    /// 当前纹理内容是否带底图淡化（与 `view.terrain_soften` 比较用）。
    pub last_soften: bool,
    /// 当前纹理内容所用的地图主题（ADR-0016，与 `view.map_palette` 比较用）——
    /// 切主题 = 换一套色板烘焙整图，必须重建。
    pub last_theme_id: &'static str,
    /// 当前纹理画的是**哪张图**的底图（`None` = 不属于任何图的占位纹理）。
    ///
    /// 与 `last_mode` 同职责：**只用于「要不要重建」的比较**。Phase 2 起
    /// 底图随当前图变化（钻入一座自带地形的城市 → 纹理整体换成该城的地形），
    /// 所以「模式没变」不再等于「不用重建」——换图也必须重建。
    pub last_map: Option<wv_core::Id>,
}

impl TerrainTexture {
    /// 标记整个纹理为脏（整图重传）。单位：逻辑格。
    pub fn mark_all_dirty(&mut self) {
        let full = [0, 0, self.size, self.height];
        self.dirty = Some(full);
        self.chunks_dirty = Some(full);
        // 全图脏 = 「没有可信的局部矩形」，场渲染据此走全量重打。
        self.field_dirty = Some(full);
    }

    /// 标记一个矩形为脏，与已有脏区取并集（增量重传）。单位：逻辑格。
    pub fn mark_dirty(&mut self, rect: [u32; 4]) {
        self.dirty = rect_union(self.dirty, rect);
        self.chunks_dirty = rect_union(self.chunks_dirty, rect);
        self.field_dirty = rect_union(self.field_dirty, rect);
    }

    /// 取走场渲染的脏矩形（**消费语义**：取走后置 `None`）。
    ///
    /// 场渲染用它决定「能不能只补一块」。必须消费而不是读：留着上一笔的
    /// 矩形会让下一笔漏补——脏区是**增量信号**，重复消费/漏消费都错。
    pub fn take_field_dirty(&mut self) -> Option<[u32; 4]> {
        self.field_dirty.take()
    }
}

/// 两个可选脏矩形取并集（`None` 视为空集）。
fn rect_union(cur: Option<[u32; 4]>, rect: [u32; 4]) -> Option<[u32; 4]> {
    Some(match cur {
        Some(c) => [
            c[0].min(rect[0]),
            c[1].min(rect[1]),
            c[2].max(rect[2]),
            c[3].max(rect[3]),
        ],
        None => rect,
    })
}

/// 计算某视图下每逻辑格的纹理像素数。
///
/// 这是 `CellRenderer`、启动纹理资源与 chunk 拷贝的唯一分辨率口径。若在各处
/// 重新推断，`TerrainTexture.res` 会与实际 `Image` 尺寸脱节，主视口 / minimap /
/// 增量 patch 随之错位。
pub fn terrain_res_for(
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&super::tile_atlas::TileAtlas>,
) -> u32 {
    let cells = data.width.max(data.height).max(1);
    if mode == ShadeMode::GoogleMap || mode == ShadeMode::VectorFlat {
        // VectorFlat 与 GoogleMap 同口径：填充的水陆边由双线性高度采样决定
        // （isoline_overlay 的等值线描边也插值自同一高度场），亚像素分辨率让
        // 填充边与描边在像素级吻合——1px/格会把边退化成锯齿阶梯。
        // 小图至少 4px/格以抗锯齿；大图回落到 1px/格，维持 4096² 纹理预算。
        let min_res = if cells <= 1024 { 4 } else { 1 };
        (4096 / cells).clamp(min_res, 8)
    } else {
        atlas.map_or(1, |_| super::tile_atlas::tile_res_for(cells))
    }
}
