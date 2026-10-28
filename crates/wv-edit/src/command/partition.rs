//! 势力图切蛋糕分区命令（ADR-0074）。
//!
//! [`BeginPartitionSpanCommand`] 是动刀的第 0 步：该层无任何历史段时，
//! 用 `land_outlines` 造**首段**（整块地 + 湖洞，初始归属可空=无主）；
//! 已有历史段时在 `from` 处 **fork**：命中段拆 `[a,from)` + 新段
//! `[from,b)`（快照复制，顶点/脸 id 保持）——此后 Cut / Move 就地改新段，
//! Assign 在 fork 上换归属。
//!
//! dirty 键 = `"partitions"`（wv-io 的 saver 路由）。

use crate::command::*;
use wv_core::model::faction_partition::{PartitionFace, PartitionSpan, PartitionVertex};
use wv_core::{Id, World};

/// 动刀第 0 步（ADR-0074 D2）：开一段新的划分历史。
///
/// 两分支：
/// - 该层（map × layer）**无任何历史段** → 用 [`wv_terrain::outline::land_outlines`]
///   造**首段**：最大外环为初始脸、湖为洞，`initial_owner` 可空 = 无主；
/// - **已有历史段** → 在 `from` 处 **fork**：命中段 `[a, to)` 拆成
///   `[a, from)` + 新段 `[from, to)`，新段为命中段的**快照复制**
///   （顶点/脸 id 保持——共享引用是零和的构造基础）。
///
/// 时间语义：`from` 必须落在既有首段之后（否则无历史可承，Err）；
/// 首段的 `from` 即所给的生效时间（spec D4：新增要填生效时间）。
///
/// dirty = `["partitions"]`。
#[derive(Debug)]
pub struct BeginPartitionSpanCommand {
    /// 目标地图。
    pub map: Id,
    /// 目标势力层。
    pub layer: Id,
    /// 生效日（半开区间下界）。
    pub from: i64,
    /// 首段初始归属；`None` = 无主（不造合成势力实体）。
    pub initial_owner: Option<Id>,
    /// `apply` 缓存的整个分区快照（revert 原样写回）。
    before: std::sync::Mutex<Option<wv_core::model::faction_partition::FactionPartition>>,
}

impl BeginPartitionSpanCommand {
    /// 构造（`before` 锁不外露）。
    pub fn new(map: Id, layer: Id, from: i64, initial_owner: Option<Id>) -> Self {
        Self {
            map,
            layer,
            from,
            initial_owner,
            before: std::sync::Mutex::new(None),
        }
    }

