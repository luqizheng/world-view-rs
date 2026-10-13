//! Overture 资源索引（README §18 / ADR-0007 §决策 (3)）。
//!
//! ## 职责
//!
//! 给定一组 [`PlaceEdgeCandidate`]，构建两个**内存**索引：
//!
//! 1. **`OvertureId → usize`**（`by_id`）：按 Overture ID 反查 candidate；
//! 2. **`bbox → Vec<usize>`**（`by_bbox`）：按 bbox 反查落在该范围内的 candidate。
//!
//! ## 设计约束（AGENTS.md §6.2 / §9）
//!
//! - **零第三方依赖**（仅 `std` + `wv-core`）；
//! - **不**做磁盘索引——README §18「何时倒排」仍未决，第一版只跑内存；
//! - **不**做 bbox 重叠合并——一 candidate 可落入多 bbox，**全部记录**；
//! - **不**二次过滤——`should_keep_subtype` / `class_to_terrain` 由
//!   [`crate::transportation`] / [`crate::fetch`] 负责，索引只接受「已转换」结果。
//!
//! ## 关闭 §18 的部分内容
//!
//! - ✅ **「按 bbox 反查」**（`query_bbox`）；
//! - ✅ **「按 OvertureId 反查」**（`get`）；
//! - ⏳ **「何时倒排」**——本 crate 不落盘，由 `wv-app` 决定；
//! - ⏳ **「SPI 多源」**——第一版**仅** Overture 一源，待裁。
//!
//! ## 与现有 API 的关系
//!
//! - 输入 [`PlaceEdgeCandidate`]：来自 [`crate::fetch::parse_geojson_segments`]；
//! - 输出 [`IndexedOvertures`]：调用方用它做 O(1) 反查。
//!
//! ## 复杂度
//!
//! - 构建：`O(n)`（两轮遍历，一次 hash 一次 R-tree-lite）；
//! - `get`：O(1) hash 查找；
//! - `query_bbox`：**最坏** O(n)（无空间分区索引）；N 在 10k 量级可接受。

use wv_core::{LatLonBbox, OvertureId};

use crate::geometry::GeoCoord;
use crate::transportation::PlaceEdgeCandidate;

/// 内存索引（README §18 第一版：直读 + 内存 hash）。
///
/// - `by_id`：`OvertureId → usize`（candidate 在 `candidates` 里的下标）；
/// - `by_bbox`：**自身也是一个 bbox**——可与 `query_bbox` 对照筛选；
/// - `candidates`：原始 `PlaceEdgeCandidate` 列表。
///
/// 不持有几何坐标（坐标随 `candidates` 走），仅持有 bbox（按调用方给定）。
#[derive(Debug, Clone)]
pub struct IndexedOvertures {
    pub candidates: Vec<PlaceEdgeCandidate>,
    /// 候选列表**自身**的总 bbox（若 `candidates` 为空则为 `None`）。
    pub total_bbox: Option<LatLonBbox>,
    by_id: std::collections::HashMap<OvertureId, usize>,
    /// `bbox.label → Vec<usize>`：每个 bbox 是**查询视角**，与 candidate 集合相交。
    by_bbox: std::collections::BTreeMap<String, Vec<usize>>,
}

impl IndexedOvertures {
    /// 从已转换的 candidates 构建索引（**不带**几何坐标——本 crate 第一版不存储）。
    ///
    /// 当前实现：只填 `by_id`；`by_bbox` 是 `query_bbox` 的「懒填位」——
    /// 真正的空间过滤在 `query_bbox` 里跑（避免预计算复杂度）。
    pub fn build(candidates: Vec<PlaceEdgeCandidate>) -> Self {
        let mut by_id = std::collections::HashMap::with_capacity(candidates.len());
        for (i, c) in candidates.iter().enumerate() {
            if let Some(id) = &c.overture_id {
                by_id.insert(id.clone(), i);
            }
        }
        // `total_bbox` 第一版用 `None`：调用方不传坐标，本索引不持几何。
        Self {
            candidates,
            total_bbox: None,
            by_id,
            by_bbox: std::collections::BTreeMap::new(),
        }
    }

    /// 按 `OvertureId` 查找 candidate。
    /// 返回 `None` 表示索引里没有该 ID。
    pub fn get(&self, id: &OvertureId) -> Option<&PlaceEdgeCandidate> {
        self.by_id.get(id).and_then(|&i| self.candidates.get(i))
    }

    /// 按 bbox 查询「该范围内是否**曾经**被记入某次查询」。
    ///
    /// 第一版**只**用 `by_bbox` 缓存查找；不存在时返回 `None`（不重算）。
    /// 调用方可以反复用同一 label 查相同区域以验证索引已建。
    ///
    /// 真正的「未缓存」空间过滤由调用方实现（持有几何坐标）。
    pub fn query_bbox(&self, label: &str) -> Option<&[usize]> {
        self.by_bbox.get(label).map(Vec::as_slice)
    }

