//! 地物符号（Landmark symbols）——世界地图上的地点标记。
//!
//! ## 职责（2026-10-23 三轮裁定，2026-10-26 追记）
//! 按**模式分流**（`vector_layers::landmark_marker`）：
//! - **CellMap（Topographic）**：`push_map_pin`——经典 map-pin（圆头 + 尖尾 +
//!   白芯，Lucide `map-pin` 实心化），数据视图的地点标记；
//! - **专业制图（Map / GoogleMap）**：同为 map-pin，但按 rank 分尺寸档
//!   （国都比市镇大）——普通地图不画插画，城市分量由「符号 + 字级」承载；
//! - **其余模式（Browse 等）**：`push_citadel` / `push_town`——城堡/小镇
//!   程序化插画，照旧。
//!
//! ## 渲染口径
//! - **顶点色 mesh 单批**：Bevy `Mesh2d` 支持 `ATTRIBUTE_COLOR`，配白色
//!   `ColorMaterial` 即顶点色渲染——全部符号合一个 mesh + 一个实体，
//!   与地形装饰（ADR-0031 D4）同款「原地改写」纪律。
//! - 符号尺寸 = 屏幕像素 × `view.scale`（缩放时整组换 mesh，与 decor 的
//!   `size_q` 同口径）。**局部坐标单位就是屏幕像素**（符号高约 22–24），
//!   缩放系数只传 `view_scale`——多乘一档画布基准会把符号放大 26 倍
//!   （2026-10-23 实测教训：Browse 出现盖住半个大陆的巨型墨团）。
//!
//! ## 边界（不做什么）
//! - **不动 POI 圆点路径**：rank < 2 的普通地点（路网交点、村庄）仍是
//!   圆点 + 外环（`sync_poi`）——编辑态需要精确点选，圆点是工具语义。
//! - **不做主题化**：常量色（同 decor 的 ink 常量先例），主题演绎规则
//!   随 README §12-30（ThemePack M3）后继。
//! - **不做选中态**：选中高亮走既有 POI/Inspector 路径，符号层只管「画」。

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

    /// 追加一个轴对齐矩形（左下 + 尺寸，局部坐标）。
    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: [f32; 4], o: Vec2, s: f32) {
        let (x0, y0, x1, y1) = (x, y, x + w, y + h);
        self.tri([x0, y0], [x1, y0], [x1, y1], rgba, o, s);
        self.tri([x0, y0], [x1, y1], [x0, y1], rgba, o, s);
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
        self.push_map_pin_tinted(o, s, [0.16, 0.18, 0.22, 1.0]);
    }

    /// 同 [`Self::push_map_pin`]，但针体颜色由调用方给（白芯不变）。
    ///
    /// 专业制图的分类 POI 用它：**同一 pin 形状**、按类别换针体色（ADR-0064
    /// 追记：先统一形状，后续再补各类图标）。
    pub fn push_map_pin_tinted(&mut self, o: Vec2, s: f32, body: [f32; 4]) {
        let hole = [1.0, 1.0, 1.0, 1.0]; // 白芯（map-pin 内圆）
                                         // 尾：圆底两侧收到底尖（上边 y=11 藏进头圆内，衔接无接缝）。
        self.tri([-5.6, 11.0], [5.6, 11.0], [0.0, 0.0], body, o, s);
        // 头圆 r=7.5 圆心 (0,15) → 总高 22.5；白芯 r=3。
        self.circle(0.0, 15.0, 7.5, 24, body, o, s);
        self.circle(0.0, 15.0, 3.0, 16, hole, o, s);
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

    /// 三种符号都产出非空几何，且顶点色通道与位置等长、索引不越界。
    #[test]
    fn landmark_batches_produce_colored_geometry() {
        let mut pin = LandmarkBatch::new();
        pin.push_map_pin(Vec2::ZERO, 1.0);
        let mut cit = LandmarkBatch::new();
        cit.push_citadel(Vec2::ZERO, 1.0);
        let mut town = LandmarkBatch::new();
        town.push_town(Vec2::ZERO, 1.0);
        for (name, b) in [("pin", pin), ("citadel", cit), ("town", town)] {
            assert!(!b.is_empty(), "{name} 应有几何");
            assert_eq!(b.verts.len(), b.colors.len(), "{name} 顶点色应与位置等长");
            assert!(
                b.indices.len().is_multiple_of(3),
                "{name} 索引必须是三角形倍数"
            );
            assert!(b.indices.iter().all(|&i| (i as usize) < b.verts.len()));
        }
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
