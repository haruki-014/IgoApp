use std::fmt;

/// 石の色。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    Black,
    White,
}

impl Color {
    pub const BOTH: [Color; 2] = [Color::Black, Color::White];

    #[inline]
    pub fn opponent(self) -> Color {
        match self {
            Color::Black => Color::White,
            Color::White => Color::Black,
        }
    }

    /// 配列の添字として使う値(黒 = 0、白 = 1)。
    #[inline]
    pub fn index(self) -> usize {
        match self {
            Color::Black => 0,
            Color::White => 1,
        }
    }

    /// `'B'` または `'W'`。
    pub fn to_char(self) -> char {
        match self {
            Color::Black => 'B',
            Color::White => 'W',
        }
    }

    /// GTP などの色表記を読む。`b` / `black` / `w` / `white`(大文字小文字は問わない)。
    pub fn parse(s: &str) -> Option<Color> {
        match s.trim().to_ascii_lowercase().as_str() {
            "b" | "black" => Some(Color::Black),
            "w" | "white" => Some(Color::White),
            _ => None,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

/// 盤面配列の 1 点の状態。`Wall` は盤の外周に置く番兵。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Cell {
    Empty = 0,
    Black = 1,
    White = 2,
    Wall = 3,
}

impl Cell {
    #[inline]
    pub fn color(self) -> Option<Color> {
        match self {
            Cell::Black => Some(Color::Black),
            Cell::White => Some(Color::White),
            _ => None,
        }
    }
}

impl From<Color> for Cell {
    #[inline]
    fn from(c: Color) -> Cell {
        match c {
            Color::Black => Cell::Black,
            Color::White => Cell::White,
        }
    }
}
