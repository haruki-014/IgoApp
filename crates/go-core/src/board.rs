//! 盤面。番兵付きの 1 次元配列で表す。
//!
//! 配列の幅は「盤サイズ + 2」で、外周の 1 マスは [`Cell::Wall`]。
//! ある点の隣接点は「±1」と「±幅」で求まり、盤端の判定が要らない。

use std::fmt;

use crate::color::{Cell, Color};
use crate::coord::{Coord, GTP_COLUMNS, MAX_SIZE, is_valid_size};
use crate::zobrist;

const MAX_CELLS: usize = (MAX_SIZE + 2) * (MAX_SIZE + 2);
const MARK_WORDS: usize = MAX_CELLS.div_ceil(64);

/// flood fill 用の訪問済みビット集合(ヒープを使わない)。
struct Marks([u64; MARK_WORDS]);

impl Marks {
    #[inline]
    fn new() -> Marks {
        Marks([0; MARK_WORDS])
    }

    /// 印を付ける。初めて付けたときだけ `true` を返す。
    #[inline]
    fn insert(&mut self, index: usize) -> bool {
        let (word, bit) = (index / 64, 1u64 << (index % 64));
        let fresh = self.0[word] & bit == 0;
        self.0[word] |= bit;
        fresh
    }
}

/// 盤サイズが対応範囲外。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeError(pub usize);

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported board size {} (must be 2..=25)", self.0)
    }
}

impl std::error::Error for SizeError {}

/// 着手が不正である理由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveError {
    /// 盤の外。
    OutOfBounds,
    /// 空点ではない。
    Occupied,
    /// 自殺手(相手の石を取れず、自分の連の呼吸点が 0 になる)。
    Suicide,
    /// 同形反復(着手後の盤面が、過去のいずれかの盤面と一致する)。
    Superko,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            MoveError::OutOfBounds => "point is off the board",
            MoveError::Occupied => "point is occupied",
            MoveError::Suicide => "suicide is not allowed",
            MoveError::Superko => "move repeats a previous board position (superko)",
        };
        f.write_str(s)
    }
}

impl std::error::Error for MoveError {}

/// 連(つながった同色の石)と、その呼吸点の数。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Group {
    /// 連に属する石の内部インデックス。
    pub stones: Vec<usize>,
    /// 呼吸点(連に隣接する空点)の数。重複は数えない。
    pub liberties: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Board {
    size: usize,
    width: usize,
    cells: Vec<Cell>,
    hash: u64,
}

impl Board {
    /// 空の盤を作る。`size` は 2〜25。
    pub fn new(size: usize) -> Result<Board, SizeError> {
        if !is_valid_size(size) {
            return Err(SizeError(size));
        }
        let width = size + 2;
        let mut cells = vec![Cell::Wall; width * width];
        for y in 0..size {
            for x in 0..size {
                cells[(y + 1) * width + (x + 1)] = Cell::Empty;
            }
        }
        Ok(Board { size, width, cells, hash: 0 })
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }

    /// 番兵を含む配列の幅(盤サイズ + 2)。
    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    /// 番兵を含む配列全体。
    #[inline]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// 内部インデックスの点の状態。配列の範囲外は `Wall`。
    #[inline]
    pub fn cell(&self, index: usize) -> Cell {
        self.cells.get(index).copied().unwrap_or(Cell::Wall)
    }

    /// (x, y) の点の状態。盤の外は `Wall`。
    #[inline]
    pub fn at(&self, c: Coord) -> Cell {
        if c.in_bounds(self.size) {
            self.cells[c.to_index(self.size)]
        } else {
            Cell::Wall
        }
    }

    /// 盤上の点の隣接 4 点(上下左右)。盤端では `Wall` の点を含む。
    #[inline]
    pub fn neighbors(&self, index: usize) -> [usize; 4] {
        debug_assert!(self.cells[index] != Cell::Wall);
        [index - 1, index + 1, index - self.width, index + self.width]
    }

