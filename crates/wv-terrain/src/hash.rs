//! 确定性整数格点哈希——渲染 / 生成层共用的「同图两次运行逐位一致」来源。
//!
//! ## 为什么集中在一处
//!
//! 项目硬约束（ADR-0059 / SPEC §3.8.6 守护项 8）：程序化派生量（符号摆位、
//! 格点抖动、边界扰动、纸纹）在**两次运行间必须逐位一致**，否则重开地图时
//! 这些量会「跳动」。此前 `terrain_view::cells`（wv-world-cell-map）与
//! `paper_overlay`（wv-render）各持一份**逐字相同**的实现，注释却都写着
//! 「独立副本」——一旦一处微调（如换末位常量），另一处不会同步，而视觉只在
//! 其中一层变化，排查代价远高于共用一份函数。故提取为单一来源。
//!
//! ## 与仓库其它格哈希的关系（重要）
//!
//! 本模块只提供**这一种**算法（murmur3 收尾的 32 位映射）。仓库里另有若干
//! 结构相似但**算法不同**的格哈希——例如 `crate::climate` 的 splitmix 变体
//! （f64、`+` 组合）与 `wv-world-cell-map::terrain_view::field::symbol_atlas`
//! 的 salt-乘法变体。它们数值**不同**，**不可**与本函数互换（互换会平移全部
//! 派生取值），因此未合并，仅按各自语义保留。

/// 整数格点确定性 hash → `[0,1)`。
///
/// 无外部依赖、跨平台一致（测试与运行共用）：同一 `(x, y, seed)` 两次运行
/// 结果逐位相同。末位取低 32 位映射到 `[0,1)`（`/(u32::MAX)`）——这是历史
/// 实现的原样保留，**改变映射会平移所有派生量的取值**，故此处只提取不改写。
pub fn cell_hash01(x: i64, y: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h & 0xFFFF_FFFF) as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同一输入两次调用逐位一致（确定性契约，串起上面所有使用方）。
    #[test]
    fn same_input_is_bit_identical() {
        let a = cell_hash01(37, -9, 0x5EED_C0DE);
        let b = cell_hash01(37, -9, 0x5EED_C0DE);
        assert_eq!(a.to_bits(), b.to_bits(), "确定性哈希必须逐位一致");
    }

    /// 输出恒在 `[0,1)`——下游用它做 `[0,1]` 抖动 / 插值，越界会外溢。
    #[test]
    fn output_stays_within_unit_interval() {
        for x in -17..17 {
            for y in -17..17 {
                for seed in [0u64, 1, 0x9E37_79B9, u64::MAX] {
                    let v = cell_hash01(x, y, seed);
                    assert!((0.0..1.0).contains(&v), "越界: ({x},{y},{seed}) → {v}");
                }
            }
        }
    }

    /// 相邻格点不相关（雪崩）：水平 / 垂直邻居不得给出同值。
    #[test]
    fn neighbor_cells_do_not_collide_trivially() {
        assert_ne!(
            cell_hash01(10, 10, 7).to_bits(),
            cell_hash01(11, 10, 7).to_bits()
        );
        assert_ne!(
            cell_hash01(10, 10, 7).to_bits(),
            cell_hash01(10, 11, 7).to_bits()
        );
    }
}