    /// 定位该 map × layer 的分区；无则建一份空分区挂进 `world`（调用方持可变借用）。
    fn partition_mut<'w>(
        &self,
        world: &'w mut World,
    ) -> &'w mut wv_core::model::faction_partition::FactionPartition {
        if let Some(i) = world
            .partitions
            .iter()
            .position(|p| p.map == self.map && p.layer == self.layer)
        {
            return &mut world.partitions[i];
        }
        world
            .partitions
            .push(wv_core::model::faction_partition::FactionPartition {
                map: self.map.clone(),
                layer: self.layer.clone(),
                spans: Vec::new(),
            });
        world.partitions.last_mut().expect("刚 push")
    }

    /// 造首段：整块地（最大外环）+ 湖洞。
    ///
    /// 坐标 = **栅格局部 wu**（终审 C1 修正：此前存 cell 索引、渲染侧少乘
    /// `cell_size` 画成缩微——正解是在存储侧就用真尺寸，渲染/手势/诊断三处
    /// 天然对齐同一 wu 口径）。`cell_size = MapNode.extent.width / 栅格宽`；
    /// 无 extent（测试 fixture / 老工程）回退 1.0。
    fn first_span(
        &self,
        world: &World,
    ) -> EditResult<wv_core::model::faction_partition::PartitionSpan> {
        let data = world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TargetMissing(format!("地图「{}」无地形栅格", self.map)))?;
        let grid = to_grid(data)?;
        let cell_size = world
            .maps
            .iter()
            .find(|m| m.id == self.map)
            .and_then(|m| m.extent.as_ref())
            .map(|e| e.width / data.width.max(1) as f32)
            .unwrap_or(1.0);
        // 取**面积最大**的外环为蛋糕主体（终审 I1 修正：此前只比面积符号，
        // 等于没比——多岛地图拿的是左上角的岛不是最大的）。
        let mut outlines = wv_terrain::outline::land_outlines(&grid, cell_size);
        outlines.sort_by(|a, b| {
            let area = |o: &wv_terrain::outline::LandOutline| -> f64 {
                let n = o.outer.len();
                let mut s = 0.0f64;
                for i in 0..n {
                    let (x1, y1) = o.outer[i];
                    let (x2, y2) = o.outer[(i + 1) % n];
                    s += x1 as f64 * y2 as f64 - x2 as f64 * y1 as f64;
                }
                s
            };
            area(b)
                .partial_cmp(&area(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(group) = outlines.first() else {
            return Err(EditError::TargetMissing(format!(
                "地图「{}」无陆地可分配（全海）",
                self.map
            )));
        };
        // 外环 → 顶点表 + 脸环。
        let n = group.outer.len();
        let mut vertices: Vec<PartitionVertex> = Vec::with_capacity(n);
        for (i, (x, y)) in group.outer.iter().enumerate() {
            vertices.push(PartitionVertex {
                id: i as u32,
                xy: [*x, *y],
            });
        }
        let ring: Vec<u32> = (0..n as u32).collect();
        // 洞：沿用外环坐标段编 id（续外环号）。
        let mut holes: Vec<Vec<u32>> = Vec::new();
        let mut next_id = n as u32;
        for hole in &group.holes {
            let mut h = Vec::with_capacity(hole.len());
            for (x, y) in hole {
                vertices.push(PartitionVertex {
                    id: next_id,
                    xy: [*x, *y],
                });
                h.push(next_id);
                next_id += 1;
            }
            holes.push(h);
        }
        Ok(PartitionSpan {
            from: self.from,
            to: None,
            vertices,
            faces: vec![PartitionFace {
                id: 0,
                owner: self.initial_owner.clone(),
                ring,
                holes,
            }],
        })
    }
}

impl EditCommand for BeginPartitionSpanCommand {
    fn label(&self) -> String {
        format!("势力层划分：自第 {} 天开段", self.from)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 快照在动任何东西之前抓（partition_mut 可能新建分区）。
        let snapshot = world
            .partitions
            .iter()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .cloned();
        let is_first = snapshot
            .as_ref()
            .map(|p| p.spans.is_empty())
            .unwrap_or(true);
        let new_span = if is_first {
            self.first_span(world)?
        } else {
            // fork：找命中 `from` 的段（from 必须落在首段之后）。
            let existing = snapshot.as_ref().expect("非首段必有分区");
            let hit = existing
                .spans
                .iter()
                .position(|s| self.from >= s.from && s.to.is_none_or(|to| self.from < to))
                .ok_or_else(|| {
                    EditError::Precondition(format!(
                        "生效日 {} 不在任何既有历史段内（首段自 {} 起）",
                        self.from, existing.spans[0].from
                    ))
                })?;
            if existing.spans[hit].from == self.from {
                // 恰在段起点 = 幂等：直接复用该段（不开零长度段）。
                // before 也要缓存（M1）——命令已入历史栈，revert 恢复同状
                // 态即可；不缓存会让 undo 时 revert Err。
                *self
                    .before
                    .lock()
                    .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
                return Ok(vec!["partitions".to_string()]);
            }
            let mut old = existing.spans[hit].clone();
            let mut fork = old.clone();
            old.to = Some(self.from);
            fork.from = self.from;
            fork.to = existing.spans[hit].to;
            let partition = self.partition_mut(world);
            partition.spans[hit] = old;
            partition.spans.insert(hit + 1, fork);
            *self
                .before
                .lock()
                .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
            return Ok(vec!["partitions".to_string()]);
        };
        // 首段：清空（空表不该有段，防御）后挂入。
        let partition = self.partition_mut(world);
        partition.spans.clear();
        partition.spans.push(new_span);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
        Ok(vec!["partitions".to_string()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snapshot = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("BeginPartitionSpan 尚未 apply".into()))?;
        let partition = self.partition_mut(world);
        *partition = snapshot;
        Ok(())
    }
}

/// 切刀几何（ADR-0074 D4）：一笔从边到边，把脸一分为二。
///
/// **切完点选归属**（D7）：`assign_at` 落在哪一半，那半 owner = `new_owner`，
/// 另一半保持原 owner。笔画两端必须落在该脸 ring 边上（吸附容差
/// [`CUT_SNAP_PX`] map-local px；命中已有顶点复用其 id）。
///
/// **就地改选中 span**（改旧语义，V2）：`span_from` 定位段。
pub const CUT_SNAP_PX: f32 = 4.0;

#[derive(Debug)]
pub struct CutPartitionFaceCommand {
    /// 目标地图。
    pub map: Id,
    /// 目标势力层。
    pub layer: Id,
    /// 被切的历史段（`span.from`）。
    pub span_from: i64,
    /// 被切的脸 id。
    pub face: u32,
    /// 切割笔画：map-local 扁平坐标 `[x0,y0,x1,y1,...]`（≥2 点 = 1 段）。
    pub stroke: Vec<f32>,
    /// 归属点选位：落在哪一半，那半归 `new_owner`。
    /// 新归属给哪一半：`true` = 弧 A（环上从刀口 1 到刀口 2 的那半），
    /// `false` = 弧 B。（实机再修：弹窗直接选侧，比点位判定稳——环顶点
    /// 上射线法不稳定。）
    pub assign_side_a: bool,
    /// 点选半的新归属；`None` = 无主。
    pub new_owner: Option<Id>,
    /// 端点吸附容差（**分区单位 = wu**）。屏幕 px 容差由调用方按当前缩放
    /// 折算（`px × view.scale`）——写死世界单位会在缩放下失效（实机教训：
    /// 4 wu 在 1px≈10wu 的缩放下比一个像素还小，永远吸不上）。
    pub snap_tol: f32,
    /// `apply` 缓存的整个分区快照。
    before: std::sync::Mutex<Option<wv_core::model::faction_partition::FactionPartition>>,
}

/// 笔画端点到边的吸附：返回 (环边序号 i, 边上参数 t ∈ [0,1])。
/// 端点到线段的最短距离 ≤ [`CUT_SNAP_PX`] 才算命中。
fn snap_endpoint_to_ring(
    ring_xy: &[(f32, f32)],
    endpoint: (f32, f32),
    snap_tol: f32,
) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32, f32)> = None; // (seg_i, t, dist2)
    let n = ring_xy.len();
    for i in 0..n {
        let a = ring_xy[i];
        let b = ring_xy[(i + 1) % n];
        let ab = (b.0 - a.0, b.1 - a.1);
        let ap = (endpoint.0 - a.0, endpoint.1 - a.1);
        let ab2 = ab.0 * ab.0 + ab.1 * ab.1;
        let t = if ab2 < 1e-12 {
            0.0
        } else {
            ((ap.0 * ab.0 + ap.1 * ab.1) / ab2).clamp(0.0, 1.0)
        };
        let cx = a.0 + ab.0 * t;
        let cy = a.1 + ab.1 * t;
        let d2 = (endpoint.0 - cx).powi(2) + (endpoint.1 - cy).powi(2);
        if d2 <= snap_tol * snap_tol && best.is_none_or(|(_, _, bd)| d2 < bd) {
            best = Some((i, t, d2));
        }
    }
    best.map(|(i, t, _)| (i, t))
}

impl CutPartitionFaceCommand {
    /// 构造（`before` 锁不外露）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        map: Id,
        layer: Id,
        span_from: i64,
        face: u32,
        stroke: Vec<f32>,
        assign_side_a: bool,
        new_owner: Option<Id>,
        snap_tol: f32,
    ) -> Self {
        Self {
            map,
            layer,
            span_from,
            face,
            stroke,
            assign_side_a,
            new_owner,
            snap_tol,
            before: std::sync::Mutex::new(None),
        }
    }

    fn partition_mut<'w>(
        &self,
        world: &'w mut World,
    ) -> EditResult<&'w mut wv_core::model::faction_partition::FactionPartition> {
        world
            .partitions
            .iter_mut()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .ok_or_else(|| {
                EditError::TargetMissing(format!("分区（{} × {}）", self.map, self.layer))
            })
    }

    /// 笔画端点吸附（首末交点）。
    fn stroke_endpoints(&self, ring_xy: &[(f32, f32)]) -> EditResult<[(usize, f32); 2]> {
        if self.stroke.len() < 4 {
            return Err(EditError::Precondition("笔画至少两点".into()));
        }
        let pts: Vec<(f32, f32)> = self.stroke.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        // 两级吸附：① 严格（snap_tol 内）直接用；② 失败则**投影**到最近的
        // 边界边（上限 snap_tol×20 = 调用方 4px 时的 80px）——切割工具的
        // 标准交互：作者画「从边到边」的意图线，端点不必精确压线。
        let project = |endpoint: (f32, f32)| -> Option<(usize, f32)> {
            snap_endpoint_to_ring(ring_xy, endpoint, self.snap_tol * 20.0)
        };
        let dist_to_ring = |endpoint: (f32, f32)| -> f32 {
            let mut best = f32::INFINITY;
            let n = ring_xy.len();
            for i in 0..n {
                let a = ring_xy[i];
                let b = ring_xy[(i + 1) % n];
                let ab = (b.0 - a.0, b.1 - a.1);
                let ap = (endpoint.0 - a.0, endpoint.1 - a.1);
                let ab2 = ab.0 * ab.0 + ab.1 * ab.1;
                let t = if ab2 < 1e-12 {
                    0.0
                } else {
                    ((ap.0 * ab.0 + ap.1 * ab.1) / ab2).clamp(0.0, 1.0)
                };
                let cx = a.0 + ab.0 * t;
                let cy = a.1 + ab.1 * t;
                best = best.min(((endpoint.0 - cx).powi(2) + (endpoint.1 - cy).powi(2)).sqrt());
            }
            best
        };
        let start = snap_endpoint_to_ring(ring_xy, pts[0], self.snap_tol)
            .or_else(|| project(pts[0]))
            .ok_or_else(|| {
                EditError::Precondition(format!(
                    "笔画起点离地块边界太远（最近 {:.0} wu > 上限 {:.0} wu；scale 折算容差 {} wu）",
                    dist_to_ring(pts[0]),
                    self.snap_tol * 20.0,
                    self.snap_tol
                ))
            })?;
        let end = snap_endpoint_to_ring(ring_xy, *pts.last().expect("≥2 点"), self.snap_tol)
            .or_else(|| project(*pts.last().expect("≥2 点")))
            .ok_or_else(|| {
                EditError::Precondition(format!(
                    "笔画终点离地块边界太远（最近 {:.0} wu > 上限 {:.0} wu）",
                    dist_to_ring(*pts.last().expect("≥2 点")),
                    self.snap_tol * 20.0,
                ))
            })?;
        Ok([start, end])
    }
}

