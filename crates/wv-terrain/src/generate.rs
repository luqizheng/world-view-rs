//! 噪声生成：手写 Perlin（基线 §2.18：手写 ~80 行，不用 `noise` crate）。
//!
//! ## 为什么手写
//!
//! 依赖是不可逆的成本（`AGENTS.md` §9）。Perlin 噪声是**教科书算法**，
//! 实现成本低（< 100 行）、可被单测锁定（基线 §2.20 风险表：手写 Perlin 出 bug → 单测锁定）。
//! 引入 `noise` crate 会为一次成型的功能付出长期维护与供应链成本。
//!
//! ## 实现口径
//!
//! 经典 Ken Perlin 的 2D 梯度噪声（2002 改进版）：
//! - 置换表由固定种子 Fisher-Yates 洗牌生成（**确定性**，同种子同图）；
//! - 梯度取 8 个方向的单位向量；
//! - 用 `fade(t) = 6t^5 - 15t^4 + 10t^3` 做平滑插值；
//! - 结果归一到 `[-1, 1]`。
//!
//! 多倍频（fractal / octave）叠加由 [`Perlin::fbm`] 提供。

/// 2D Perlin 噪声发生器。
#[derive(Debug, Clone)]
pub struct Perlin {
    /// 置换表（512 项，前半与后半互为镜像以省去取模）。
    perm: [u8; 512],
}

impl Perlin {
    /// 用给定种子构造。同种子 → 完全相同的噪声场（可复现）。
    pub fn new(seed: u64) -> Self {
        // 0..256 的顺序表。
        let mut p: [u8; 256] = [0; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        // xorshift64* 作为确定性 PRNG——避免引入 rand crate。
        let mut state = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        let mut next = || {
            // xorshift64*
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        // Fisher-Yates 洗牌。
        for i in (1..256).rev() {
            let j = (next() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        // 镜像成 512。
        let mut perm = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Self { perm }
    }

    /// 在 `(x, y)` 处采样，返回 `[-1, 1]`。
    pub fn noise2d(&self, x: f64, y: f64) -> f64 {
        let xi = x.floor() as i32 & 255;
        let yi = y.floor() as i32 & 255;
        let xf = x - x.floor();
        let yf = y - y.floor();

        let u = fade(xf);
        let v = fade(yf);

        let aa = self.perm[(self.perm[xi as usize] as usize + yi as usize) & 255];
        let ab = self.perm[(self.perm[xi as usize] as usize + yi as usize + 1) & 255];
        let ba = self.perm[(self.perm[(xi + 1) as usize] as usize + yi as usize) & 255];
        let bb = self.perm[(self.perm[(xi + 1) as usize] as usize + yi as usize + 1) & 255];

        let x1 = lerp(grad(aa, xf, yf), grad(ba, xf - 1.0, yf), u);
        let x2 = lerp(grad(ab, xf, yf - 1.0), grad(bb, xf - 1.0, yf - 1.0), u);
        lerp(x1, x2, v)
    }

    /// 多倍频叠加（fractal Brownian motion）。
    ///
    /// 返回 `[-1, 1]`（各倍频振幅之和归一）。
    ///
    /// - `octaves`：叠加层数；
    /// - `lacunarity`：每层频率倍数（常用 2.0）；
    /// - `persistence`：每层振幅衰减（常用 0.5）。
    pub fn fbm(&self, x: f64, y: f64, octaves: u32, lacunarity: f64, persistence: f64) -> f64 {
        let mut total = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut max_amplitude = 0.0;
        for _ in 0..octaves.max(1) {
            total += self.noise2d(x * frequency, y * frequency) * amplitude;
            max_amplitude += amplitude;
            frequency *= lacunarity;
            amplitude *= persistence;
        }
        if max_amplitude > 0.0 {
            total / max_amplitude
        } else {
            0.0
        }
    }
}

/// Perlin 的平滑曲线 `6t^5 - 15t^4 + 10t^3`。
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

/// 8 方向的单位梯度向量与格点偏移的点积。
fn grad(hash: u8, x: f64, y: f64) -> f64 {
    // 取 hash 的低 3 位选 8 个方向之一。
    match hash & 7 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_produces_identical_field() {
        let a = Perlin::new(42);
        let b = Perlin::new(42);
        for i in 0..50 {
            let x = i as f64 * 0.37;
            let y = i as f64 * 0.11;
            assert_eq!(a.noise2d(x, y), b.noise2d(x, y), "同种子须完全一致");
        }
    }

    #[test]
    fn different_seed_produces_different_field() {
        let a = Perlin::new(1);
        let b = Perlin::new(2);
        let differs = (0..50).any(|i| {
            let x = i as f64 * 0.37;
            a.noise2d(x, x) != b.noise2d(x, x)
        });
        assert!(differs, "不同种子应产生不同噪声场");
    }

    #[test]
    fn noise_stays_within_unit_range() {
        let p = Perlin::new(7);
        for i in 0..2000 {
            let x = (i as f64) * 0.137;
            let y = (i as f64) * 0.319;
            let v = p.noise2d(x, y);
            assert!((-1.0..=1.0).contains(&v), "噪声越界: {v}");
        }
    }

    #[test]
    fn noise_is_zero_at_integer_lattice_points() {
        // Perlin 的经典性质：格点处梯度噪声值为 0。
        let p = Perlin::new(11);
        for ix in 0..10 {
            for iy in 0..10 {
                let v = p.noise2d(ix as f64, iy as f64);
                assert!(v.abs() < 1e-9, "格点 ({ix},{iy}) 应为 0，实际 {v}");
            }
        }
    }

    #[test]
    fn fbm_stays_within_unit_range() {
        let p = Perlin::new(3);
        for i in 0..1000 {
            let x = i as f64 * 0.05;
            let y = i as f64 * 0.07;
            let v = p.fbm(x, y, 5, 2.0, 0.5);
            assert!((-1.0..=1.0).contains(&v), "fbm 越界: {v}");
        }
    }
}
