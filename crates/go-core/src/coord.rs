//! 座標変換。扱う座標系は次の 4 つで、変換はすべてこのモジュールに集約する。
//!
//! - 内部インデックス:番兵付き 1 次元配列の添字。幅は「盤サイズ + 2」。
//! - (x, y):左上が原点で 0 始まり。x は右方向、y は下方向。
//! - GTP 座標:列は `I` を飛ばした `A`〜`Z`、行は下から数えて 1 始まり(例:`D4`)。
//! - SGF 座標:左上が原点の英小文字 2 文字(例:`pd`)。

use std::fmt;

/// 対応する最小の盤サイズ。
pub const MIN_SIZE: usize = 2;
/// 対応する最大の盤サイズ。GTP の列文字(`I` を除く `A`〜`Z`)が 25 個であることによる。
pub const MAX_SIZE: usize = 25;

/// GTP の列文字。`I` は使わない。
pub const GTP_COLUMNS: &[u8; 25] = b"ABCDEFGHJKLMNOPQRSTUVWXYZ";

/// 盤サイズが対応範囲(2〜25)かどうか。
pub fn is_valid_size(size: usize) -> bool {
    (MIN_SIZE..=MAX_SIZE).contains(&size)
}

/// 盤上の点。左上が原点で 0 始まり。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Coord {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoordError {
    /// 書式が正しくない。
    Syntax,
    /// 書式は正しいが盤の外。
    OutOfBounds,
}

impl fmt::Display for CoordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoordError::Syntax => write!(f, "invalid coordinate syntax"),
            CoordError::OutOfBounds => write!(f, "coordinate out of bounds"),
        }
    }
}

impl std::error::Error for CoordError {}

impl Coord {
    #[inline]
    pub fn new(x: usize, y: usize) -> Coord {
        Coord { x, y }
    }

    #[inline]
    pub fn in_bounds(self, size: usize) -> bool {
        self.x < size && self.y < size
    }

    /// (x, y) → 内部インデックス。
    #[inline]
    pub fn to_index(self, size: usize) -> usize {
        debug_assert!(self.in_bounds(size));
        (self.y + 1) * (size + 2) + (self.x + 1)
    }

    /// 内部インデックス → (x, y)。番兵や範囲外の添字なら `None`。
    pub fn from_index(index: usize, size: usize) -> Option<Coord> {
        let width = size + 2;
        if index >= width * width {
            return None;
        }
        let (col, row) = (index % width, index / width);
        if col == 0 || row == 0 || col > size || row > size {
            return None;
        }
        Some(Coord::new(col - 1, row - 1))
    }

    /// (x, y) → GTP 座標(例:19 路の左上は `A19`)。
    pub fn to_gtp(self, size: usize) -> String {
        debug_assert!(self.in_bounds(size) && is_valid_size(size));
        format!("{}{}", GTP_COLUMNS[self.x] as char, size - self.y)
    }

