//! # wv-overture —— 真实地理底图读取层（ADR-0007）
//!
//! 职责：
//! - 按 [`wv_core::GeographicSourceV1`] + `bbox` 下载/校验本地 Parquet 切片；
//! - 把 Overture segment 转成**渲染层**用的几何资源；
//! - **不写回领域模型**——回到领域模型时只有引用、没有几何。
//!
//! ## 依赖纪律（AGENTS.md §9）
//!
//! - 本 crate **不依赖 Bevy**（与 `wv-io` 同样约束）；
//! - 第一版**零运行时第三方依赖**（仅 `wv-core` + `thiserror`），
//!   因为 `segment_length_m()` 是纯函数；
//! - Parquet / Arrow 读取作为 T5 实施期再决定（README §17 schema 升级策略）。
//!
//! ## 设计参考
//!
//! - ADR-0007：只存引用、不落几何；
//! - README §15：决议 (a)–(f) 全部关闭；
//! - README §16 §17 §18：等待本 crate 实施期逐条关闭。

pub mod fetch;
pub mod geometry;
pub mod index;
pub mod transportation;

pub use fetch::{
    parse_geojson_segments, parse_geojson_segments_with_connectors, parse_geojson_str,
    GeoJSONConnector, GeoJSONFeature, GeoJSONFeatureCollection, GeoJSONProperties,
    SegmentWithConnectors,
};
pub use geometry::{haversine_m, segment_length_m, GeoCoord};

/// Overture schema 版本号（ADR-0007 §理由（4）+ README §17 schema 升级策略）。
///
/// **不写入领域模型**——仅本 crate 内部用作 Parquet 字段映射的版本校验。
/// 当 Overture 发布新 schema 时，由 wv-overture 内部更新此常量并跑回归测试。
pub const OVERTURE_SCHEMA_VERSION: &str = "2026-09-23.0";
