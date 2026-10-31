//! 地物符号（Landmark symbols）——世界地图上的地点标记。
//!
//! ## 职责（ADR-0093 图标钉轮，2026-10-31 用户裁定）
//! **全模式统一「图标钉」**：彩色钉身（按 [`wv_core::distance::PoiKind`]
//! 分类配色，选中 = 金）+ 钉头内的**白色含义图形**（`poi_glyphs` 生成数据，
//! Lucide 直取 + AI 生成补缺）。城堡/小镇程序化插画（Citadel/Town）随本轮
//! 退役——「加新语义 = 加一个 PoiKind 变体 + 一个 SVG 源」。
//!
//! ## 渲染口径
//! - **顶点色 mesh 单批**：Bevy `Mesh2d` 支持 `ATTRIBUTE_COLOR`，配白色
//!   `ColorMaterial` 即顶点色渲染——每类一个 mesh + 一个实体。
//! - 符号尺寸 = 屏幕像素 × `view.scale`。**局部坐标单位就是屏幕像素**
//!   （符号高约 22.5），多乘画布基准会把符号放大 26 倍（2026-10-23 教训）。
//! - 含义图形 = 折线描边（相邻点对 → 垂直偏移四边形），白色画在钉身之上。
//!
//! ## 边界（不做什么）
//! - **不动 POI 圆点路径**：rank < 2 的普通地点仍是圆点 + 外环（工具语义）。
//! - **不做选中环**：选中 = 钉身染金（换色优先于环，用户裁定）。

/// 选中钉的针体色（金 = 本仓库「选中」语义色；白芯不变）。
pub const SELECTED_PIN_BODY: [f32; 4] = [1.0, 0.78, 0.18, 1.0];

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec2;
use bevy::prelude::*;
use bevy::render::mesh::PrimitiveTopology;
use bevy::render::mesh::{Indices, Mesh, MeshVertexAttribute, VertexFormat};

/// 符号 Z：POI 内芯（0.22）之上、标注（0.4）之下——地物是「世界的景物」，
/// 压过点标；标签永远最后（在符号之上可读）。
pub const LANDMARK_Z: f32 = 0.24;

