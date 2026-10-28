//! 势力图切蛋糕分区（ADR-0079）的数据模型与时间求值。
//!
//! ## 职责
//!
//! 一张图 × 一个势力层一份 [`FactionPartition`]：历史由若干
//! [`PartitionSpan`] 组成，每个 span 是一份**完整拓扑快照**——
//!
//! - [`PartitionVertex`]：共享顶点表。相邻脸的环**引用**同一顶点 id，
//!   拖动它两侧必然联动——零和与铺满是构造保证（ADR-0079 D1）；
//! - [`PartitionFace`]：脸 = 被边界围出的连通块，环 / 洞存**顶点 id**
//!   （首尾不重复），归属 = Faction `Id` 引用、可空 = 无主（P4）。
//!
//! 时间语义与 [`crate::time::TimeSpan`] 同口径：半开 `[from, to)`，
//! `to == None` 至今。想要「某天某图某层的划分」走
//! [`partition_span_at`]——渲染层禁自比时间（AGENTS §3.2）。
//!
//! ## 为什么快照式（ADR-0079 §3）
//!
//! 改旧历史段（V2）在快照式下是「就地改一份快照」，无重放、无传染；
//! 操作日志式改旧需重放后段。代价是每段完整存拓扑——分区是小型矢量
//! （相对地形栅格），P3 YAML 可承受。

use serde::{Deserialize, Serialize};

use crate::id::Id;
use crate::time::TimeSpan;
use crate::World;

/// 分区快照的一个顶点：共享表的条目。
///
/// `id` 在**一个 span 内**唯一；相邻脸引用同一 id 即共享该顶点。
/// 坐标 map-local 平面（flat f32），与 `GeometryData::Polygon` 同口径。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PartitionVertex {
    /// span 内唯一顶点 id。
    pub id: u32,
    /// map-local 坐标 `[x, y]`。
    pub xy: [f32; 2],
}

/// 分区快照的一张脸：被边界围出的连通块。
///
/// `ring` / `holes` 存**顶点 id**（不是坐标），首尾不重复；环引用的
/// 顶点 id 必须存在于同一 span 的顶点表（[`PartitionSpan::validate_topology`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionFace {
    /// span 内唯一脸 id。
    pub id: u32,
    /// 归属势力；`None` = 无主（渲染灰；不因建层造合成实体，ADR-0079 §2 D1）。
    pub owner: Option<Id>,
    /// 外环：顶点 id 序列，首尾不重复，≥3 点。
    pub ring: Vec<u32>,
    /// 洞环列表（如湖），每个都是顶点 id 序列、首尾不重复。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<u32>>,
}

/// 分区的一段历史：一份完整拓扑快照 + 生效区间。
///
/// 区间半开 `[from, to)`，`to == None` 至今；同一分区的各 span
/// 区间**不重叠且升序**（[`PartitionSpan::validate_topology`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionSpan {
    /// 生效起点（含）。
    pub from: i64,
    /// 生效终点（不含）；`None` = 至今。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<i64>,
    /// 共享顶点表。
    pub vertices: Vec<PartitionVertex>,
    /// 脸列表。
    pub faces: Vec<PartitionFace>,
}

/// 一份分区：一张图 × 一个势力层。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactionPartition {
    /// 所属地图。
    pub map: Id,
    /// 所属势力层（`FactionLayer.id`）。
    pub layer: Id,
    /// 历史段，按 `from` 升序、区间不重叠。
    pub spans: Vec<PartitionSpan>,
}

