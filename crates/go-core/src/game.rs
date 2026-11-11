//! 対局。着手履歴を正とし、盤面は履歴からいつでも再構築できる。

use std::fmt;

use crate::board::{Board, MoveError, SizeError};
use crate::color::{Cell, Color};
use crate::coord::{Coord, CoordError};
use crate::score::{GameResult, Score};

/// 着手。投了は盤面を変えないので、ここには含めない。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Move {
    Play(Coord),
    Pass,
}

impl Move {
    /// GTP の着手表記(`D4`、`pass`)。
    pub fn to_gtp(self, size: usize) -> String {
        match self {
            Move::Play(c) => c.to_gtp(size),
            Move::Pass => "pass".to_string(),
        }
    }

    /// GTP の着手表記を読む。大文字小文字は問わない。
    pub fn from_gtp(s: &str, size: usize) -> Result<Move, CoordError> {
        if s.trim().eq_ignore_ascii_case("pass") {
            Ok(Move::Pass)
        } else {
            Coord::from_gtp(s, size).map(Move::Play)
        }
    }

    /// SGF の着手表記(`pd`、パスは空文字列)。
    pub fn to_sgf(self) -> String {
        match self {
            Move::Play(c) => c.to_sgf(),
            Move::Pass => String::new(),
        }
    }

    /// SGF の着手表記を読む。空文字列はパス。19 路以下では `tt` もパスとして扱う。
    pub fn from_sgf(s: &str, size: usize) -> Result<Move, CoordError> {
        if s.is_empty() || (s == "tt" && size <= 19) {
            Ok(Move::Pass)
        } else {
            Coord::from_sgf(s, size).map(Move::Play)
        }
    }
}

/// 履歴に残る 1 手。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PlayedMove {
    pub color: Color,
    pub mv: Move,
}

/// 着手履歴の再生に失敗した。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ReplayError {
    Size(SizeError),
    /// `index` 番目(0 始まり)の手が不正。
    Move { index: usize, error: MoveError },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Size(e) => e.fmt(f),
            ReplayError::Move { index, error } => write!(f, "move {} is illegal: {}", index + 1, error),
        }
    }
}

impl std::error::Error for ReplayError {}

/// 取り消せる手が足りない。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UndoError;

impl fmt::Display for UndoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not enough moves to undo")
    }
}

impl std::error::Error for UndoError {}

#[derive(Clone, Debug)]
pub struct Game {
    komi: f64,
    board: Board,
    /// 着手履歴。これが対局状態の正で、ほかのフィールドはここから再構築できる。
    moves: Vec<PlayedMove>,
    /// これまでに現れた盤面の Zobrist ハッシュ。`positions[n]` は n 手目を打った後の盤面
    /// (`positions[0]` は空の盤)。同形反復の判定に使う。
    positions: Vec<u64>,
    /// 取った石の数(アゲハマ)。添字は取った側の色。
    captures: [u32; 2],
}

impl Game {
    pub fn new(size: usize, komi: f64) -> Result<Game, SizeError> {
        let board = Board::new(size)?;
        Ok(Game {
            komi,
            positions: vec![board.hash()],
            board,
            moves: Vec::new(),
            captures: [0, 0],
        })
    }

    /// 着手履歴から対局を再構築する。待った・再接続・サーバー再起動後の復元に使う。
    pub fn from_moves(size: usize, komi: f64, moves: &[PlayedMove]) -> Result<Game, ReplayError> {
        let mut game = Game::new(size, komi).map_err(ReplayError::Size)?;
        for (index, m) in moves.iter().enumerate() {
            game.play_as(m.color, m.mv)
                .map_err(|error| ReplayError::Move { index, error })?;
        }
        Ok(game)
    }

    pub fn size(&self) -> usize {
        self.board.size()
    }

    pub fn komi(&self) -> f64 {
        self.komi
    }