    /// GTP 座標 → (x, y)。大文字小文字は問わない。
    pub fn from_gtp(s: &str, size: usize) -> Result<Coord, CoordError> {
        let s = s.trim();
        let mut chars = s.chars();
        let col = chars.next().ok_or(CoordError::Syntax)?.to_ascii_uppercase();
        if !col.is_ascii_uppercase() || col == 'I' {
            return Err(CoordError::Syntax);
        }
        let digits = chars.as_str();
        if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CoordError::Syntax);
        }
        let row: usize = digits.parse().map_err(|_| CoordError::Syntax)?;
        let x = GTP_COLUMNS
            .iter()
            .position(|&c| c as char == col)
            .ok_or(CoordError::Syntax)?;
        if x >= size || row == 0 || row > size {
            return Err(CoordError::OutOfBounds);
        }
        Ok(Coord::new(x, size - row))
    }

    /// (x, y) → SGF 座標(例:右上の星は `pd`)。
    pub fn to_sgf(self) -> String {
        debug_assert!(self.x < 26 && self.y < 26);
        let mut s = String::with_capacity(2);
        s.push((b'a' + self.x as u8) as char);
        s.push((b'a' + self.y as u8) as char);
        s
    }

    /// SGF 座標 → (x, y)。
    pub fn from_sgf(s: &str, size: usize) -> Result<Coord, CoordError> {
        let b = s.as_bytes();
        if b.len() != 2 || !b[0].is_ascii_lowercase() || !b[1].is_ascii_lowercase() {
            return Err(CoordError::Syntax);
        }
        let c = Coord::new((b[0] - b'a') as usize, (b[1] - b'a') as usize);
        if c.in_bounds(size) {
            Ok(c)
        } else {
            Err(CoordError::OutOfBounds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_layout_matches_sentinel_array() {
        // 9 路なら幅 11。左上 (0,0) は 1 行目 1 列目 = 12。
        assert_eq!(Coord::new(0, 0).to_index(9), 12);
        assert_eq!(Coord::new(8, 0).to_index(9), 20);
        assert_eq!(Coord::new(0, 1).to_index(9), 23);
        assert_eq!(Coord::new(8, 8).to_index(9), 9 * 11 + 9);
    }

    #[test]
    fn index_rejects_sentinels() {
        let size = 9;
        let width = size + 2;
        for i in 0..width {
            assert_eq!(Coord::from_index(i, size), None, "top wall {i}");
            assert_eq!(Coord::from_index(i * width, size), None, "left wall {i}");
            assert_eq!(Coord::from_index(i * width + width - 1, size), None, "right wall {i}");
            assert_eq!(Coord::from_index((width - 1) * width + i, size), None, "bottom wall {i}");
        }
        assert_eq!(Coord::from_index(width * width, size), None);
        assert_eq!(Coord::from_index(usize::MAX, size), None);
    }

    #[test]
    fn gtp_known_values() {
        assert_eq!(Coord::new(0, 0).to_gtp(19), "A19");
        assert_eq!(Coord::new(18, 18).to_gtp(19), "T1");
        assert_eq!(Coord::new(0, 18).to_gtp(19), "A1");
        // I は飛ばすので、9 列目(x = 8)は J。
        assert_eq!(Coord::new(7, 0).to_gtp(19), "H19");
        assert_eq!(Coord::new(8, 0).to_gtp(19), "J19");
        assert_eq!(Coord::from_gtp("D4", 19), Ok(Coord::new(3, 15)));
        assert_eq!(Coord::from_gtp("q16", 19), Ok(Coord::new(15, 3)));
        assert_eq!(Coord::from_gtp("J1", 9), Ok(Coord::new(8, 8)));
        assert_eq!(Coord::new(24, 0).to_gtp(25), "Z25");
        assert_eq!(Coord::from_gtp("Z25", 25), Ok(Coord::new(24, 0)));
        assert_eq!(Coord::from_gtp("A1", 2), Ok(Coord::new(0, 1)));
        assert_eq!(Coord::from_gtp("B2", 2), Ok(Coord::new(1, 0)));
    }

    #[test]
    fn gtp_rejects_invalid() {
        assert_eq!(Coord::from_gtp("I5", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("i5", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("A", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("5A", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("A1x", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("A+1", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("A100", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("あ1", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_gtp("A0", 19), Err(CoordError::OutOfBounds));
        assert_eq!(Coord::from_gtp("A20", 19), Err(CoordError::OutOfBounds));
        assert_eq!(Coord::from_gtp("U1", 19), Err(CoordError::OutOfBounds));
        assert_eq!(Coord::from_gtp("K1", 9), Err(CoordError::OutOfBounds));
    }

    #[test]
    fn sgf_known_values() {
        assert_eq!(Coord::new(15, 3).to_sgf(), "pd");
        assert_eq!(Coord::from_sgf("pd", 19), Ok(Coord::new(15, 3)));
        assert_eq!(Coord::from_sgf("aa", 19), Ok(Coord::new(0, 0)));
        assert_eq!(Coord::from_sgf("ss", 19), Ok(Coord::new(18, 18)));
        assert_eq!(Coord::new(24, 24).to_sgf(), "yy");
        // GTP の Q16 と SGF の pd は同じ点。
        assert_eq!(Coord::from_gtp("Q16", 19), Coord::from_sgf("pd", 19));
        assert_eq!(Coord::from_sgf("tt", 19), Err(CoordError::OutOfBounds));
        assert_eq!(Coord::from_sgf("a", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_sgf("AA", 19), Err(CoordError::Syntax));
        assert_eq!(Coord::from_sgf("abc", 19), Err(CoordError::Syntax));
    }

    /// 4 つの座標系のあいだを往復しても値が変わらない(全サイズ・全点)。
    #[test]
    fn roundtrip_all_sizes_and_points() {
        for size in MIN_SIZE..=MAX_SIZE {
            let mut seen_index = std::collections::HashSet::new();
            let mut seen_gtp = std::collections::HashSet::new();
            let mut seen_sgf = std::collections::HashSet::new();
            for y in 0..size {
                for x in 0..size {
                    let c = Coord::new(x, y);

                    let index = c.to_index(size);
                    assert_eq!(Coord::from_index(index, size), Some(c));
                    assert!(seen_index.insert(index));

                    let gtp = c.to_gtp(size);
                    assert_eq!(Coord::from_gtp(&gtp, size), Ok(c), "size {size} {gtp}");
                    assert_eq!(Coord::from_gtp(&gtp.to_lowercase(), size), Ok(c));
                    assert!(!gtp.contains('I'));
                    assert!(seen_gtp.insert(gtp));

                    let sgf = c.to_sgf();
                    assert_eq!(Coord::from_sgf(&sgf, size), Ok(c), "size {size} {sgf}");
                    assert!(seen_sgf.insert(sgf));

                    // 経路を変えた往復:GTP → 内部インデックス → SGF → (x, y)
                    let via = Coord::from_gtp(&c.to_gtp(size), size).unwrap().to_index(size);
                    let via = Coord::from_index(via, size).unwrap().to_sgf();
                    assert_eq!(Coord::from_sgf(&via, size), Ok(c));
                }
            }
            assert_eq!(seen_index.len(), size * size);
        }
    }

    /// 逆方向:内部インデックスを全走査し、盤上の点だけが (x, y) に写ることを確認する。
    #[test]
    fn roundtrip_from_every_index() {
        for size in MIN_SIZE..=MAX_SIZE {
            let width = size + 2;
            let mut on_board = 0;
            for index in 0..width * width {
                if let Some(c) = Coord::from_index(index, size) {
                    assert!(c.in_bounds(size));
                    assert_eq!(c.to_index(size), index);
                    on_board += 1;
                }
            }
            assert_eq!(on_board, size * size);
        }
    }

    #[test]
    fn size_validation() {
        assert!(!is_valid_size(0));
        assert!(!is_valid_size(1));
        assert!(is_valid_size(2));
        assert!(is_valid_size(25));
        assert!(!is_valid_size(26));
    }
}
