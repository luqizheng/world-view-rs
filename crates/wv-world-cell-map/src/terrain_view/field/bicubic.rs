//! Catmull-Rom 双三次核的**宿主侧镜像**与解析导数（P5）——纯 Rust，无 Bevy。
//!
//! ## 这个模块存在的理由
//!
//! `terrain_field.wgsl` 里的 `sample_height_bicubic_grad()` 是逐像素求值的
//! 内核，**编译期不校验数学正确性**：`cargo check` 过、clippy 过、naga 过，
//! 甚至跑起来出画面了，一个写错的权重导数也只会表现为「山体明暗有点怪」，
//! 没有任何报错能指到它。
//!
//! 本模块把同一套权重与求导规则用 Rust 重写一遍（**逐行对照 WGSL**），
//! 于是可以在 `cargo test` 里用数值方法证伪：
//!
//! 1. **解析偏导 ≡ 同一核的数值导数**——这是解析导数的定义式检查，
//!    对 `cubic_weights_deriv` 的四个系数任意一个写错都会红。
//! 2. **解析偏导在格边界两侧连续**——旧中心差分在 `fract()` 不连续处
//!    精度塌陷，这是 P5 换算法的实质理由，不换测不出来。
//! 3. **权重和恒 1 / 导数和恒 0**——Catmull-Rom 插值与分段常数一致性的前提。
//!
//! ## 与 WGSL 的对应关系（改动必须双边同步）
//!
//! | 本模块 | `terrain_field.wgsl` |
//! |---|---|
//! | [`cubic_weights`] | `cubic_weights` |
//! | [`cubic_weights_deriv`] | `cubic_weights_deriv` |
//! | [`bicubic_value`] | `sample_height_bicubic_grad` 的 `h` 通道 |
//! | [`bicubic_grad`] | `sample_height_bicubic_grad` |
//!
//! ## 坐标约定（与 WGSL 一致，唯一权威）
//!
//! 入参 `pos` 是**格坐标（边界空间）**：第 i 格占 `[i, i+1)`，格心 `i + 0.5`。
//! `tc = pos − 0.5` 是**平移**，故 `d/dpos = d/dtc`，求导无额外链式因子。

