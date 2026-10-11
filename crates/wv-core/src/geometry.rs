//! 几何辅助。当前仅为占位，几何编码方式仍是 §13 待裁问题。

/// 二维点。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
