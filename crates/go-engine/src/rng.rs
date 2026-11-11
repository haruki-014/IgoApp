//! 小さな擬似乱数生成器(xoshiro256**)。外部クレートに依存しないために自前で持つ。

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Rng {
    state: [u64; 4],
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    /// シードを指定して作る。同じシードなら同じ乱数列になる。
    pub fn from_seed(seed: u64) -> Rng {
        let mut s = seed;
        Rng {
            state: [
                splitmix64(&mut s),
                splitmix64(&mut s),
                splitmix64(&mut s),
                splitmix64(&mut s),
            ],
        }
    }

    /// 時刻とプロセス ID からシードを作る。
    pub fn from_entropy() -> Rng {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Rng::from_seed(nanos ^ ((std::process::id() as u64) << 32))
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// 0 以上 `n` 未満の一様乱数。`n` は 1 以上。
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        // 上位ビットを使う乗算法。偏りは n / 2^64 程度で、この用途では無視できる。
        ((self.next_u64() as u128 * n as u128) >> 64) as usize
    }

    /// 0 以上 1 未満の一様乱数。
    #[inline]
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::from_seed(42);
        let mut b = Rng::from_seed(42);
        let mut c = Rng::from_seed(43);
        let xs: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let zs: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut rng = Rng::from_seed(7);
        let mut seen = [false; 10];
        for _ in 0..1000 {
            let v = rng.below(10);
            assert!(v < 10);
            seen[v] = true;
        }
        assert!(seen.iter().all(|&s| s));
        assert_eq!(rng.below(1), 0);
        for _ in 0..100 {
            let u = rng.unit();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