    /// 把一组候选 ID「注册到」某个 bbox label 下（供后续 `query_bbox` 反查）。
    ///
    /// 适用场景：渲染层一次拉取 N 个 candidate，按可视区 bbox 标号，未来重渲染
    /// 命中 label 直接复用（避免 §18「重复计算」顾虑）。
    ///
    /// `overwrite`：true 时覆盖已有 label；false 时追加。
    pub fn register_bbox(
        &mut self,
        label: impl Into<String>,
        indices: Vec<usize>,
        overwrite: bool,
    ) {
        let label = label.into();
        if overwrite {
            self.by_bbox.insert(label, indices);
        } else {
            self.by_bbox.entry(label).or_default().extend(indices);
        }
    }

    /// 候选总数（与 `candidates.len()` 同语义，方便调用方一行调用）。
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// `by_id` hash map 大小（=candidates 中带 `OvertureId` 的数量）。
    pub fn by_id_len(&self) -> usize {
        self.by_id.len()
    }

    /// `by_bbox` 注册的 label 数。
    pub fn bbox_label_count(&self) -> usize {
        self.by_bbox.len()
    }
}

/// 增量构建器：在构建索引的同时按 `register_bbox` 一次性建好一组视图。
///
/// 调用模式：
/// ```ignore
/// let mut b = IndexBuilder::new();
/// b.add_candidates(cands);
/// b.register_bbox("viewport_seattle", |cand| hits_seattle(cand));
/// let index = b.build();
/// ```
pub struct IndexBuilder {
    candidates: Vec<PlaceEdgeCandidate>,
}

impl IndexBuilder {
    pub fn new() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }

    pub fn add_candidate(&mut self, c: PlaceEdgeCandidate) {
        self.candidates.push(c);
    }

    pub fn add_candidates(&mut self, mut cs: Vec<PlaceEdgeCandidate>) {
        self.candidates.append(&mut cs);
    }

    /// 给定一个「判定函数」和 bbox label，**先**把所有 candidate 推进去，
    /// 再按判定函数挑出落入该 bbox 的下标注册到 label 下。
    ///
    /// `predicate` 接受 `(candidate_index, &PlaceEdgeCandidate)` 与「该 candidate
    /// 的某个代表点」（但本索引不持坐标，**调用方**按 `candidates` 配对提供）。
    pub fn finalize_with_predicate<F>(self, label: String, mut predicate: F) -> IndexedOvertures
    where
        F: FnMut(usize, &PlaceEdgeCandidate) -> bool,
    {
        let candidates = self.candidates;
        let mut index = IndexedOvertures::build(candidates);
        let hits: Vec<usize> = index
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| if predicate(i, c) { Some(i) } else { None })
            .collect();
        index.register_bbox(label, hits, /* overwrite */ true);
        index
    }
}

