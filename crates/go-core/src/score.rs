//! 中国ルール(面積計算)による得点計算。
//!
//! 得点は「盤上の自分の石」と「自分の石だけに囲まれた空点」の合計で、白にはコミを加える。
//! 死石の判定は行わず、盤上の石はすべて生きている石として数える。
//! 両方の色に接している空点(ダメやセキの中の空点)は、どちらの得点にもならない。

use std::fmt;

use crate::board::Board;
use crate::color::{Cell, Color};

/// 得点計算の内訳。
#[derive(Clone, PartialEq, Debug)]
pub struct Score {
    pub size: usize,
    pub komi: f64,
    /// 盤上の黒石の数。
    pub black_stones: usize,
    /// 黒石だけに囲まれた空点の数。
    pub black_territory: usize,
    pub white_stones: usize,
    pub white_territory: usize,
    /// どちらの得点にもならない空点の数。
    pub neutral: usize,
    /// 各点の帰属(左上から行優先、長さは size × size)。
    /// 石のある点はその石の色、空点は囲んでいる色、中立の空点は `None`。
    pub ownership: Vec<Option<Color>>,
}

impl Score {
    pub fn of(board: &Board, komi: f64) -> Score {
        let size = board.size();
        let width = board.width();
        let cells = board.cells();
        let to_xy = |index: usize| (index / width - 1) * size + (index % width - 1);

        let mut ownership: Vec<Option<Color>> = vec![None; size * size];
        let mut stones = [0usize; 2];
        let mut territory = [0usize; 2];
        let mut neutral = 0;
        let mut visited = vec![false; cells.len()];
        let mut region = Vec::new();

        for start in board.points() {
            match cells[start] {
                Cell::Black | Cell::White => {
                    let color = cells[start].color().unwrap();
                    stones[color.index()] += 1;
                    ownership[to_xy(start)] = Some(color);
                }
                Cell::Empty if !visited[start] => {
                    // つながった空点の領域を求め、接している石の色を調べる。
                    region.clear();
                    region.push(start);
                    visited[start] = true;
                    let mut touches = [false; 2];
                    let mut i = 0;
                    while i < region.len() {
                        let p = region[i];
                        i += 1;
                        for n in board.neighbors(p) {
                            match cells[n] {
                                Cell::Empty => {
                                    if !visited[n] {
                                        visited[n] = true;
                                        region.push(n);
                                    }
                                }
                                Cell::Black => touches[Color::Black.index()] = true,
                                Cell::White => touches[Color::White.index()] = true,
                                Cell::Wall => {}
                            }
                        }
                    }
                    let owner = match touches {
                        [true, false] => Some(Color::Black),
                        [false, true] => Some(Color::White),
                        _ => None,
                    };
                    match owner {
                        Some(color) => {
                            territory[color.index()] += region.len();
                            for &p in &region {
                                ownership[to_xy(p)] = Some(color);
                            }
                        }
                        None => neutral += region.len(),
                    }
                }
                _ => {}
            }
        }

        Score {
            size,
            komi,
            black_stones: stones[0],
            black_territory: territory[0],
            white_stones: stones[1],
            white_territory: territory[1],
            neutral,
            ownership,
        }
    }

    /// 黒の地(石 + 囲んだ空点)。
    pub fn black_area(&self) -> usize {
        self.black_stones + self.black_territory
    }

    /// 白の地(石 + 囲んだ空点)。コミは含まない。
    pub fn white_area(&self) -> usize {
        self.white_stones + self.white_territory
    }

    /// 黒の得点。
    pub fn black_total(&self) -> f64 {
        self.black_area() as f64
    }

    /// 白の得点(コミ込み)。
    pub fn white_total(&self) -> f64 {
        self.white_area() as f64 + self.komi
    }

    /// 黒から見た得点差(正なら黒の勝ち)。
    pub fn margin(&self) -> f64 {
        self.black_total() - self.white_total()
    }

    pub fn result(&self) -> GameResult {
        let margin = self.margin();
        if margin > 0.0 {
            GameResult::Score { winner: Color::Black, margin }
        } else if margin < 0.0 {
            GameResult::Score { winner: Color::White, margin: -margin }
        } else {
            GameResult::Draw
        }
    }
}

/// 対局結果。表記は `B+3.5`、`W+R`(R は投了)。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GameResult {
    /// 計算による決着。`margin` は勝者から見た得点差(正の値)。
    Score { winner: Color, margin: f64 },
    /// 投了による決着。
    Resign { winner: Color },
    /// 同点(コミが整数のときだけ起こる)。
    Draw,
}

impl GameResult {
    pub fn winner(&self) -> Option<Color> {
        match *self {
            GameResult::Score { winner, .. } | GameResult::Resign { winner } => Some(winner),
            GameResult::Draw => None,
        }
    }
}

impl fmt::Display for GameResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            GameResult::Score { winner, margin } => write!(f, "{winner}+{margin:.1}"),
            GameResult::Resign { winner } => write!(f, "{winner}+R"),
            GameResult::Draw => f.write_str("Draw"),
        }
    }
}