impl EditCommand for CutPartitionFaceCommand {
    fn label(&self) -> String {
        format!("切分脸 {}（层 {}）", self.face, self.layer)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let snapshot = world
            .partitions
            .iter()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .cloned();
        let partition = self.partition_mut(world)?;
        let span = partition
            .spans
            .iter_mut()
            .find(|s| s.from == self.span_from)
            .ok_or_else(|| EditError::TargetMissing(format!("历史段 from={}", self.span_from)))?;
        let face_idx = span
            .faces
            .iter()
            .position(|f| f.id == self.face)
            .ok_or_else(|| EditError::TargetMissing(format!("脸 {}", self.face)))?;
        let ring_ids = span.faces[face_idx].ring.clone();
        let ring_xy: Vec<(f32, f32)> = ring_ids
            .iter()
            .map(|id| {
                span.vertices
                    .iter()
                    .find(|v| v.id == *id)
                    .map(|v| (v.xy[0], v.xy[1]))
                    .expect("ring 引用存在（validate_topology 保证）")
            })
            .collect();
        let [(i0, t0), (i1, t1)] = self.stroke_endpoints(&ring_xy)?;
        if (i0, t0) == (i1, t1) {
            return Err(EditError::Precondition("笔画两端落在同一点".into()));
        }
        // 刀口：交点在边中间（0<t<1）时新建顶点（id 续号）；吸附到既有顶点
        // 则复用其 id（共享顶点=相邻脸零和联动的基础）。
        //
        // 终审 I3 修正：全部几何在**副本**上算，所有校验过了才一次性写回——
        // 此前 cut_vertex 先 push 顶点、退化检查后置，Err 路径留下孤儿顶点
        // 落盘（违反「Err 不落盘」契约）。
        let mut next_vid = span.vertices.iter().map(|v| v.id).max().unwrap_or(0) + 1;
        let mut new_vertices: Vec<PartitionVertex> = Vec::new();
        let mut cut_vertex =
            |new_vertices: &mut Vec<PartitionVertex>, i: usize, t: f32| -> Result<u32, EditError> {
                let a = ring_ids[i];
                let b = ring_ids[(i + 1) % ring_ids.len()];
                if t <= 1e-6 {
                    return Ok(a);
                }
                if t >= 1.0 - 1e-6 {
                    return Ok(b);
                }
                let Some(v_a) = span.vertices.iter().find(|v| v.id == a) else {
                    return Err(EditError::TargetMissing(format!("环顶点 {a} 不存在")));
                };
                let (ax, ay) = (v_a.xy[0], v_a.xy[1]);
                let Some(v_b) = span.vertices.iter().find(|v| v.id == b) else {
                    return Err(EditError::TargetMissing(format!("环顶点 {b} 不存在")));
                };
                let (bx, by) = (v_b.xy[0], v_b.xy[1]);
                let vid = next_vid;
                next_vid += 1;
                new_vertices.push(PartitionVertex {
                    id: vid,
                    xy: [ax + (bx - ax) * t, ay + (by - ay) * t],
                });
                Ok(vid)
            };
        let v_start = cut_vertex(&mut new_vertices, i0, t0)?;
        let v_end = cut_vertex(&mut new_vertices, i1, t1)?;
        // 两刀口把环切成两条弧。弧 A：从刀口 1 沿环走到刀口 2。
        let mut arc_a: Vec<u32> = vec![v_start];
        let mut idx = (i0 + 1) % ring_ids.len();
        while idx != (i1 + 1) % ring_ids.len() {
            arc_a.push(ring_ids[idx]);
            idx = (idx + 1) % ring_ids.len();
        }
        arc_a.push(v_end);
        // 弧 B：从刀口 2 沿环走回刀口 1。
        let mut arc_b: Vec<u32> = vec![v_end];
        let mut idx = (i1 + 1) % ring_ids.len();
        while idx != (i0 + 1) % ring_ids.len() {
            arc_b.push(ring_ids[idx]);
            idx = (idx + 1) % ring_ids.len();
        }
        arc_b.push(v_start);
        if arc_a.len() < 3 || arc_b.len() < 3 {
            return Err(EditError::Precondition("切分退化（某半不足 3 点）".into()));
        }
        // 归属点选（D7）：弹窗直接指定哪半（assign_side_a）。
        // 终审 C2 根因修复：刀口顶点还在 new_vertices 里没写回——环坐标
        // 必须从「span ∪ new_vertices」解析，否则射线法丢边、归属判反。
        let resolve = |id: u32| -> (f32, f32) {
            span.vertices
                .iter()
                .chain(new_vertices.iter())
                .find(|v| v.id == id)
                .map(|v| (v.xy[0], v.xy[1]))
                .unwrap_or((f32::NAN, f32::NAN))
        };
        let arc_a_xy: Vec<(f32, f32)> = arc_a.iter().map(|id| resolve(*id)).collect();
        let in_a = self.assign_side_a;
        let old_owner = span.faces[face_idx].owner.clone();
        let (owner_a, owner_b) = if in_a {
            (self.new_owner.clone(), old_owner)
        } else {
            (old_owner, self.new_owner.clone())
        };
        // 终审 I2 修正：洞按首顶点落点**重派**到所属半——此前全留在被改的
        // 原脸，洞在外半 → earcut 乱织 + 湖面变成领土（违反海不参与切分）。
        let old_holes = span.faces[face_idx].holes.clone();
        let mut holes_a: Vec<Vec<u32>> = Vec::new();
        let mut holes_b: Vec<Vec<u32>> = Vec::new();
        for hole in &old_holes {
            let probe = span
                .vertices
                .iter()
                .find(|v| v.id == hole[0])
                .map(|v| [v.xy[0], v.xy[1]])
                .unwrap_or([f32::NAN; 2]);
            if point_in_ring_coords(probe, &arc_a_xy) {
                holes_a.push(hole.clone());
            } else {
                holes_b.push(hole.clone());
            }
        }
        // ── 全部校验已过，一次性写回 ──
        span.vertices.extend(new_vertices);
        span.faces[face_idx].ring = arc_a;
        span.faces[face_idx].owner = owner_a;
        span.faces[face_idx].holes = holes_a;
        span.faces.push(PartitionFace {
            id: span.faces.iter().map(|f| f.id).max().unwrap_or(0) + 1,
            owner: owner_b,
            ring: arc_b,
            holes: holes_b,
        });
        span.validate_topology()
            .map_err(|e| EditError::Precondition(format!("切后拓扑破损: {e}")))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
        Ok(vec!["partitions".to_string()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snapshot = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("CutPartitionFace 尚未 apply".into()))?;
        let partition = self.partition_mut(world)?;
        *partition = snapshot;
        Ok(())
    }
}

/// 射线法（f32 版）：点是否在**坐标环**内（终审 C2 修复的根基——环的
/// 坐标由调用方解析好传入，刀口顶点尚未写回 span 也能正确求值）。
fn point_in_ring_coords(p: [f32; 2], ring_xy: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = ring_xy.len();
    if n < 3 {
        return false;
    }
    let (px, py) = (p[0], p[1]);
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ring_xy[i];
        let (xj, yj) = ring_xy[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 零和拖边界（ADR-0074 D4/D2）：刚体平移共享边链上的顶点。
///
/// **每个被移动顶点必须被 ≥2 张脸引用**（共享顶点=零和构造保证，ADR-0074
/// D1）——单脸顶点被移动会在它与邻接边之间拉出缝/重叠，Err 拒绝。
///
/// **就地改选中 span**（改旧语义，V2）。
#[derive(Debug)]
pub struct MovePartitionVerticesCommand {
    /// 目标地图。
    pub map: Id,
    /// 目标势力层。
    pub layer: Id,
    /// 被改的历史段（`span.from`）。
    pub span_from: i64,
    /// 移动表：`(顶点 id, 新位置)`。
    pub moves: Vec<(u32, [f32; 2])>,
    /// `apply` 缓存的整个分区快照。
    pub before: std::sync::Mutex<Option<wv_core::model::faction_partition::FactionPartition>>,
}

/// 整块易主（ADR-0074 D8）：几何零改动，某脸自 `from` 天起换归属。
///
/// **fork 型**：在 `from` 处 fork 命中段（同 [`BeginPartitionSpanCommand`]
/// 的 fork 配方），fork 段中该脸 owner 换新值；前段求值不变、后段不受传染
/// （每段是独立快照——改旧不重放，V2）。
#[derive(Debug)]
pub struct AssignPartitionFaceCommand {
    /// 目标地图。
    pub map: Id,
    /// 目标势力层。
    pub layer: Id,
    /// 生效日（fork 点）。
    pub from: i64,
    /// 被易主的脸。
    pub face: u32,
    /// 新归属；`None` = 无主。
    pub owner: Option<Id>,
    /// `apply` 缓存的整个分区快照。
    pub before: std::sync::Mutex<Option<wv_core::model::faction_partition::FactionPartition>>,
}

/// 顶点 id → 引用它的脸数（一个 span 内）。
fn face_ref_count(span: &PartitionSpan, vid: u32) -> usize {
    span.faces
        .iter()
        .filter(|f| f.ring.contains(&vid) || f.holes.iter().any(|h| h.contains(&vid)))
        .count()
}

impl MovePartitionVerticesCommand {
    fn partition_mut<'w>(
        &self,
        world: &'w mut World,
    ) -> EditResult<&'w mut wv_core::model::faction_partition::FactionPartition> {
        world
            .partitions
            .iter_mut()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .ok_or_else(|| {
                EditError::TargetMissing(format!("分区（{} × {}）", self.map, self.layer))
            })
    }
}

impl EditCommand for MovePartitionVerticesCommand {
    fn label(&self) -> String {
        format!("拖边界（{} 个顶点）", self.moves.len())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let snapshot = world
            .partitions
            .iter()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .cloned();
        // 零和预检：全部顶点都须是共享顶点（≥2 脸引用），一票否决。
        {
            let partition = snapshot.as_ref().ok_or_else(|| {
                EditError::TargetMissing(format!("分区（{} × {}）", self.map, self.layer))
            })?;
            let span = partition
                .spans
                .iter()
                .find(|s| s.from == self.span_from)
                .ok_or_else(|| {
                    EditError::TargetMissing(format!("历史段 from={}", self.span_from))
                })?;
            for (vid, _) in &self.moves {
                if face_ref_count(span, *vid) < 2 {
                    return Err(EditError::Precondition(format!(
                        "顶点 {vid} 只被 1 张脸引用——移动它会拉出缝（零和被破坏）"
                    )));
                }
            }
        }
        let partition = self.partition_mut(world)?;
        let span = partition
            .spans
            .iter_mut()
            .find(|s| s.from == self.span_from)
            .ok_or_else(|| EditError::TargetMissing(format!("历史段 from={}", self.span_from)))?;
        for (vid, xy) in &self.moves {
            let v = span
                .vertices
                .iter_mut()
                .find(|v| v.id == *vid)
                .ok_or_else(|| EditError::TargetMissing(format!("顶点 {vid}")))?;
            v.xy = *xy;
        }
        span.validate_topology()
            .map_err(|e| EditError::Precondition(format!("移动后拓扑破损: {e}")))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
        Ok(vec!["partitions".to_string()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snapshot = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("MovePartitionVertices 尚未 apply".into()))?;
        let partition = self.partition_mut(world)?;
        *partition = snapshot;
        Ok(())
    }
}

impl AssignPartitionFaceCommand {
    fn partition_mut<'w>(
        &self,
        world: &'w mut World,
    ) -> EditResult<&'w mut wv_core::model::faction_partition::FactionPartition> {
        world
            .partitions
            .iter_mut()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .ok_or_else(|| {
                EditError::TargetMissing(format!("分区（{} × {}）", self.map, self.layer))
            })
    }
}