    pub fn set_komi(&mut self, komi: f64) {
        self.komi = komi;
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn moves(&self) -> &[PlayedMove] {
        &self.moves
    }

    /// これまでの手数(パスを含む)。
    pub fn move_number(&self) -> usize {
        self.moves.len()
    }

    pub fn last_move(&self) -> Option<PlayedMove> {
        self.moves.last().copied()
    }

    /// 次の手番。初手は黒、以降は直前に打った色の相手。
    pub fn to_move(&self) -> Color {
        match self.moves.last() {
            Some(last) => last.color.opponent(),
            None => Color::Black,
        }
    }

    /// `color` が取った石の数(アゲハマ)。
    pub fn captures(&self, color: Color) -> u32 {
        self.captures[color.index()]
    }

    /// 手番の色で着手する。取った石の座標を返す。
    pub fn play(&mut self, mv: Move) -> Result<Vec<Coord>, MoveError> {
        self.play_as(self.to_move(), mv)
    }

    /// 色を指定して着手する(GTP の `play` は手番と無関係に色を指定できる)。
    ///
    /// 不正な場合は対局の状態を一切変更しない。パスは常に合法。
    /// 終局後かどうかはここでは判定しない。[`Game::is_over`] で呼び出し側が確認する。
    pub fn play_as(&mut self, color: Color, mv: Move) -> Result<Vec<Coord>, MoveError> {
        let captured = match mv {
            Move::Pass => Vec::new(),
            Move::Play(coord) => {
                let (next, captured) = self.try_place(color, coord)?;
                self.board = next;
                self.captures[color.index()] += captured.len() as u32;
                captured
            }
        };
        self.moves.push(PlayedMove { color, mv });
        self.positions.push(self.board.hash());
        Ok(captured)
    }

    /// 着手後の盤面を、現在の盤面を変えずに求める。
    fn try_place(&self, color: Color, coord: Coord) -> Result<(Board, Vec<Coord>), MoveError> {
        let size = self.size();
        if !coord.in_bounds(size) {
            return Err(MoveError::OutOfBounds);
        }
        // 1〜4:空点の確認、石を置く、相手の石を取る、自殺手の判定。
        let mut next = self.board.clone();
        let captured = next.place(color, coord.to_index(size))?;
        // 5:過去の盤面との照合(ポジショナル・スーパーコウ。手番は問わない)。
        if self.positions.contains(&next.hash()) {
            return Err(MoveError::Superko);
        }
        let captured = captured
            .into_iter()
            .map(|index| Coord::from_index(index, size).expect("captured stone is on the board"))
            .collect();
        Ok((next, captured))
    }

    /// 着手が合法かどうかを、対局を変更せずに調べる。
    pub fn check(&self, color: Color, mv: Move) -> Result<(), MoveError> {
        match mv {
            Move::Pass => Ok(()),
            Move::Play(coord) => self.try_place(color, coord).map(|_| ()),
        }
    }

    pub fn is_legal(&self, color: Color, mv: Move) -> bool {
        self.check(color, mv).is_ok()
    }

    /// `color` が打てる点をすべて列挙する(左上から行優先)。パスは含まない。
    pub fn legal_points(&self, color: Color) -> Vec<Coord> {
        let size = self.size();
        let mut points = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let coord = Coord::new(x, y);
                if self.board.at(coord) == Cell::Empty && self.try_place(color, coord).is_ok() {
                    points.push(coord);
                }
            }
        }
        points
    }

    /// `color` の合法手をすべて列挙する。パスは常に合法なので、最後に必ず含まれる。
    pub fn legal_moves(&self, color: Color) -> Vec<Move> {
        let mut moves: Vec<Move> = self.legal_points(color).into_iter().map(Move::Play).collect();
        moves.push(Move::Pass);
        moves
    }

    /// 終局判定。両者が連続でパスしたら終局。
    pub fn is_over(&self) -> bool {
        let n = self.moves.len();
        n >= 2 && self.moves[n - 1].mv == Move::Pass && self.moves[n - 2].mv == Move::Pass
    }

    /// 末尾の `count` 手を取り消す。盤面は残りの着手履歴から再構築する。
    pub fn undo(&mut self, count: usize) -> Result<(), UndoError> {
        if count > self.moves.len() {
            return Err(UndoError);
        }
        let keep = self.moves.len() - count;
        let rebuilt = Game::from_moves(self.size(), self.komi, &self.moves[..keep])
            .expect("a prefix of a legal history is legal");
        *self = rebuilt;
        Ok(())
    }

    /// 現在の盤面を面積計算で数える。
    pub fn score(&self) -> Score {
        Score::of(&self.board, self.komi)
    }

    /// 現在の盤面を数えたときの結果(`B+3.5` など)。
    pub fn result_by_score(&self) -> GameResult {
        self.score().result()
    }
}