    /// 盤上の全点の内部インデックス(左上から行優先)。
    pub fn points(&self) -> impl Iterator<Item = usize> + use<> {
        let (size, width) = (self.size, self.width);
        (0..size).flat_map(move |y| (0..size).map(move |x| (y + 1) * width + (x + 1)))
    }

    /// 差分更新している Zobrist ハッシュ。
    #[inline]
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// 盤面全体から Zobrist ハッシュを計算し直す(検証用)。
    pub fn compute_hash(&self) -> u64 {
        let mut hash = 0;
        for y in 0..self.size {
            for x in 0..self.size {
                if let Some(color) = self.cells[(y + 1) * self.width + (x + 1)].color() {
                    hash ^= zobrist::key(x, y, color);
                }
            }
        }
        hash
    }

    /// 盤上にある `color` の石の数。
    pub fn count(&self, color: Color) -> usize {
        let cell = Cell::from(color);
        self.cells.iter().filter(|&&c| c == cell).count()
    }

    /// `index` の石が属する連を flood fill で求める。石がなければ `None`。
    pub fn group(&self, index: usize) -> Option<Group> {
        self.cell(index).color()?;
        let mut stones = Vec::new();
        let liberties = self.collect_group(index, &mut stones);
        Some(Group { stones, liberties })
    }

    /// `start` の石が属する連を `stones` に集め、呼吸点の数を返す。
    fn collect_group(&self, start: usize, stones: &mut Vec<usize>) -> usize {
        let own = self.cells[start];
        debug_assert!(own.color().is_some());
        let mut marks = Marks::new();
        marks.insert(start);
        stones.clear();
        stones.push(start);
        let mut liberties = 0;
        let mut i = 0;
        while i < stones.len() {
            let p = stones[i];
            i += 1;
            for n in self.neighbors(p) {
                let cell = self.cells[n];
                if cell == Cell::Empty {
                    if marks.insert(n) {
                        liberties += 1;
                    }
                } else if cell == own && marks.insert(n) {
                    stones.push(n);
                }
            }
        }
        liberties
    }

    /// `start` の石が属する連に、呼吸点が 1 つでもあるか。
    ///
    /// 着手のたびに呼ぶので、連全体は集めず、呼吸点が見つかった時点で打ち切る
    /// (ヒープも使わない)。結果は `collect_group(..) > 0` と同じ。
    fn has_liberty(&self, start: usize) -> bool {
        let own = self.cells[start];
        debug_assert!(own.color().is_some());
        let mut marks = Marks::new();
        marks.insert(start);
        let mut stack = [0u16; MAX_SIZE * MAX_SIZE];
        stack[0] = start as u16;
        let mut len = 1;
        while len > 0 {
            len -= 1;
            let p = stack[len] as usize;
            for n in self.neighbors(p) {
                let cell = self.cells[n];
                if cell == Cell::Empty {
                    return true;
                }
                if cell == own && marks.insert(n) {
                    stack[len] = n as u16;
                    len += 1;
                }
            }
        }
        false
    }

    #[inline]
    fn toggle_hash(&mut self, index: usize, color: Color) {
        let (x, y) = (index % self.width - 1, index / self.width - 1);
        self.hash ^= zobrist::key(x, y, color);
    }

    #[inline]
    fn put(&mut self, index: usize, color: Color) {
        debug_assert_eq!(self.cells[index], Cell::Empty);
        self.cells[index] = color.into();
        self.toggle_hash(index, color);
    }

    #[inline]
    fn take(&mut self, index: usize) {
        let color = self.cells[index].color().expect("take: no stone");
        self.cells[index] = Cell::Empty;
        self.toggle_hash(index, color);
    }