impl EditCommand for AssignPartitionFaceCommand {
    fn label(&self) -> String {
        format!("脸 {} 易主（自第 {} 天）", self.face, self.from)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let snapshot = world
            .partitions
            .iter()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .cloned();
        let existing = snapshot
            .as_ref()
            .ok_or_else(|| {
                EditError::TargetMissing(format!("分区（{} × {}）", self.map, self.layer))
            })?
            .clone();
        let hit = existing
            .spans
            .iter()
            .position(|s| self.from >= s.from && s.to.is_none_or(|to| self.from < to))
            .ok_or_else(|| {
                EditError::Precondition(format!("生效日 {} 不在任何历史段内", self.from))
            })?;
        // 几何在 fork 副本上改，最后一次性写回（避免双可变借世界）。
        let mut new_spans = existing.spans.clone();
        if existing.spans[hit].from == self.from {
            // 恰在段起点：直接改该段（不开零长度段）。
            let face = new_spans[hit]
                .faces
                .iter_mut()
                .find(|f| f.id == self.face)
                .ok_or_else(|| EditError::TargetMissing(format!("脸 {}", self.face)))?;
            face.owner = self.owner.clone();
        } else {
            // fork：旧段收口、fork 段换归属。
            let mut old = new_spans[hit].clone();
            let mut fork = old.clone();
            old.to = Some(self.from);
            fork.from = self.from;
            fork.to = new_spans[hit].to;
            let face = fork
                .faces
                .iter_mut()
                .find(|f| f.id == self.face)
                .ok_or_else(|| EditError::TargetMissing(format!("脸 {}", self.face)))?;
            face.owner = self.owner.clone();
            new_spans[hit] = old;
            new_spans.insert(hit + 1, fork);
        }
        let partition = self.partition_mut(world)?;
        partition.spans = new_spans;
        partition
            .spans
            .last()
            .unwrap()
            .validate_topology()
            .map_err(|e| EditError::Precondition(format!("易主后拓扑破损: {e}")))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
        Ok(vec!["partitions".to_string()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snapshot = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("AssignPartitionFace 尚未 apply".into()))?;
        let partition = self.partition_mut(world)?;
        *partition = snapshot;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    /// 3×3 陆块、中心一格水（湖），cell=10。整图 8×8。
    fn world_with_island_lake() -> World {
        let mut grid = wv_terrain::TerrainGrid::new(8, 8);
        for y in 3..6 {
            for x in 3..6 {
                grid.set(
                    x,
                    y,
                    wv_terrain::TerrainCell {
                        height: 100,
                        surface: wv_terrain::SURFACE_LAND,
                        biome: 0,
                    },
                );
            }
        }
        grid.set(
            4,
            4,
            wv_terrain::TerrainCell {
                height: -10,
                surface: wv_terrain::SURFACE_WATER,
                biome: 0,
            },
        );
        let mut world = World::default();
        world.set_terrain(test_map(), from_grid(grid));
        world
    }

    /// 已有一段 `[0,None)` 的分区（单脸单方环）。
    fn world_with_existing_span() -> World {
        let mut world = World::default();
        world
            .partitions
            .push(wv_core::model::faction_partition::FactionPartition {
                map: test_map(),
                layer: Id::new_unchecked("layer_borders"),
                spans: vec![PartitionSpan {
                    from: 0,
                    to: None,
                    vertices: vec![
                        PartitionVertex {
                            id: 0,
                            xy: [0.0, 0.0],
                        },
                        PartitionVertex {
                            id: 1,
                            xy: [10.0, 0.0],
                        },
                        PartitionVertex {
                            id: 2,
                            xy: [10.0, 10.0],
                        },
                    ],
                    faces: vec![PartitionFace {
                        id: 0,
                        owner: Some(Id::new_unchecked("faction_a")),
                        ring: vec![0, 1, 2],
                        holes: Vec::new(),
                    }],
                }],
            });
        world
    }

    #[test]
    fn first_span_is_whole_land_with_lake_hole() {
        let mut world = world_with_island_lake();
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            0,
            Some(Id::new_unchecked("faction_a")),
        );
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec!["partitions".to_string()], "脏键应为 partitions");

        let p = &world.partitions[0];
        assert_eq!(p.map, test_map());
        assert_eq!(p.layer, Id::new_unchecked("layer_borders"));
        assert_eq!(p.spans.len(), 1, "首段");
        let span = &p.spans[0];
        assert_eq!(span.from, 0);
        assert_eq!(span.to, None, "首段至今");
        // 3×3 陆块 → 外环抽稀后 4 角；中心湖 → 1 洞（4 角）。
        assert_eq!(span.faces.len(), 1, "单脸");
        let face = &span.faces[0];
        assert_eq!(face.ring.len(), 4, "3×3 外环应抽稀到 4 角: {:?}", face.ring);
        assert_eq!(face.holes.len(), 1, "湖应成洞: {:?}", face.holes);
        assert_eq!(
            face.owner,
            Some(Id::new_unchecked("faction_a")),
            "初始归属写入"
        );
        span.validate_topology().expect("生成的首段应过拓扑自检");
    }

