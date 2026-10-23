//! 地形着色档位（自 wv-render `ViewMode` 收窄而来的**纯着色**维度）。
//!
//! `ViewMode` 是 wv-render 的呈现态维度（含内容门控 / 覆盖物开关）；本枚举
//! 只保留「底图像素怎么算色」这一个维度，由 wv-render 侧
//! `cellmap_bridge::shade_mode_of` 做 `ViewMode → ShadeMode` 映射（ADR-0054 D4）。

/// 地形底图的着色档位（与 `ViewMode` 八档一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadeMode {
    /// 等值线矢量平涂（浏览态默认）。
    #[default]
    VectorFlat,
    /// Google Default Map。
    GoogleMap,
    /// 真实地形（高度调制明度）。
    RealTerrain,
    /// 色块地形（纯色平涂）。
    Schematic,
    /// 势力图（底色淡化地形）。
    Faction,
    /// 事件图（极简灰白底图）。
    Event,
    /// 轮廓图（水陆二分深色 + 海岸线）。
    Coastline,
    /// Topographic / CellMap 工作区（群系固定色块）。
    Topographic,
}