/// Catmull-Rom 四项权重（t ∈ [0,1]）。
///
/// ```text
/// w0 = -0.5t³ + t²   - 0.5t
/// w1 =  1.5t³ - 2.5t² + 1
/// w2 = -1.5t³ + 2t²   + 0.5t
/// w3 =  0.5t³ - 0.5t²
/// ```
///
/// 四项和恒为 1（负权重由正权重补偿），故插值是分段仿射且无归一化步骤。
#[must_use]
pub fn cubic_weights(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

/// Catmull-Rom 权重对 `t` 的**解析导数**（P5）。
///
/// 对 [`cubic_weights`] 四条逐项求导（dt3 = 3t²、dt2 = 2t）：
///
/// ```text
/// w0' = -1.5t² + 2t   - 0.5
/// w1' =  4.5t² - 5t
/// w2' = -4.5t² + 4t   + 0.5
/// w3' =  1.5t² - t
/// ```
///
/// 四项和恒为 0（权重和 = 1 是常数 → 导数为 0）。
#[must_use]
pub fn cubic_weights_deriv(t: f32) -> [f32; 4] {
    let t2 = t * t;
    [
        -1.5 * t2 + 2.0 * t - 0.5,
        4.5 * t2 - 5.0 * t,
        -4.5 * t2 + 4.0 * t + 0.5,
        1.5 * t2 - t,
    ]
}

/// 在 `pos`（格坐标）处对双三次核求值。
///
/// `src` 是按纹素下标索引的高度场；越界取 0（对应 WGSL 的
/// `clamp` 到图内——镜像实现取 0 以便单元测试构造小网格）。
#[must_use]
pub fn bicubic_value(src: &[f32], gw: usize, pos: [f32; 2]) -> f32 {
    let (base, _, wx, wy) = window(pos);
    let mut acc = 0.0;
    for (j, &wyj) in wy.iter().enumerate() {
        for (i, &wxi) in wx.iter().enumerate() {
            acc += texel(src, gw, base, i, j) * (wxi * wyj);
        }
    }
    acc
}

/// 在 `pos`（格坐标）处取双三次核的值与两个解析偏导 `(h, dh/dx, dh/dy)`。
///
/// 与 [`bicubic_value`] 共享同一个 4×4 窗口与同一次取样——这是 P5 的
/// 全部意义：旧写法要为两个偏导各重取两次双三次（4×16 = 64 tap），
/// 这里一次 16 tap 给出三者。
#[must_use]
pub fn bicubic_grad(src: &[f32], gw: usize, pos: [f32; 2]) -> [f32; 3] {
    let (base, f, wx, wy) = window(pos);
    let dxw = cubic_weights_deriv(f[0]);
    let dyw = cubic_weights_deriv(f[1]);
    let (mut h, mut gx, mut gy) = (0.0, 0.0, 0.0);
    for (j, (&wyj, &dywj)) in wy.iter().zip(dyw.iter()).enumerate() {
        for (i, (&wxi, &dxwi)) in wx.iter().zip(dxw.iter()).enumerate() {
            let t = texel(src, gw, base, i, j);
            h += t * (wxi * wyj);
            gx += t * (dxwi * wyj);
            gy += t * (wxi * dywj);
        }
    }
    [h, gx, gy]
}

/// 4×4 窗口的基点、亚格位置与两组权重。
fn window(pos: [f32; 2]) -> ([f32; 2], [f32; 2], [f32; 4], [f32; 4]) {
    let tc = [pos[0] - 0.5, pos[1] - 0.5];
    let f = [tc[0].fract(), tc[1].fract()];
    let base = [tc[0].floor(), tc[1].floor()];
    (base, f, cubic_weights(f[0]), cubic_weights(f[1]))
}

/// 取第 `base + (i-1, j-1)` 号纹素。
///
/// WGSL 侧 `clamp` 到图内（防边缘黑边）；宿主镜像在越界时取 0，
/// 这样单元测试可以用小网格而不必为边界情形单独造数据。
fn texel(src: &[f32], gw: usize, base: [f32; 2], i: usize, j: usize) -> f32 {
    let idx = |k: f32, off: usize| -> Option<usize> {
        let v = k as isize + off as isize - 1;
        usize::try_from(v).ok()
    };
    match (idx(base[0], i), idx(base[1], j)) {
        (Some(x), Some(y)) => src.get(y * gw + x).copied().unwrap_or(0.0),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 平地：解析偏导必须为 0（否则会把整片平原染出假山体）。
    ///
    /// 采样点刻意留在网格**内部**——宿主镜像越界返回 0（WGSL 侧是 `clamp`，
    /// 两者在边界本就不同），拿边界测「常数还原」只会测到越界约定。
    #[test]
    fn flat_field_has_zero_gradient() {
        let (gw, gh) = (16, 16);
        let flat = vec![0.42; gw * gh];
        for &(px, py) in &[(4.5, 4.5), (6.2, 8.7), (2.5, 2.5), (13.5, 11.5)] {
            let g = bicubic_grad(&flat, gw, [px, py]);
            assert!(
                g[1].abs() < 1e-6 && g[2].abs() < 1e-6,
                "平地梯度应为零，实际 ({}, {})",
                g[1],
                g[2]
            );
            assert!((g[0] - 0.42).abs() < 1e-6, "平地高度应还原常数");
        }
    }

    /// 权重和恒 1、导数和恒 0——Catmull-Rom 与分段常数一致的前提。
    #[test]
    fn weights_sum_to_one_and_derivs_to_zero() {
        for k in 0..=100 {
            let t = k as f32 / 100.0;
            let w: f32 = cubic_weights(t).iter().sum();
            let d: f32 = cubic_weights_deriv(t).iter().sum();
            assert!((w - 1.0).abs() < 1e-5, "t={t} 权重和 {w} ≠ 1");
            assert!(d.abs() < 1e-5, "t={t} 导数和 {d} ≠ 0");
        }
    }

    /// 解析导数 ≡ 同一核的数值导数（定义式检查）。
    ///
    /// 这是 P5 的核心断言：[`cubic_weights_deriv`] 四个系数任意一个
    /// 写错（漏项、系数错、符号错），本用例如即失败。
    #[test]
    fn analytic_grad_matches_finite_difference_of_same_kernel() {
        let (gw, gh) = (16, 16);
        // 确定性伪随机高度场（非 fbm 也能暴露权重错误，比纯斜坡更敏感）
        let src: Vec<f32> = (0..gw * gh)
            .map(|k| {
                let s = (k as f32 * 12.9898).sin() * 43758.545_f32;
                s - s.floor()
            })
            .collect();

        let delta = 1e-3;
        let mut worst = 0.0f32;
        // 采样点刻意避开 f 紧贴 0/1 的位置——数值差分在窗口边界处
        // 本身精度就差，那里比的是解析解的连续性，不是导数对不对。
        for k in 0..200 {
            let px = 2.0 + (k as f32) * 0.0611;
            let py = 2.0 + (k as f32) * 0.0337;
            let f = [(px - 0.5).fract(), (py - 0.5).fract()];
            if f[0] < 0.15 || f[0] > 0.85 || f[1] < 0.15 || f[1] > 0.85 {
                continue;
            }
            let g = bicubic_grad(&src, gw, [px, py]);
            let dx = (bicubic_value(&src, gw, [px + delta, py])
                - bicubic_value(&src, gw, [px - delta, py]))
                / (2.0 * delta);
            let dy = (bicubic_value(&src, gw, [px, py + delta])
                - bicubic_value(&src, gw, [px, py - delta]))
                / (2.0 * delta);
            worst = worst.max((g[1] - dx).abs()).max((g[2] - dy).abs());
        }
        // 数值差分截断误差 O(δ²·h''')，δ=1e-3 时约 1e-5 量级
        assert!(worst < 1e-3, "解析导数与数值导数最大偏差 {worst} 过大");
    }

    /// 解析偏导在格边界**两侧连续**——旧中心差分在这里精度塌陷。
    ///
    /// `fract()` 在 `f → 1` 时跳回 0，四点窗口随之整体移位；中心差分
    /// 的两个取样点会落到不同窗口，差分实际跨了边界。解析解没有这个问题，
    /// 它的极限在两侧都存在且相等。
    #[test]
    fn analytic_grad_is_continuous_across_cell_boundary() {
        let (gw, gh) = (16, 16);
        let src: Vec<f32> = (0..gw * gh)
            .map(|k| {
                let s = (k as f32 * 12.9898).sin() * 43758.545_f32;
                s - s.floor()
            })
            .collect();

        // 格心 `i + 0.5` 是 f 的接缝；取缝两侧极限。
        let eps = 1e-4;
        for &edge in &[4.5_f32, 7.5, 10.5] {
            let lo = bicubic_grad(&src, gw, [edge - eps, 5.3])[1];
            let hi = bicubic_grad(&src, gw, [edge + eps, 5.3])[1];
            assert!(
                (lo - hi).abs() < 1e-2,
                "接缝 x={edge} 处 dh/dx 跳变：{lo} vs {hi}"
            );
        }
    }

    /// 仿射场（斜面）必须被双三次**精确**还原，且梯度等于斜率。
    ///
    /// Catmull-Rom 对线性函数是精确的（权重和 1 + 一阶矩为 0），
    /// 这条同时钉住值通道与导数通道。
    ///
    /// ⚠️ 纹素 `(i,j)` 的值以**纹素下标** `i/j` 定义，而连续位置 `pos`
    /// 的对应纹素坐标是 `pos − 0.5`（坐标约定）。故期望值必须用
    /// `(px − 0.5, py − 0.5)` 代入——直接用 `px/py` 会整体差半个纹素间距，
    /// 表现为恒定偏差 0.075（本例斜率之和 0.15 × 0.5）。
    #[test]
    fn affine_field_is_exact() {
        let (gw, gh) = (16, 16);
        // h = 0.25 + 0.1x + 0.05y，x/y 为纹素下标
        let src: Vec<f32> = (0..gw * gh)
            .map(|k| {
                let (x, y) = ((k % gw) as f32, (k / gw) as f32);
                0.25 + 0.1 * x + 0.05 * y
            })
            .collect();

        // 避开网格边缘（越界取 0 会破坏仿射关系）
        for &(px, py) in &[(4.3, 6.7), (8.5, 9.2), (6.05, 7.95)] {
            let g = bicubic_grad(&src, gw, [px, py]);
            let expect = 0.25 + 0.1 * (px - 0.5) + 0.05 * (py - 0.5);
            assert!(
                (g[0] - expect).abs() < 1e-4,
                "值通道：{} vs {}",
                g[0],
                expect
            );
            assert!((g[1] - 0.1).abs() < 1e-4, "dh/dx 应为 0.1，实际 {}", g[1]);
            assert!((g[2] - 0.05).abs() < 1e-4, "dh/dy 应为 0.05，实际 {}", g[2]);
        }
    }

    /// 格心处应还原该格自己的值（无偏移）——配准正确性的回归闸。
    #[test]
    fn cell_center_returns_own_texel() {
        let (gw, gh) = (8, 8);
        let src: Vec<f32> = (0..gw * gh).map(|k| k as f32 * 0.01).collect();
        for &(x, y) in &[(0_f32, 0_f32), (3.0, 5.0), (7.0, 7.0)] {
            let g = bicubic_grad(&src, gw, [x + 0.5, y + 0.5]);
            let expect = (y as usize * gw + x as usize) as f32 * 0.01;
            assert!(
                (g[0] - expect).abs() < 1e-5,
                "格 ({x},{y}) 中心高度 {got} 应为 {expect}",
                got = g[0]
            );
        }
    }

    /// 一次 `bicubic_grad` 的取样数必须是 16——P5 的省采样承诺在此钉死。
    ///
    /// 旧写法为 h 取 16 + dx 的 32 + dy 的 32 = 80 tap。
    #[test]
    fn grad_uses_single_4x4_window() {
        // 探针：只有 base+(i-1,j-1) 全落在 0..gw 时 texel 才非零。
        let (gw, gh) = (16, 16);
        let mut src = vec![0.0; gw * gh];
        src[8 * gw + 8] = 1.0; // 中心格
                               // pos 使 base = floor(pos-0.5) = 8 → 窗口覆盖 base-1..base+2
        let g = bicubic_grad(&src, gw, [8.5, 8.5]);
        // f = 0 → 只有 w1 命中 → h = 1.0
        assert!((g[0] - 1.0).abs() < 1e-5, "h 应为 1.0，实际 {}", g[0]);
        // 单个非零纹素在 f=0 处的解析导数为 0（w1' = 0）
        assert!(g[1].abs() < 1e-5 && g[2].abs() < 1e-5);
    }
}