    #[test]
    fn first_span_owner_none_is_unclaimed() {
        let mut world = world_with_island_lake();
        let cmd =
            BeginPartitionSpanCommand::new(test_map(), Id::new_unchecked("layer_borders"), 5, None);
        cmd.apply(&mut world).expect("apply");
        assert_eq!(
            world.partitions[0].spans[0].faces[0].owner, None,
            "无主 = owner None，不造合成势力实体"
        );
    }

    #[test]
    fn fork_closes_old_span_and_copies_snapshot() {
        let mut world = world_with_existing_span();
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            10,
            None,
        );
        cmd.apply(&mut world).expect("apply");
        let p = &world.partitions[0];
        assert_eq!(p.spans.len(), 2, "fork 出两段");
        assert_eq!(
            (p.spans[0].from, p.spans[0].to),
            (0, Some(10)),
            "旧段收口于 fork 日"
        );
        assert_eq!(
            (p.spans[1].from, p.spans[1].to),
            (10, None),
            "新段自 fork 日至今"
        );
        // 快照复制：顶点/脸逐位相等（id 保持——共享引用的零和基础）。
        assert_eq!(p.spans[0].vertices, p.spans[1].vertices, "顶点快照复制");
        assert_eq!(p.spans[0].faces, p.spans[1].faces, "脸快照复制");
    }

    #[test]
    fn fork_day_before_first_span_errors() {
        let mut world = world_with_existing_span(); // 首段 [0,None)
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            -5,
            None,
        );
        assert!(
            cmd.apply(&mut world).is_err(),
            "fork 日在首段之前无历史可承，应 Err 不落盘"
        );
        assert!(world.partitions[0].spans.len() == 1, "Err 时不得改动世界");
    }

    #[test]
    fn begin_without_terrain_errors() {
        let mut world = World::default(); // 无栅格
        let cmd =
            BeginPartitionSpanCommand::new(test_map(), Id::new_unchecked("layer_borders"), 0, None);
        assert!(
            cmd.apply(&mut world).is_err(),
            "无自有地形栅格的图造不出蛋糕，应 Err"
        );
    }

    #[test]
    fn revert_restores_previous_partition_state() {
        let mut world = world_with_existing_span();
        let before = world.partitions.clone();
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            10,
            None,
        );
        cmd.apply(&mut world).expect("apply");
        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.partitions, before,
            "revert 应恢复到 apply 前的分区状态"
        );
    }
}

