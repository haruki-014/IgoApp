//! Zobrist ハッシュ用の乱数表。
//!
//! 「点 × 色」ごとに 64bit の乱数を割り当てる。再現性のため、固定シードの
//! SplitMix64 でコンパイル時に生成する。表は (x, y) で引くので、同じ点・同じ色の
//! 乱数は盤サイズによらず同じ値になる。空の盤のハッシュは 0。

use crate::color::Color;
use crate::coord::MAX_SIZE;

/// 乱数表の生成に使う固定シード。
pub const SEED: u64 = 0x1960_0519_C0FF_EE42;

const POINTS: usize = MAX_SIZE * MAX_SIZE;

static TABLE: [[u64; 2]; POINTS] = build_table();

const fn splitmix64(state: u64) -> (u64, u64) {
    let state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (state, z ^ (z >> 31))
}

const fn build_table() -> [[u64; 2]; POINTS] {
    let mut table = [[0u64; 2]; POINTS];
    let mut state = SEED;
    let mut i = 0;
    while i < POINTS {
        let mut c = 0;
        while c < 2 {
            let (next, value) = splitmix64(state);
            state = next;
            table[i][c] = value;
            c += 1;
        }
        i += 1;
    }
    table
}

/// 点 (x, y) に `color` の石があることを表す乱数。
#[inline]
pub fn key(x: usize, y: usize, color: Color) -> u64 {
    TABLE[y * MAX_SIZE + x][color.index()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_are_distinct_and_nonzero() {
        let mut seen = HashSet::new();
        for y in 0..MAX_SIZE {
            for x in 0..MAX_SIZE {
                for color in Color::BOTH {
                    let k = key(x, y, color);
                    assert_ne!(k, 0);
                    assert!(seen.insert(k), "duplicate key at ({x},{y}) {color}");
                }
            }
        }
        assert_eq!(seen.len(), MAX_SIZE * MAX_SIZE * 2);
    }

    /// 固定シードなので、値はビルドや実行のたびに変わらない。
    #[test]
    fn table_is_reproducible() {
        let (state, first) = splitmix64(SEED);
        let (_, second) = splitmix64(state);
        assert_eq!(key(0, 0, Color::Black), first);
        assert_eq!(key(0, 0, Color::White), second);
        assert_eq!(build_table(), TABLE);
    }
}
