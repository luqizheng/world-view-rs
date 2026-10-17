//! 几何辅助。几何编码方式仍是 §13 待裁问题；本模块现提供无洞单环的
//! 点包含谓词 [`point_in_ring`]。

/// 二维点。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    /// 原点 `(0, 0)`。
    pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

    /// 构造一个点。
    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

/// 判断点 `(x, y)` 是否落在多边形环内部（射线法 / even-odd 规则）。
///
/// `ring` 是扁平坐标 `[x0, y0, x1, y1, ...]`，首尾**不重复**——与
/// [`crate::map::GeometryData::Polygon`] 的存储口径一致（§12-8：v0.1 仅支持
/// 无洞单环）。
///
/// ## 为什么放在 `base::geometry`
///
/// 这是纯几何谓词，不依赖任何领域类型。地界晕的命中测试与近带符号采样的
/// 网格过滤（`wv-render`）共用同一实现，避免各写一遍射线法——与 §4.2
/// 「唯一计算入口」同一条纪律。
///
/// ## 边界与退化
///
/// - 顶点数不足 3（`ring.len() < 6` 个浮点）→ 恒 `false`：退化环不构成面；
/// - 奇数长度输入忽略末尾落单的浮点（口径同
///   [`crate::map::distance::Place::fit_anchor_to_ring`] 的 `chunks_exact`）；
/// - 点**恰好落在边或顶点上**时结果取决于浮点比较的舍入，属未定义边界——
///   调用方不应依赖边界判定（地界命中本身也不关心一条边上的点）。
pub fn point_in_ring(ring: &[f32], x: f32, y: f32) -> bool {
    if ring.len() < 6 {
        return false;
    }
    let n = ring.len() / 2;
    let mut inside = false;
    // 上一条边的起点，初始化为最后一条边的起点以闭合环。
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[2 * i], ring[2 * i + 1]);
        let (xj, yj) = (ring[2 * j], ring[2 * j + 1]);
        // 向 +x 方向发射射线：只统计跨过 y 且交点在 x 右侧的边，
        // 每跨一条边翻转一次内外状态。`(yi > y) != (yj > y)` 保证不会有
        // 水平边参与、也保证下面的除法分母 `yj - yi` 非零。
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_in_ring_covers_interior_and_excludes_outside() {
        let ring = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        assert!(point_in_ring(&ring, 5.0, 5.0), "正方形中心 (5,5) 应在内部");
        assert!(
            !point_in_ring(&ring, 15.0, 5.0),
            "右侧外部点 (15,5) 应被排除"
        );
        assert!(
            !point_in_ring(&ring, 5.0, -1.0),
            "上方外部点 (5,-1) 应被排除"
        );
    }

    #[test]
    fn point_in_ring_rejects_degenerate_ring() {
        assert!(
            !point_in_ring(&[0.0, 0.0, 1.0, 1.0], 0.5, 0.5),
            "只有 2 个顶点（4 个浮点）不构成多边形，应返回 false"
        );
    }
}
