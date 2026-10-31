//! `PoiKind` → 含义图形映射（ADR-0093）。
//!
//! 图形数据在 [`crate::poi_glyphs`]（生成文件，勿手改）；本模块是**手写的
//! 映射表**——17 类各对应一个 SVG 源（Lucide 直取 + AI 生成补缺），键 =
//! `PoiKind::key()` 除 `Other` 用 `circle-dot`（通用点语义）。

use crate::poi_glyphs::{glyph, GlyphData};
use wv_core::distance::PoiKind;

/// 类别的含义图形；`None` = 生成表缺该源（渲染侧回退白芯 pin）。
pub fn glyph_of(kind: PoiKind) -> Option<&'static GlyphData> {
    let name = match kind {
        PoiKind::Other => "circle-dot",
        PoiKind::Settlement => "castle",
        PoiKind::Peak => "mountain",
        PoiKind::Port => "anchor",
        PoiKind::Station => "train-front",
        PoiKind::Scenic => "trees",
        PoiKind::Food => "utensils",
        PoiKind::Lodging => "bed-double",
        PoiKind::Medical => "cross",
        PoiKind::Education => "graduation-cap",
        PoiKind::Government => "landmark",
        PoiKind::Shop => "store",
        PoiKind::Ruins => "ruins",
        PoiKind::Arena => "arena",
        PoiKind::Shrine => "shrine",
        PoiKind::Portal => "portal",
        PoiKind::Danger => "danger",
    };
    glyph(name)
}