impl PartitionSpan {
    /// 拓扑自检（编辑命令提交前的轻校验；违规即 Err，**构造侧**责任——
    /// 存量数据由校验面板提示，不阻止加载 / 保存，V1）。
    ///
    /// 检查：① ring/holes 引用的顶点 id 都在顶点表；② ring ≥3 点；
    /// ③ 顶点 / 脸 id 各自 span 内唯一。
    pub fn validate_topology(&self) -> Result<(), String> {
        let vertex_ids: std::collections::HashSet<u32> =
            self.vertices.iter().map(|v| v.id).collect();
        let mut face_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for face in &self.faces {
            if !face_ids.insert(face.id) {
                return Err(format!("脸 id 重复: {}", face.id));
            }
            if face.ring.len() < 3 {
                return Err(format!("脸 {} 的环少于 3 点", face.id));
            }
            for hole in &face.holes {
                if hole.len() < 3 {
                    return Err(format!("脸 {} 的洞环少于 3 点", face.id));
                }
            }
            for vid in face.ring.iter().chain(face.holes.iter().flatten()) {
                if !vertex_ids.contains(vid) {
                    return Err(format!("脸 {} 引用了不存在的顶点 {}", face.id, vid));
                }
            }
        }
        Ok(())
    }
}

impl FactionPartition {
    /// 二分定位：`from <= day < to`（`to == None` 视作 +∞）的那一段。
    ///
    /// span 无命中 → `None`（day 落在首段之前 / 段间隙——渲染画空，
    /// **不**回退最近段：day 在段外就该什么都没有）。
    pub fn span_at(&self, day: i64) -> Option<&PartitionSpan> {
        self.spans
            .binary_search_by(|span| {
                if day < span.from {
                    std::cmp::Ordering::Greater
                } else if let Some(to) = span.to {
                    if day < to {
                        std::cmp::Ordering::Equal
                    } else {
                        std::cmp::Ordering::Less
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()
            .map(|i| &self.spans[i])
    }
}

/// 某天某图某层的划分快照；分区不存在 / 无命中段 → `None`。
///
/// wv-core 的唯一求值入口（`factions_at` / `place_territory_at` 同款
/// 契约）：渲染层拿到的永远是「本日该画的东西」，禁自比时间。
pub fn partition_span_at<'a>(
    world: &'a World,
    map: &Id,
    layer: &Id,
    day: i64,
) -> Option<&'a PartitionSpan> {
    let partition = world
        .partitions
        .iter()
        .find(|p| &p.map == map && &p.layer == layer)?;
    partition.span_at(day)
}

/// 默认段构造辅助（`BeginPartitionSpanCommand` 与测试共用）：整块地 +
/// 单一无主脸。`vertices` 为外环坐标（map-local flat），自动编 id。
///
/// 返回 `(span, 下一可用顶点 id, 下一可用脸 id)`，供 fork 复制时续号。
pub fn whole_land_span(outer: &[f32], from: i64, to: Option<i64>) -> (PartitionSpan, u32, u32) {
    let n = outer.len() / 2;
    let vertices: Vec<PartitionVertex> = (0..n)
        .map(|i| PartitionVertex {
            id: i as u32,
            xy: [outer[i * 2], outer[i * 2 + 1]],
        })
        .collect();
    let ring: Vec<u32> = (0..n as u32).collect();
    (
        PartitionSpan {
            from,
            to,
            vertices,
            faces: vec![PartitionFace {
                id: 0,
                owner: None,
                ring,
                holes: Vec::new(),
            }],
        },
        n as u32,
        1,
    )
}

/// `TimeSpan` 的区段口径在这里复述一次以保持单测自足（与
/// `crate::time::TimeSpan::contains` 同义：`from <= day < to`）。
///
/// 独立成函数是给 [`whole_land_span`] 生成的区间做对照断言用；
/// 真正的权威实现在 `time` 模块。
#[allow(dead_code)]
fn span_contains_reference(span_from: i64, span_to: Option<i64>, day: i64) -> bool {
    let ts = TimeSpan {
        from: span_from,
        to: span_to,
    };
    ts.contains(&crate::time::TimePoint(day))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两段历史 `[0,10)` 与 `[20, None)`，中间有 10 天的间隙。
    fn two_span_partition() -> FactionPartition {
        let (span_a, _, _) =
            whole_land_span(&[0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0], 0, Some(10));
        let (span_b, _, _) = whole_land_span(&[0.0, 0.0, 8.0, 0.0, 8.0, 8.0, 0.0, 8.0], 20, None);
        FactionPartition {
            map: Id::new_unchecked("map_a"),
            layer: Id::new_unchecked("layer_borders"),
            spans: vec![span_a, span_b],
        }
    }

    #[test]
    fn span_query_returns_none_before_first_span_and_in_gaps() {
        let p = two_span_partition();
        assert!(p.span_at(-1).is_none(), "day=-1 在首段之前应为 None");
        assert!(p.span_at(15).is_none(), "day=15 落在 [10,20) 间隙应为 None");
        assert!(p.span_at(0).is_some(), "day=0 命中第一段");
        assert!(p.span_at(9).is_some(), "day=9（半开上界前）命中第一段");
        assert!(p.span_at(10).is_none(), "day=10 是第一段开上界，应为 None");
        assert!(p.span_at(20).is_some(), "day=20 命中第二段（至今）");
        assert!(p.span_at(999_999).is_some(), "to=None 视作至今");
    }

    #[test]
    fn span_query_half_open_bounds_match_timespan_contains() {
        // 与 TimeSpan::contains 对照：本模块的区间口径不得漂移。
        for day in [-5i64, 0, 9, 10, 15, 20, 100] {
            let p = two_span_partition();
            let hit = p.span_at(day).map(|s| s.from);
            let want_two_span =
                |from: i64, to: Option<i64>| span_contains_reference(from, to, day).then_some(from);
            let expected = want_two_span(0, Some(10)).or_else(|| want_two_span(20, None));
            assert_eq!(hit, expected, "day={day} 的命中与 TimeSpan 口径不一致");
        }
    }

    #[test]
    fn partition_yaml_round_trips_with_skip_defaults() {
        let p = two_span_partition();
        // 空洞 / 无主走 skip：序列化文本里不应出现空列表 / null。
        let text = serde_yaml::to_string(&p).expect("应能序列化");
        assert!(!text.contains("holes:"), "空 holes 应 skip: {text}");
        assert!(
            !text.contains("to: null"),
            "None to 应 skip（有值 to 正常出现）: {text}"
        );
        let back: FactionPartition = serde_yaml::from_str(&text).expect("应能反序列化");
        assert_eq!(back, p, "round-trip 后应相等");
    }

    #[test]
    fn face_ring_vertex_ids_must_exist() {
        let (mut span, _, _) = whole_land_span(&[0.0, 0.0, 10.0, 0.0, 10.0, 10.0], 0, None);
        assert!(
            span.validate_topology().is_ok(),
            "合法快照应通过: {:?} vs {:?}",
            span.faces[0].ring,
            span.vertices.iter().map(|v| v.id).collect::<Vec<_>>()
        );

        // 环引用缺失顶点 → Err。
        span.faces[0].ring.push(999);
        let err = span.validate_topology().expect_err("缺失顶点应报错");
        assert!(err.contains("999"), "错误应指出顶点 id: {err}");
    }

    #[test]
    fn degenerate_ring_fewer_than_three_points_is_rejected() {
        let mut span = PartitionSpan {
            from: 0,
            to: None,
            vertices: vec![
                PartitionVertex {
                    id: 0,
                    xy: [0.0, 0.0],
                },
                PartitionVertex {
                    id: 1,
                    xy: [1.0, 0.0],
                },
            ],
            faces: vec![PartitionFace {
                id: 0,
                owner: None,
                ring: vec![0, 1],
                holes: Vec::new(),
            }],
        };
        let err = span.validate_topology().expect_err("2 点环应报错");
        assert!(err.contains("3"), "错误应说明环至少 3 点: {err}");
        // 3 点环（含环内重复 id——重复属渲染语义，V1 不查）应通过。
        span.faces[0].ring = vec![0, 1, 1];
        span.vertices.push(PartitionVertex {
            id: 2,
            xy: [0.5, 1.0],
        });
        span.vertices[1].xy = [1.0, 0.0];
        span.validate_topology()
            .expect("3 点环（含环内重复 id）应通过拓扑检查");
    }

    #[test]
    fn partition_span_at_filters_by_map_and_layer() {
        let world = World::default();
        assert!(
            partition_span_at(
                &world,
                &Id::new_unchecked("map_a"),
                &Id::new_unchecked("layer_borders"),
                0
            )
            .is_none(),
            "空世界应返回 None 而非 panic"
        );
    }
}
