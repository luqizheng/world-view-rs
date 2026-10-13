//! 几何辅助。当前仅为占位，几何编码方式仍是 §13 待裁问题。

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