/// 一个符号种类的绘制参数。`push_*` 系列函数向共享顶点缓冲追加三角形。
pub struct LandmarkBatch {
    pub verts: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl LandmarkBatch {
    pub fn new() -> Self {
        Self {
            verts: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }

    /// 追加一个纯色三角形（顶点为符号局部坐标，调用方负责平移/缩放）。
    #[allow(clippy::too_many_arguments)]
    fn tri(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], rgba: [f32; 4], o: Vec2, s: f32) {
        let base = self.verts.len() as u32;
        for p in [a, b, c] {
            self.verts.push([o.x + p[0] * s, o.y + p[1] * s, 0.0]);
            self.colors.push(rgba);
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    /// 实心圆（扇形三角逼近，`seg` 段）。
    #[allow(clippy::too_many_arguments)]
    fn circle(&mut self, cx: f32, cy: f32, r: f32, seg: usize, rgba: [f32; 4], o: Vec2, s: f32) {
        let tau = std::f32::consts::TAU;
        for i in 0..seg {
            let a0 = i as f32 * tau / seg as f32;
            let a1 = (i + 1) as f32 * tau / seg as f32;
            self.tri(
                [cx, cy],
                [cx + r * a0.cos(), cy + r * a0.sin()],
                [cx + r * a1.cos(), cy + r * a1.sin()],
                rgba,
                o,
                s,
            );
        }
    }

    /// 经典 map-pin（Lucide `map-pin` 的实心化）：圆头 + 尖尾 + 白芯。
    ///
    /// 局部坐标：原点 = 锚点（Place 的 x/y）= pin 底尖，符号向上生长（y+）。
    /// 全类别共用同一钉形——类别语义由配色与含义图形承担（ADR-0093）。
    pub fn push_map_pin(&mut self, o: Vec2, s: f32) {
        self.push_map_pin_tinted(o, s, [0.16, 0.18, 0.22, 1.0]);
    }

    /// 同 [`Self::push_map_pin`]，但针体颜色由调用方给（白芯不变）。
    ///
    /// rank < 2 的分类点标兜底用；rank ≥ 2 的图标钉走
    /// [`Self::push_icon_pin`]（含义图形替代白芯）。
    pub fn push_map_pin_tinted(&mut self, o: Vec2, s: f32, body: [f32; 4]) {
        let hole = [1.0, 1.0, 1.0, 1.0]; // 白芯（map-pin 内圆）
                                         // 尾：圆底两侧收到底尖（上边 y=11 藏进头圆内，衔接无接缝）。
        self.tri([-5.6, 11.0], [5.6, 11.0], [0.0, 0.0], body, o, s);
        // 头圆 r=7.5 圆心 (0,15) → 总高 22.5；白芯 r=3。
        self.circle(0.0, 15.0, 7.5, 24, body, o, s);
        self.circle(0.0, 15.0, 3.0, 16, hole, o, s);
    }

    /// **图标钉**（ADR-0093）：钉身（分类色 / 选中金）+ 钉头内白色含义图形。
    ///
    /// 图形替代白芯位：折线描边画在头圆之上（同 mesh 内索引序靠后 = 顶层）。
    pub fn push_icon_pin(
        &mut self,
        glyph: &crate::poi_glyphs::GlyphData,
        o: Vec2,
        s: f32,
        body: [f32; 4],
    ) {
        self.tri([-5.6, 11.0], [5.6, 11.0], [0.0, 0.0], body, o, s);
        self.circle(0.0, 15.0, 7.5, 24, body, o, s);
        self.push_glyph(glyph, o, s, [1.0, 1.0, 1.0, 1.0]);
    }

    /// 白色含义图形：24 空间折线 → 钉头居中描边。
    ///
    /// 图形盒映射到 11px（头圆直径 15 − 4px 边距），笔画宽 = 2px（图形空间）
    /// × k ≈ 0.92px × s；圆角线帽在 11px 尺度下不可辨，取方帽（实现最简）。
    pub fn push_glyph(
        &mut self,
        glyph: &crate::poi_glyphs::GlyphData,
        o: Vec2,
        s: f32,
        rgba: [f32; 4],
    ) {
        const GLYPH_BOX: f32 = 11.0;
        let k = GLYPH_BOX / 24.0;
        let center = o + Vec2::new(0.0, 15.0 * s);
        let half_w = k * s * 0.5;
        let mid = Vec2::new(12.0, 12.0);
        for sub in glyph.subpaths {
            for pair in sub.windows(2) {
                let a = center + (Vec2::new(pair[0].0, pair[0].1) - mid) * k * s;
                let b = center + (Vec2::new(pair[1].0, pair[1].1) - mid) * k * s;
                self.stroke_seg(a, b, half_w, rgba);
            }
        }
    }

    /// 单段描边：垂直偏移成四边形（两三角形）。零长度段忽略。
    fn stroke_seg(&mut self, p0: Vec2, p1: Vec2, half_w: f32, rgba: [f32; 4]) {
        let dir = p1 - p0;
        let len = dir.length();
        if len < 1e-6 {
            return;
        }
        let n = Vec2::new(-dir.y / len, dir.x / len) * half_w;
        self.tri(
            (p0 - n).to_array(),
            (p1 - n).to_array(),
            (p1 + n).to_array(),
            rgba,
            Vec2::ZERO,
            1.0,
        );
        self.tri(
            (p0 - n).to_array(),
            (p1 + n).to_array(),
            (p0 + n).to_array(),
            rgba,
            Vec2::ZERO,
            1.0,
        );
    }
}

impl Default for LandmarkBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// 把一批符号烘成 mesh（顶点位置 + 顶点色 + 索引）。
///
/// 空 batch 返回 `None`（调用方下线实体，不插 0 顶点 mesh——Bevy slab
/// allocator 空 mesh 崩溃的既有教训）。
pub fn build_landmark_mesh(b: LandmarkBatch) -> Option<Mesh> {
    if b.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    let LandmarkBatch {
        verts,
        colors,
        indices,
    } = b;
    let n = verts.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, verts);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Uv", 1, VertexFormat::Float32x2),
        vec![[0.0, 0.0]; n],
    );
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 图标钉产出非空几何：钉身（分类色）+ 白色含义图形，索引合法。
    #[test]
    fn icon_pin_produces_body_and_white_glyph() {
        let glyph = crate::poi_glyphs::glyph("castle").expect("castle 图形应在生成表中");
        let mut b = LandmarkBatch::new();
        b.push_icon_pin(glyph, Vec2::ZERO, 1.0, [0.2, 0.4, 0.8, 1.0]);
        assert!(!b.is_empty(), "图标钉应有几何");
        assert_eq!(b.verts.len(), b.colors.len());
        assert!(b.indices.iter().all(|&i| (i as usize) < b.verts.len()));
        // 钉身蓝色与图形白色都在场。
        assert!(
            b.colors.iter().any(|c| c[2] > 0.7 && c[0] < 0.4),
            "钉身应为给定蓝"
        );
        assert!(
            b.colors
                .iter()
                .any(|c| c[0] == 1.0 && c[1] == 1.0 && c[2] == 1.0),
            "含义图形必须为白色"
        );
    }

    /// 全部 17 类都有图形（PoiKind → glyph 映射完备性，ADR-0093）。
    #[test]
    fn every_poi_kind_has_a_glyph() {
        for kind in wv_core::distance::PoiKind::ALL {
            let g = crate::glyphs::glyph_of(kind);
            assert!(g.is_some(), "{kind:?} 缺含义图形");
            assert!(!g.unwrap().subpaths.is_empty(), "{kind:?} 图形为空");
        }
    }

    /// 图形画在钉头内：顶点落在头圆（圆心 (0,15) r≈7）附近而非钉尾。
    #[test]
    fn glyph_stays_inside_pin_head() {
        let glyph = crate::poi_glyphs::glyph("anchor").expect("anchor 图形应在生成表中");
        let mut b = LandmarkBatch::new();
        b.push_glyph(glyph, Vec2::ZERO, 1.0, [1.0, 1.0, 1.0, 1.0]);
        let ys: Vec<f32> = b.verts.iter().map(|v| v[1]).collect();
        let top = ys.iter().copied().fold(f32::MIN, f32::max);
        let bottom = ys.iter().copied().fold(f32::MAX, f32::min);
        assert!(top <= 15.0 + 6.0, "图形顶应落在头圆内，得 {top}");
        assert!(bottom >= 15.0 - 6.0, "图形底应落在头圆内，得 {bottom}");
    }

    /// 空 batch 不产 mesh（空 mesh 禁律）。
    #[test]
    fn empty_batch_builds_no_mesh() {
        assert!(build_landmark_mesh(LandmarkBatch::new()).is_none());
    }

    /// 分类 pin：针体色由调用方给、白芯不变；形状与默认 pin 逐顶点一致。
    #[test]
    fn tinted_pin_keeps_shape_and_swaps_body_color_only() {
        let mut tinted = LandmarkBatch::new();
        tinted.push_map_pin_tinted(Vec2::ZERO, 1.0, [1.0, 0.0, 0.0, 1.0]);
        let mut plain = LandmarkBatch::new();
        plain.push_map_pin(Vec2::ZERO, 1.0);

        assert_eq!(
            tinted.verts, plain.verts,
            "只换色不换形，位置序列必须逐点相同"
        );
        assert_ne!(tinted.colors, plain.colors, "针体色应不同");
        assert!(
            tinted
                .colors
                .iter()
                .any(|c| (c[0] - 1.0).abs() < 1e-6 && c[1] < 1e-6 && c[2] < 1e-6),
            "针体应为给定红色"
        );
        assert!(
            tinted
                .colors
                .iter()
                .any(|c| c[0] == 1.0 && c[1] == 1.0 && c[2] == 1.0),
            "白芯必须保留（否则 pin 在深色底图上失去可辨性）"
        );
    }

    /// 符号向上生长：pin 底尖贴锚点，头圆在锚点上方。
    #[test]
    fn symbols_grow_upward_from_anchor() {
        let mut b = LandmarkBatch::new();
        b.push_map_pin(Vec2::ZERO, 1.0);
        let top = b.verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        let bottom = b.verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        assert!(top > 18.0, "pin 头顶应在锚点上方，得 {top}");
        assert!(bottom >= -0.5, "pin 底尖应贴锚点（y≈0），得 {bottom}");
    }

    /// 缩放生效：s=2 时几何整体放大（顶点坐标随之翻倍）。
    #[test]
    fn pin_scales_with_symbol_scale() {
        let mut b = LandmarkBatch::new();
        b.push_map_pin(Vec2::new(100.0, 50.0), 2.0);
        let top = b.verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        assert!(
            (top - (50.0 + 22.5 * 2.0)).abs() < 0.1,
            "头顶应到锚点+45，得 {top}"
        );
    }
}