#[cfg(test)]
mod cut_tests {
    //! 切刀命令（ADR-0074 T8）——手算坐标钉行为。

    use super::*;
    use wv_core::model::faction_partition::FactionPartition;

    fn test_map() -> Id {
        Id::new_unchecked("world_main")
    }

    /// 单脸方环 (0,0)-(100,0)-(100,100)-(0,100)，无主。
    fn world_with_square() -> World {
        let mut world = World::default();
        world.partitions.push(FactionPartition {
            map: test_map(),
            layer: Id::new_unchecked("layer_borders"),
            spans: vec![PartitionSpan {
                from: 0,
                to: None,
                vertices: vec![
                    PartitionVertex {
                        id: 0,
                        xy: [0.0, 0.0],
                    },
                    PartitionVertex {
                        id: 1,
                        xy: [100.0, 0.0],
                    },
                    PartitionVertex {
                        id: 2,
                        xy: [100.0, 100.0],
                    },
                    PartitionVertex {
                        id: 3,
                        xy: [0.0, 100.0],
                    },
                ],
                faces: vec![PartitionFace {
                    id: 0,
                    owner: Some(Id::new_unchecked("faction_a")),
                    ring: vec![0, 1, 2, 3],
                    holes: Vec::new(),
                }],
            }],
        });
        world
    }

    const LAYER: &str = "layer_borders";

