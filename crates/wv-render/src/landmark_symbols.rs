//! 地物符号（Landmark symbols）——世界地图上的地点标记。
//!
//! ## 职责
//! rank ≥ 2 的地点以**经典 map-pin**（圆头 + 尖尾 + 白芯，Lucide `map-pin`
//! 的实心化）呈现。2026-10-23 用户裁定城堡/小镇插画符号退役（"地表不要贴
//! 城堡 town 的图"），统一为几何 pin。
//!
//! ## 渲染口径
//! - **顶点色 mesh 单批**：Bevy `Mesh2d` 支持 `ATTRIBUTE_COLOR`，配白色
//!   `ColorMaterial` 即顶点色渲染——全部符号合一个 mesh + 一个实体，
//!   与地形装饰（ADR-0031 D4）同款「原地改写」纪律。
//! - 符号尺寸 = 屏幕像素 × `view.scale`（缩放时整组换 mesh，与 decor 的
//!   `size_q` 同口径）。
//!
//! ## 边界（不做什么）
//! - **不动 POI 圆点路径**：rank < 2 的普通地点（路网交点、村庄）仍是
//!   圆点 + 外环（`sync_poi`）——编辑态需要精确点选，圆点是工具语义。
//! - **不做主题化**：常量色（同 decor 的 ink 常量先例），主题演绎规则
//!   随 README §12-30（ThemePack M3）后继。
//! - **不做选中态**：选中高亮走既有 POI/Inspector 路径，符号层只管「画」。

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec2;
use bevy::prelude::*;
use bevy::render::mesh::PrimitiveTopology;
use bevy::render::mesh::{Indices, Mesh, MeshVertexAttribute, VertexFormat};

/// 符号 Z：POI 内芯（0.22）之上、标注（0.4）之下——地物是「世界的景物」，
/// 压过点标；标签永远最后（在符号之上可读）。
pub const LANDMARK_Z: f32 = 0.24;
/// 符号基准高度（屏幕像素；pin 实际顶高 22.5，留 1.5 呼吸）。
pub const BASE_H_PX: f32 = 26.0;

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
    /// 两种 `LandmarkKind` 共用同一形状——等级语义由标签层承担，符号层不画。
    pub fn push_map_pin(&mut self, o: Vec2, s: f32) {
        let body = [0.16, 0.18, 0.22, 1.0]; // 深墨针体
        let hole = [1.0, 1.0, 1.0, 1.0]; // 白芯（map-pin 内圆）
                                         // 尾：圆底两侧收到底尖（上边 y=11 藏进头圆内，衔接无接缝）。
        self.tri([-5.6, 11.0], [5.6, 11.0], [0.0, 0.0], body, o, s);
        // 头圆 r=7.5 圆心 (0,15) → 总高 22.5；白芯 r=3。
        self.circle(0.0, 15.0, 7.5, 24, body, o, s);
        self.circle(0.0, 15.0, 3.0, 16, hole, o, s);
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

    /// map-pin 产出非空几何，且顶点色通道与位置等长、索引不越界。
    #[test]
    fn landmark_batches_produce_colored_geometry() {
        let mut a = LandmarkBatch::new();
        a.push_map_pin(Vec2::ZERO, 1.0);
        assert!(!a.is_empty());
        assert_eq!(a.verts.len(), a.colors.len());
        assert!(a.indices.len().is_multiple_of(3), "索引必须是三角形倍数");
        // 索引不越界。
        assert!(a.indices.iter().all(|&i| (i as usize) < a.verts.len()));
    }

    /// 空 batch 不产 mesh（空 mesh 禁律）。
    #[test]
    fn empty_batch_builds_no_mesh() {
        assert!(build_landmark_mesh(LandmarkBatch::new()).is_none());
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