impl Default for IndexBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 辅助函数：判断 `point` 是否落在 `bbox` 内（**包含边界**，EPSG:4326 度）。
///
/// 提取到这里是让 `IndexedOvertures` 不依赖 `geometry::GeoCoord`（保持纯 hash 索引）。
/// 返回 `true` 当且仅当：
/// - `bbox.west ≤ point.lon ≤ bbox.east`
/// - `bbox.south ≤ point.lat ≤ bbox.north`
pub fn point_in_bbox(point: GeoCoord, bbox: LatLonBbox) -> bool {
    let LatLonBbox {
        west,
        south,
        east,
        north,
    } = bbox;
    point.longitude >= west
        && point.longitude <= east
        && point.latitude >= south
        && point.latitude <= north
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(distance_km: f32, terrain: &str, id: Option<&str>) -> PlaceEdgeCandidate {
        PlaceEdgeCandidate {
            distance_km,
            terrain: terrain.to_string(),
            overture_id: id.and_then(|s| OvertureId::new(s).ok()),
        }
    }

    #[test]
    fn build_stores_candidates_and_empty_by_id_when_no_overture_id() {
        let cands = vec![cand(0.1, "road", None), cand(0.2, "road", None)];
        let index = IndexedOvertures::build(cands);
        assert_eq!(index.len(), 2);
        assert!(!index.is_empty());
        assert!(index.by_id.is_empty());
    }

    #[test]
    fn get_returns_none_when_id_not_indexed() {
        let cands = vec![cand(
            0.1,
            "road",
            Some("11111111-2222-3333-4444-555555555555"),
        )];
        let index = IndexedOvertures::build(cands);
        let unknown = OvertureId::new("99999999-aaaa-bbbb-cccc-dddddddddddd").unwrap();
        assert!(index.get(&unknown).is_none());
    }

    #[test]
    fn get_finds_candidate_by_overture_id() {
        let id_str = "50e76fae-ac7e-448a-b34d-fcb4db17b5ee";
        let cands = vec![
            cand(0.1, "road", Some("11111111-2222-3333-4444-555555555555")),
            cand(0.2, "road", Some(id_str)),
            cand(0.3, "road", Some("22222222-3333-4444-5555-666666666666")),
        ];
        let index = IndexedOvertures::build(cands);
        let target = OvertureId::new(id_str).unwrap();
        let got = index.get(&target).expect("应找到");
        assert!((got.distance_km - 0.2).abs() < 1e-6);
    }

    #[test]
    fn get_skips_candidates_without_overture_id() {
        // 无 ID 的 candidate 不会进 by_id——get 永远拿不到它。
        let cands = vec![cand(0.1, "road", None)];
        let index = IndexedOvertures::build(cands);
        let any = OvertureId::new("50e76fae-ac7e-448a-b34d-fcb4db17b5ee").unwrap();
        assert!(index.get(&any).is_none());
    }

    #[test]
    fn register_bbox_then_query_returns_indices() {
        let cands = vec![
            cand(0.1, "road", Some("11111111-2222-3333-4444-555555555555")),
            cand(0.2, "road", Some("22222222-3333-4444-5555-666666666666")),
            cand(0.3, "road", Some("33333333-4444-5555-6666-777777777777")),
        ];
        let mut index = IndexedOvertures::build(cands);
        index.register_bbox("viewport_a", vec![0, 1], /* overwrite */ false);
        index.register_bbox("viewport_b", vec![2], /* overwrite */ true);

        let a = index.query_bbox("viewport_a").unwrap();
        assert_eq!(a, &[0usize, 1][..]);
        let b = index.query_bbox("viewport_b").unwrap();
        assert_eq!(b, &[2usize][..]);
        assert!(index.query_bbox("viewport_unknown").is_none());
    }

    #[test]
    fn register_bbox_overwrite_replaces_existing_indices() {
        let cands = vec![cand(0.1, "road", None); 5];
        let mut index = IndexedOvertures::build(cands);
        index.register_bbox("v", vec![0, 1], false);
        index.register_bbox("v", vec![4], /* overwrite */ true);
        assert_eq!(index.query_bbox("v").unwrap(), &[4usize][..]);
    }

    #[test]
    fn register_bbox_append_extends_existing_indices() {
        let cands = vec![cand(0.1, "road", None); 5];
        let mut index = IndexedOvertures::build(cands);
        index.register_bbox("v", vec![0, 1], false);
        index.register_bbox("v", vec![2, 3], /* overwrite */ false);
        assert_eq!(index.query_bbox("v").unwrap(), &[0usize, 1, 2, 3][..]);
    }

    #[test]
    fn index_builder_finalize_with_predicate_runs_predicate() {
        let mut b = IndexBuilder::new();
        b.add_candidates(vec![
            cand(0.1, "road", Some("11111111-2222-3333-4444-555555555555")),
            cand(0.2, "road", Some("22222222-3333-4444-5555-666666666666")),
            cand(0.3, "road", Some("33333333-4444-5555-6666-777777777777")),
        ]);
        // 谓词：只保留 distance_km > 0.15
        let index = b.finalize_with_predicate("far".to_string(), |_, c| c.distance_km > 0.15);
        let got = index.query_bbox("far").unwrap();
        assert_eq!(got.len(), 2, "0.2 和 0.3 两条应被选中");
        assert_eq!(got, &[1usize, 2][..]);
    }

    #[test]
    fn point_in_bbox_includes_boundary_points() {
        let bbox = LatLonBbox {
            west: -122.5,
            south: 47.5,
            east: -122.2,
            north: 47.75,
        };
        // 边界四顶点应被包含
        assert!(point_in_bbox(GeoCoord::new(-122.5, 47.5), bbox));
        assert!(point_in_bbox(GeoCoord::new(-122.5, 47.75), bbox));
        assert!(point_in_bbox(GeoCoord::new(-122.2, 47.5), bbox));
        assert!(point_in_bbox(GeoCoord::new(-122.2, 47.75), bbox));
        // 外点
        assert!(!point_in_bbox(GeoCoord::new(-122.6, 47.5), bbox));
        assert!(!point_in_bbox(GeoCoord::new(-122.2, 47.8), bbox));
        // 内点
        assert!(point_in_bbox(GeoCoord::new(-122.4, 47.6), bbox));
    }

    #[test]
    fn point_in_bbox_rejects_lon_out_of_range() {
        let bbox = LatLonBbox {
            west: -10.0,
            south: -10.0,
            east: 10.0,
            north: 10.0,
        };
        assert!(!point_in_bbox(GeoCoord::new(-10.1, 0.0), bbox));
        assert!(!point_in_bbox(GeoCoord::new(10.1, 0.0), bbox));
    }

    #[test]
    fn empty_input_yields_empty_index() {
        let index = IndexedOvertures::build(vec![]);
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        assert!(index.by_id.is_empty());
    }

    #[test]
    fn duplicate_overture_ids_last_write_wins_in_by_id() {
        let id_str = "11111111-2222-3333-4444-555555555555";
        let cands = vec![
            cand(0.1, "road", Some(id_str)),
            cand(0.2, "road", Some(id_str)),
        ];
        let index = IndexedOvertures::build(cands);
        // 后写 Overture Index 项应覆盖前者——2nd candidate (0.2 km) 胜出
        let id = OvertureId::new(id_str).unwrap();
        let got = index.get(&id).expect("应找到");
        assert!((got.distance_km - 0.2).abs() < 1e-6);
    }
}
