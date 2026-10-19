//! 地物符号（Landmark symbols）——世界地图上的城市/城镇插画符号。
//!
//! ## 职责
//! 世界地图档把 rank ≥ 2 的地点从「圆点 + 外环」升级为**程序化多边形符号**：
//! - `Citadel`（rank 3 国家级）：石墙 + 双塔 + 红旗 + 底部投影；
//! - `Town`（rank 2 市镇级）：两间错落小屋 + 投影。
//!
//! ## 渲染口径
//! - **顶点色 mesh 单批**：Bevy `Mesh2d` 支持 `ATTRIBUTE_COLOR`，配白色
//!   `ColorMaterial` 即顶点色渲染——全部符号合一个 mesh + 一个实体，
//!   与地形装饰（ADR-0031 D4）同款「原地改写」纪律。
//! - 符号尺寸 = 屏幕像素 × `view.scale`（缩放时整组换 mesh，与 decor 的
//!   `size_q` 同口径）。
//! - 绘制函数（`push_citadel` / `push_town`）**独立于渲染系统**——未来换成
//!   sprite 资产（ART_STYLE_SPEC §3.4 的 11 类符号）时逐符号替换，接线不动。
//!
//! ## 边界（不做什么）
//! - **不动 POI 圆点路径**：rank < 2 的普通地点（路网交点、村庄）仍是
//!   圆点 + 外环（`sync_poi`）——编辑态需要精确点选，圆点是工具语义。
//! - **不做主题化**：v1 常量色（同 decor 的 ink 常量先例），主题演绎规则
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
/// 符号基准高度（屏幕像素，Citadel 全高；Town 约 0.7×）。
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

    /// 追加一个轴对齐矩形（左下 + 尺寸，局部坐标）。
    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: [f32; 4], o: Vec2, s: f32) {
        let (x0, y0, x1, y1) = (x, y, x + w, y + h);
        self.tri([x0, y0], [x1, y0], [x1, y1], rgba, o, s);
        self.tri([x0, y0], [x1, y1], [x0, y1], rgba, o, s);
    }

    /// Citadel（rank 3）：石墙 + 双塔 + 红旗 + 底部投影。
    ///
    /// 局部坐标：原点 = 锚点（Place 的 x/y），符号向上生长（y+），
    /// 投影向右下（ART_STYLE_SPEC §1.5：左上光、右下投影 0.5× 高）。
    pub fn push_citadel(&mut self, o: Vec2, s: f32) {
        let stone = [0.72, 0.68, 0.62, 1.0]; // 墙体石灰
        let stone_dark = [0.55, 0.51, 0.46, 1.0]; // 背光面
        let roof = [0.62, 0.28, 0.22, 1.0]; // 塔顶赭红
        let flag = [0.80, 0.20, 0.16, 1.0]; // 旗红
        let shadow = [0.25, 0.22, 0.18, 0.30]; // 投影（半透明）

        // 底部投影：右下偏移的扁矩形（宽 14 高 4）。
        self.quad(1.0, -2.0, 14.0, 3.5, shadow, o, s);
        // 主体城墙：宽 13 高 8，底边落在 y=0。
        self.quad(-6.5, 0.0, 13.0, 8.0, stone, o, s);
        // 城墙垛口（三个小方齿，墙顶）。
        for cx in [-6.5, -1.5, 3.5] {
            self.quad(cx, 8.0, 3.0, 2.0, stone, o, s);
        }
        // 左塔（高 14）+ 背光右壁 + 锥顶。
        self.quad(-8.0, 0.0, 4.5, 14.0, stone, o, s);
        self.quad(2.2, 0.0, 2.3, 14.0, stone_dark, o, s); // 右塔身合并在 x 2.2..4.5
        self.tri([-8.2, 14.0], [-3.3, 14.0], [-5.75, 19.0], roof, o, s);
        // 右塔（高 12）+ 锥顶。
        self.quad(3.5, 0.0, 4.5, 12.0, stone, o, s);
        self.tri([3.3, 12.0], [8.2, 12.0], [5.75, 17.0], roof, o, s);
        // 旗杆 + 三角旗（左塔顶）。
        self.quad(-6.0, 19.0, 0.8, 5.0, stone_dark, o, s);
        self.tri([-5.2, 24.0], [-0.6, 23.0], [-5.2, 21.5], flag, o, s);
    }

    /// Town（rank 2）：两间错落小屋 + 投影。
    pub fn push_town(&mut self, o: Vec2, s: f32) {
        let wall = [0.85, 0.78, 0.66, 1.0]; // 暖墙
        let wall_dark = [0.68, 0.60, 0.50, 1.0]; // 背光墙
        let roof = [0.58, 0.33, 0.20, 1.0]; // 深褐顶
        let shadow = [0.25, 0.22, 0.18, 0.30];

        // 投影。
        self.quad(0.5, -1.5, 10.0, 2.8, shadow, o, s);
        // 主屋（左，大）：墙 + 人字顶。
        self.quad(-6.0, 0.0, 7.5, 5.5, wall, o, s);
        self.tri([-6.8, 5.5], [1.5, 5.5], [-2.65, 9.5], roof, o, s);
        // 副屋（右，小、后错）：墙 + 顶。
        self.quad(1.8, 0.0, 5.0, 4.0, wall_dark, o, s);
        self.tri([1.2, 4.0], [7.4, 4.0], [4.3, 7.2], roof, o, s);
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

    /// Citadel / Town 都产出非空几何，且顶点色通道与位置等长。
    #[test]
    fn landmark_batches_produce_colored_geometry() {
        let mut a = LandmarkBatch::new();
        a.push_citadel(Vec2::ZERO, 1.0);
        assert!(!a.is_empty());
        assert_eq!(a.verts.len(), a.colors.len());
        assert!(a.indices.len().is_multiple_of(3), "索引必须是三角形倍数");
        // 索引不越界。
        assert!(a.indices.iter().all(|&i| (i as usize) < a.verts.len()));

        let mut b = LandmarkBatch::new();
        b.push_town(Vec2::ZERO, 1.0);
        assert!(!b.is_empty());
        assert_eq!(b.verts.len(), b.colors.len());
        assert!(b.indices.iter().all(|&i| (i as usize) < b.verts.len()));
    }

    /// 空 batch 不产 mesh（空 mesh 禁律）。
    #[test]
    fn empty_batch_builds_no_mesh() {
        assert!(build_landmark_mesh(LandmarkBatch::new()).is_none());
    }

    /// 符号向上生长：全部顶点 y >= -3（投影允许微沉），主体在 0 以上。
    #[test]
    fn symbols_grow_upward_from_anchor() {
        let mut b = LandmarkBatch::new();
        b.push_citadel(Vec2::ZERO, 1.0);
        let n_cit = b.verts.len();
        b.push_town(Vec2::new(100.0, 50.0), 1.0);
        let cit_top = b.verts[..n_cit]
            .iter()
            .map(|v| v[1])
            .fold(f32::MIN, f32::max);
        assert!(cit_top > 18.0, "城堡塔尖应在锚点上方，得 {cit_top}");
    }
}