    /// `index` に `color` の石を打ち、取った相手の石の内部インデックスを返す。
    ///
    /// 手順は次のとおり。同形反復の判定は盤面の履歴が要るので [`crate::Game`] が行う。
    /// 1. 打つ点が空点か確認する。
    /// 2. 石を置く。
    /// 3. 隣接する相手の連のうち、呼吸点が 0 のものを取り除く。
    /// 4. 自分の連の呼吸点が 0 なら、自殺手として不正とする。
    ///
    /// 不正な場合は盤面を一切変更しない。
    pub fn place(&mut self, color: Color, index: usize) -> Result<Vec<usize>, MoveError> {
        match self.cell(index) {
            Cell::Empty => {}
            Cell::Wall => return Err(MoveError::OutOfBounds),
            Cell::Black | Cell::White => return Err(MoveError::Occupied),
        }

        self.put(index, color);

        let opponent = Cell::from(color.opponent());
        let mut captured = Vec::new();
        let mut group = Vec::new();
        for n in self.neighbors(index) {
            // 先に取り除いた連に属していた点は、すでに空点になっている。
            if self.cells[n] == opponent && !self.has_liberty(n) {
                self.collect_group(n, &mut group);
                for &stone in &group {
                    self.take(stone);
                }
                captured.extend_from_slice(&group);
            }
        }

        // 石を取ったなら、取った跡が自分の連の呼吸点になるので自殺手にはならない。
        if captured.is_empty() && !self.has_liberty(index) {
            self.take(index);
            return Err(MoveError::Suicide);
        }

        Ok(captured)
    }

    /// 図から盤面を作る(テストや検証用)。
    ///
    /// 1 行が盤の 1 段(上から順)。`.` または `+` が空点、`X` が黒、`O` が白。
    /// 空白は無視する。行数と列数は等しくなければならない。
    pub fn from_diagram(diagram: &str) -> Result<Board, String> {
        let rows: Vec<Vec<char>> = diagram
            .lines()
            .map(|line| line.chars().filter(|c| !c.is_whitespace()).collect::<Vec<_>>())
            .filter(|row| !row.is_empty())
            .collect();
        let size = rows.len();
        let mut board = Board::new(size).map_err(|e| e.to_string())?;
        for (y, row) in rows.iter().enumerate() {
            if row.len() != size {
                return Err(format!("row {} has {} points, expected {}", y + 1, row.len(), size));
            }
            for (x, &ch) in row.iter().enumerate() {
                let color = match ch {
                    '.' | '+' => continue,
                    'X' | 'x' => Color::Black,
                    'O' | 'o' => Color::White,
                    other => return Err(format!("unexpected character {other:?} in diagram")),
                };
                board.put(Coord::new(x, y).to_index(size), color);
            }
        }
        Ok(board)
    }

    /// [`Board::from_diagram`] で読める形式の図にする。
    pub fn to_diagram(&self) -> String {
        let mut out = String::with_capacity(self.size * (self.size * 2 + 1));
        for y in 0..self.size {
            for x in 0..self.size {
                if x > 0 {
                    out.push(' ');
                }
                out.push(match self.at(Coord::new(x, y)) {
                    Cell::Black => 'X',
                    Cell::White => 'O',
                    _ => '.',
                });
            }
            out.push('\n');
        }
        out
    }
}

/// 座標ラベル付きの盤面(GTP の showboard 用)。
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header: String = GTP_COLUMNS[..self.size]
            .iter()
            .map(|&c| format!(" {}", c as char))
            .collect();
        writeln!(f, "  {header}")?;
        for y in 0..self.size {
            let row = self.size - y;
            write!(f, "{row:>2}")?;
            for x in 0..self.size {
                let ch = match self.at(Coord::new(x, y)) {
                    Cell::Black => 'X',
                    Cell::White => 'O',
                    _ => '.',
                };
                write!(f, " {ch}")?;
            }
            writeln!(f, " {row}")?;
        }
        write!(f, "  {header}")
    }
}