    #[test]
    fn vertical_cut_splits_square_into_two_rings() {
        let mut world = world_with_square();
        // 竖切 x=50：起点 (50,-3) 吸到下边 (0-1 边)，终点 (50,103) 吸到上边 (2-3 边)。
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![50.0, -3.0, 50.0, 103.0],
            false,
            Some(Id::new_unchecked("faction_b")),
            4.0,
        );
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec!["partitions".to_string()]);
        let span = &world.partitions[0].spans[0];
        assert_eq!(span.faces.len(), 2, "一分为二");
        // 两环共享两个新顶点（id 4=下刀口、5=上刀口）——零和构造基础。
        let ring_a = &span.faces[0].ring;
        let ring_b = &span.faces[1].ring;
        assert_eq!(ring_a.len(), 4, "半方环 4 点: {ring_a:?}");
        assert_eq!(ring_b.len(), 4, "半方环 4 点: {ring_b:?}");
        let shared: std::collections::HashSet<_> =
            ring_a.iter().filter(|id| ring_b.contains(id)).collect();
        assert_eq!(shared.len(), 2, "两环共享两刀口顶点: {shared:?}");
        // 点选 (30,50) 在左半（x<50）→ 左半归 faction_b；右半保持 faction_a。
        // 脸的数组序不保证（faces[0] 是被改的原脸），按环几何判左右。
        let (xb, yb) = span
            .vertices
            .iter()
            .find(|v| v.id == span.faces[0].ring[0])
            .map(|v| (v.xy[0], v.xy[1]))
            .expect("顶点存在");
        let face0_is_left = xb < 50.0;
        let face0_owner = &span.faces[0].owner;
        let face1_owner = &span.faces[1].owner;
        let picked = Some(Id::new_unchecked("faction_b"));
        let kept = Some(Id::new_unchecked("faction_a"));
        assert_eq!(
            face0_owner,
            &(if face0_is_left {
                picked.clone()
            } else {
                kept.clone()
            }),
            "点选侧（左半）应归 new_owner"
        );
        assert_eq!(
            face1_owner,
            &(if face0_is_left { kept } else { picked }),
            "非点选侧保持原归属"
        );
        let _ = (yb, 0.0);
        span.validate_topology().expect("切后拓扑自检");
    }

    #[test]
    fn assign_at_decides_owner_side() {
        let mut world = world_with_square();
        // 同一笔画，点选换到右半。
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![50.0, -3.0, 50.0, 103.0],
            true,
            Some(Id::new_unchecked("faction_b")),
            4.0,
        );
        cmd.apply(&mut world).expect("apply");
        let span = &world.partitions[0].spans[0];
        let owners: std::collections::HashSet<_> =
            span.faces.iter().map(|f| f.owner.clone()).collect();
        assert!(
            owners.contains(&Some(Id::new_unchecked("faction_b")))
                && owners.contains(&Some(Id::new_unchecked("faction_a"))),
            "两半各归其主: {owners:?}"
        );
    }

    #[test]
    fn stroke_endpoint_inside_face_errors() {
        let mut world = world_with_square();
        // 终点 (50,50) 在面内部（距任何边 > 4px）→ Err。
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![50.0, -3.0, 50.0, 50.0],
            false,
            None,
            4.0,
        );
        assert!(cmd.apply(&mut world).is_err(), "端点不入边应 Err");
        assert_eq!(
            world.partitions[0].spans[0].faces.len(),
            1,
            "Err 时脸数不变"
        );
    }

    #[test]
    fn endpoint_inside_face_projects_to_nearest_edge() {
        // 端点在面内部 30px（>4px 严格容差，<80px 投影上限）→ 自动吸到
        // 最近边界，切分成功。
        let mut world = world_with_square();
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![30.0, 10.0, 70.0, 90.0],
            false,
            Some(Id::new_unchecked("faction_b")),
            4.0,
        );
        let dirty = cmd.apply(&mut world).expect("投影后应切分成功");
        assert_eq!(dirty, vec!["partitions".to_string()]);
        let span = &world.partitions[0].spans[0];
        assert_eq!(span.faces.len(), 2, "一分为二");
        span.validate_topology().expect("投影切分后拓扑自检");
    }

    #[test]
    fn endpoint_far_beyond_project_limit_errors() {
        // 端点深入面内部超过投影上限（snap_tol=1 → 上限 20px；端点距边
        // 45px+）→ 仍 Err。
        let mut world = world_with_square();
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![50.0, 45.0, 52.0, 55.0],
            true,
            None,
            1.0,
        );
        assert!(cmd.apply(&mut world).is_err(), "超出投影上限应 Err");
        assert_eq!(world.partitions[0].spans[0].faces.len(), 1);
    }

    #[test]
    fn cut_reuses_existing_vertex_id_when_snapped() {
        let mut world = world_with_square();
        // 起点吸到既有顶点 1 (100,0)：从顶点 1 切到对边中点。
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![100.0, 0.0, 0.0, 50.0],
            true,
            None,
            4.0,
        );
        cmd.apply(&mut world).expect("apply");
        let span = &world.partitions[0].spans[0];
        // 顶点数：4 既有 + 1 新刀口（终点吸边中点）。
        assert_eq!(span.vertices.len(), 5, "只新增一个刀口顶点");
        // 环 0 含既有顶点 1（复用 id，无重复新建）。
        assert!(
            span.faces[0].ring.contains(&1) || span.faces[1].ring.contains(&1),
            "既有顶点 id 应被复用"
        );
    }

    #[test]
    fn cut_reassigns_holes_to_the_side_they_land_in() {
        // 终审 I2：湖在右半 → 切后洞必须跟去右半（左半无洞）。
        let mut world = World::default();
        world.partitions.push(FactionPartition {
            map: test_map(),
            layer: Id::new_unchecked(LAYER),
            spans: vec![PartitionSpan {
                from: 0,
                to: None,
                vertices: vec![
                    PartitionVertex {
                        id: 0,
                        xy: [0.0, 0.0],
                    },
                    PartitionVertex {
                        id: 1,
                        xy: [100.0, 0.0],
                    },
                    PartitionVertex {
                        id: 2,
                        xy: [100.0, 100.0],
                    },
                    PartitionVertex {
                        id: 3,
                        xy: [0.0, 100.0],
                    },
                    PartitionVertex {
                        id: 4,
                        xy: [75.0, 40.0],
                    },
                    PartitionVertex {
                        id: 5,
                        xy: [85.0, 50.0],
                    },
                    PartitionVertex {
                        id: 6,
                        xy: [75.0, 60.0],
                    },
                ],
                faces: vec![PartitionFace {
                    id: 0,
                    owner: Some(Id::new_unchecked("faction_a")),
                    ring: vec![0, 1, 2, 3],
                    holes: vec![vec![4, 5, 6]], // 湖在 x∈[75,85] —— 右半
                }],
            }],
        });
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![50.0, -3.0, 50.0, 103.0],
            false, // 弧 B（左半）拿新归属 → 右半保持 faction_a
            Some(Id::new_unchecked("faction_b")),
            4.0,
        );
        cmd.apply(&mut world).expect("apply");
        let span = &world.partitions[0].spans[0];
        let with_holes: Vec<_> = span.faces.iter().filter(|f| !f.holes.is_empty()).collect();
        assert_eq!(with_holes.len(), 1, "恰有一张脸带洞: {:?}", span.faces);
        // 带洞脸必须是右半（含 x=75..85 的顶点）。
        let ring_max_x = with_holes[0]
            .ring
            .iter()
            .filter_map(|id| span.vertices.iter().find(|v| v.id == *id))
            .map(|v| v.xy[0])
            .fold(f32::MIN, f32::max);
        assert!(
            ring_max_x > 60.0,
            "洞应跟去右半（ring max x={}）",
            ring_max_x
        );
        span.validate_topology().expect("洞重派后拓扑自检");
    }

    #[test]
    fn cut_revert_restores_snapshot() {
        let mut world = world_with_square();
        let before = world.partitions.clone();
        let cmd = CutPartitionFaceCommand::new(
            test_map(),
            Id::new_unchecked(LAYER),
            0,
            0,
            vec![50.0, -3.0, 50.0, 103.0],
            false,
            None,
            4.0,
        );
        cmd.apply(&mut world).expect("apply");
        cmd.revert(&mut world).expect("revert");
        assert_eq!(world.partitions, before, "revert 应整段还原");
    }
}

#[cfg(test)]
mod move_assign_tests {
    //! 零和拖顶点 + 整块易主（ADR-0074 T9）。

