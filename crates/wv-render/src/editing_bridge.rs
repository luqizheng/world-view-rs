//! 渲染层 → 应用层的编辑意图桥（ADR-0020 §2.5）。
//!
//! 渲染层**不**持有 `wv-edit::EditHistory`，也**不**构造 `EditCommand`——
//! 那是应用层的职责（AGENTS §2.1 分层红线：`wv-render` 禁止反向依赖 `wv-edit`）。
//! 本模块只定义两个方向相反的队列：
//!
//! - [`EditIntents`]：渲染 System 提交的「想在世界上做什么」的**意图**（发出去）；
//! - [`EditReceipts`]：应用层翻译器回填的「哪些意图已成功提交」**回执**（收回来）。
//!
//! ## 单帧三段流程（顺序由 `wv-app` 的 `app_bootstrap` 用 `.chain()` 钉死）
//!
//! 1. 渲染输入系统（`streets::commit_street_click` / `measurement::commit_measurement_click`）
//!    push 意图 + 记挂起回执（各自的 `*CommitAcks`）；
//! 2. `wv-app::edit::intent_translator::translate_edit_intents` 排空意图 →
//!    构造 `wv-edit` 的 `EditCommand` → `History.commit` → 成功者写回执 +
//!    `projection.dirty = true`；
//! 3. 渲染回执系统（`streets::apply_street_receipts` /
//!    `measurement::apply_measurement_receipts`）消费回执，做**纯本地**的状态回填
//!    （草稿推进 / 连线锚点）。
//!
//! ## 为什么需要回执，而不是「先改本地、失败再回滚」
//!
//! 旧实现（wv-app 时代）commit 失败时不推进草稿 / 锚点。拆 crate 后提交是异步的
//! （跨过一个翻译系统），回执把「失败不动本地」这一行为**原样**保留——
//! 这是 ADR-0020 闸门 8（行为零变化）的组成部分，不是实现细节。

use bevy::prelude::*;
use wv_core::id::Id;
use wv_core::{Measurement, StreetSegment};

/// 街道落笔意图：在 `layer_id` 图层追加一段。
#[derive(Debug, Clone)]
pub struct StreetSegmentIntent {
    /// 目标图层 id（`MapNode.layers[].id`）。
    pub layer_id: Id,
    /// 新段（**本图局部**坐标）。
    pub segment: StreetSegment,
}

/// 作者手绘小河意图：在 `map_id` 图上新建一个带折线的地点（ADR-0046）。
///
/// 折线是**图内局部 wu**（与 `Place.line` 同一坐标口径）。
#[derive(Debug, Clone)]
pub struct PlaceLineIntent {
    /// 目标图 id。
    pub map_id: Id,
    /// 地点名（空串 = 翻译层给默认名）。
    pub name: String,
    /// 折线点列（**图内局部 wu**，扁平 `[x0,y0,x1,y1,...]`）。
    pub line: Vec<f32>,
}

/// 测量编辑动作。
#[derive(Debug, Clone)]
pub enum MeasurementOp {
    /// 新增一条测量。
    Add(Measurement),
    /// 按 id 删除一条测量（橡皮）。
    Remove(Id),
}

/// 测量编辑意图：在 `map_id` 图上做一次测量增/删。
#[derive(Debug, Clone)]
pub struct MeasurementIntent {
    /// 目标图 id。
    pub map_id: Id,
    /// 增 / 删。
    pub op: MeasurementOp,
}

/// 渲染层提交的编辑意图队列（每帧由应用层翻译器排空）。
///
/// 序号 `seq` 单调递增，是回执的凭据：渲染侧持 seq 记挂起动作，
/// 翻译器成功后按 seq 回执，渲染侧凭 seq 回填本地状态。
#[derive(Resource, Default)]
pub struct EditIntents {
    next_seq: u64,
    street_segments: Vec<(u64, StreetSegmentIntent)>,
    measurements: Vec<(u64, MeasurementIntent)>,
    place_lines: Vec<(u64, PlaceLineIntent)>,
}

impl EditIntents {
    /// 提交一条街道意图，返回其序号（回执凭据）。
    pub fn push_street(&mut self, intent: StreetSegmentIntent) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.street_segments.push((seq, intent));
        seq
    }

    /// 提交一条测量意图，返回其序号（回执凭据）。
    pub fn push_measurement(&mut self, intent: MeasurementIntent) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.measurements.push((seq, intent));
        seq
    }

    /// 提交一条作者小河意图，返回其序号（回执凭据）。
    pub fn push_place_line(&mut self, intent: PlaceLineIntent) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.place_lines.push((seq, intent));
        seq
    }

    /// 排空全部作者小河意图（应用层翻译器专用）。
    pub fn drain_place_lines(&mut self) -> Vec<(u64, PlaceLineIntent)> {
        std::mem::take(&mut self.place_lines)
    }

    /// 排空全部街道意图（应用层翻译器专用）。
    pub fn drain_streets(&mut self) -> Vec<(u64, StreetSegmentIntent)> {
        std::mem::take(&mut self.street_segments)
    }

    /// 排空全部测量意图（应用层翻译器专用）。
    pub fn drain_measurements(&mut self) -> Vec<(u64, MeasurementIntent)> {
        std::mem::take(&mut self.measurements)
    }
}

/// 本帧成功提交的意图序号（应用层翻译器写、渲染回执系统读）。
///
/// 帧内生命周期：翻译器开头 `clear()`（冲掉上一帧残余）→ 成功者 `commit(seq)` →
/// 渲染回执系统 `take(seq)`（读走即删，防重复回填）。
#[derive(Resource, Default)]
pub struct EditReceipts {
    committed: Vec<u64>,
}

impl EditReceipts {
    /// 回执一条已成功提交的意图（应用层翻译器专用）。
    pub fn commit(&mut self, seq: u64) {
        self.committed.push(seq);
    }

    /// 清空上一帧的回执（应用层翻译器每帧开头调用）。
    pub fn clear(&mut self) {
        self.committed.clear();
    }

    /// 查询并**消费**某序号的回执（渲染回执系统专用）。重复查询返回 `false`。
    pub fn take(&mut self, seq: u64) -> bool {
        if let Some(pos) = self.committed.iter().position(|&s| s == seq) {
            self.committed.swap_remove(pos);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_take_consumes_exactly_once() {
        let mut r = EditReceipts::default();
        r.commit(1);
        r.commit(2);
        assert!(r.take(1));
        assert!(!r.take(1), "回执只应被消费一次");
        assert!(r.take(2));
        assert!(!r.take(2));
    }

    #[test]
    fn intents_alloc_monotonic_seqs_and_drain_empties_the_queue() {
        let mut i = EditIntents::default();
        let a = i.push_street(StreetSegmentIntent {
            layer_id: Id::new_unchecked("roads"),
            segment: StreetSegment::straight(wv_core::RoadClass::Primary, 0.0, 0.0, 1.0, 0.0),
        });
        let b = i.push_measurement(MeasurementIntent {
            map_id: Id::new_unchecked("m"),
            op: MeasurementOp::Remove(Id::new_unchecked("x")),
        });
        assert_eq!(a, 0);
        assert_eq!(b, 1, "seq 应单调递增");
        assert_eq!(i.drain_streets().len(), 1);
        assert_eq!(i.drain_streets().len(), 0, "排空后应清零");
        assert_eq!(i.drain_measurements().len(), 1);
    }
}