    use super::*;
    use wv_core::model::faction_partition::FactionPartition;

    fn test_map() -> Id {
        Id::new_unchecked("world_main")
    }

    /// 方环被竖切成两半（复用 T8 的拓扑结果，直接手摆）：
    /// 脸 R：[1,2,5,4]（右半）；脸 L：[4,5,3,0]（左半）。共享 4、5。
    fn world_with_halves() -> World {
        let mut world = World::default();
        world.partitions.push(FactionPartition {
            map: test_map(),
            layer: Id::new_unchecked("layer_borders"),
            spans: vec![PartitionSpan {
                from: 0,
                to: None,
                vertices: vec![
                    PartitionVertex {
                        id: 0,
                        xy: [0.0, 0.0],
                    },
                    PartitionVertex {
                        id: 1,
                        xy: [100.0, 0.0],
                    },
                    PartitionVertex {
                        id: 2,
                        xy: [100.0, 100.0],
                    },
                    PartitionVertex {
                        id: 3,
                        xy: [0.0, 100.0],
                    },
                    PartitionVertex {
                        id: 4,
                        xy: [50.0, 0.0],
                    },
                    PartitionVertex {
                        id: 5,
                        xy: [50.0, 100.0],
                    },
                ],
                faces: vec![
                    PartitionFace {
                        id: 0,
                        owner: Some(Id::new_unchecked("faction_a")),
                        ring: vec![4, 5, 3, 0],
                        holes: Vec::new(),
                    },
                    PartitionFace {
                        id: 1,
                        owner: Some(Id::new_unchecked("faction_b")),
                        ring: vec![1, 2, 5, 4],
                        holes: Vec::new(),
                    },
                ],
            }],
        });
        world
    }

    const LAYER: &str = "layer_borders";

    #[test]
    fn moving_shared_vertex_moves_both_faces_rings() {
        let mut world = world_with_halves();
        // 拖共享顶点 4（底刀口）到 (60, 0)。
        let cmd = MovePartitionVerticesCommand {
            map: test_map(),
            layer: Id::new_unchecked(LAYER),
            span_from: 0,
            moves: vec![(4, [60.0, 0.0])],
            before: std::sync::Mutex::new(None),
        };
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec!["partitions".to_string()]);
        let span = &world.partitions[0].spans[0];
        let v4 = span.vertices.iter().find(|v| v.id == 4).expect("顶点 4");
        assert_eq!(v4.xy, [60.0, 0.0], "顶点本体移动");
        // 两环引用同一 id → 两侧同时生效（零和：面积一侧增一侧减，无缝）。
        assert!(span.faces[0].ring.contains(&4) && span.faces[1].ring.contains(&4));
        span.validate_topology().expect("移动不改拓扑");
    }

    #[test]
    fn moving_single_face_vertex_errors() {
        let mut world = world_with_halves();
        // 顶点 2 只被脸 1 引用（右上角）——单侧移动会拉出缝。
        let cmd = MovePartitionVerticesCommand {
            map: test_map(),
            layer: Id::new_unchecked(LAYER),
            span_from: 0,
            moves: vec![(2, [120.0, 100.0])],
            before: std::sync::Mutex::new(None),
        };
        assert!(cmd.apply(&mut world).is_err(), "单脸顶点移动应 Err");
        let v2 = world.partitions[0].spans[0]
            .vertices
            .iter()
            .find(|v| v.id == 2)
            .expect("顶点 2");
        assert_eq!(v2.xy, [100.0, 100.0], "Err 不落盘");
    }

    #[test]
    fn assign_forks_span_and_changes_owner_only() {
        let mut world = world_with_halves();
        // 三段历史：再 fork 出 [20,None) 便于验证「改中段不传染后段」。
        let cmd0 = BeginPartitionSpanCommand::new(test_map(), Id::new_unchecked(LAYER), 20, None);
        cmd0.apply(&mut world).expect("fork 中间段");
        // 整块易主：脸 0（左半）自第 10 天归 faction_c。
        let cmd = AssignPartitionFaceCommand {
            map: test_map(),
            layer: Id::new_unchecked(LAYER),
            from: 10,
            face: 0,
            owner: Some(Id::new_unchecked("faction_c")),
            before: std::sync::Mutex::new(None),
        };
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec!["partitions".to_string()]);
        let p = &world.partitions[0];
        assert_eq!(p.spans.len(), 3, "fork 后三段");
        // 前段 [0,10)：脸 0 仍 faction_a。
        assert_eq!(
            p.spans[0].faces[0].owner,
            Some(Id::new_unchecked("faction_a")),
            "fork 前段求值不变"
        );
        // 中段 [10,20)：脸 0 → faction_c（易主生效）。
        assert_eq!(
            p.spans[1].faces[0].owner,
            Some(Id::new_unchecked("faction_c")),
            "fork 段换归属"
        );
        // 后段 [20,None)：仍 faction_a（改中段不传染后段——Review Focus 1）。
        assert_eq!(
            p.spans[2].faces[0].owner,
            Some(Id::new_unchecked("faction_a")),
            "改中段不得传染后段"
        );
        // 几何零改动。
        assert_eq!(p.spans[0].vertices, p.spans[1].vertices, "几何不动");
    }

    #[test]
    fn assign_before_first_span_errors() {
        let mut world = world_with_halves();
        // 早于首段（首段自 0 起）无历史可承 → Err（与 Begin fork 同判）。
        let cmd = AssignPartitionFaceCommand {
            map: test_map(),
            layer: Id::new_unchecked(LAYER),
            from: -5,
            face: 0,
            owner: None,
            before: std::sync::Mutex::new(None),
        };
        assert!(cmd.apply(&mut world).is_err(), "早于首段应 Err");
        assert_eq!(world.partitions[0].spans.len(), 1, "Err 不落盘");
    }

    #[test]
    fn assign_at_far_future_hits_open_ended_span() {
        let mut world = world_with_halves();
        // to=None = 至今：from=999 命中该段并 fork（合理语义，非错误）。
        let cmd = AssignPartitionFaceCommand {
            map: test_map(),
            layer: Id::new_unchecked(LAYER),
            from: 999,
            face: 0,
            owner: Some(Id::new_unchecked("faction_c")),
            before: std::sync::Mutex::new(None),
        };
        cmd.apply(&mut world).expect("开放段内随时可 fork");
        assert_eq!(world.partitions[0].spans.len(), 2);
    }
}
