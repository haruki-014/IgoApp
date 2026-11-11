//! 対局のメタ情報と、クライアントへ配信する状態。

use go_core::{Cell, Color, Coord, Game, Move, PlayedMove, Score};
use serde::Serialize;
use uuid::Uuid;

/// DB の black_player / white_player で人間の席を表す値。
pub const HUMAN: &str = "human";

/// 席に座るプレイヤー。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Player {
    Human,
    /// エンジン名。
    Engine(String),
}

impl Player {
    pub fn from_db(value: &str) -> Player {
        if value == HUMAN {
            Player::Human
        } else {
            Player::Engine(value.to_string())
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Player::Human => HUMAN,
            Player::Engine(name) => name,
        }
    }

    pub fn is_human(&self) -> bool {
        matches!(self, Player::Human)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Playing,
    Finished,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Playing => "playing",
            Status::Finished => "finished",
        }
    }
}

/// 対局中に変わらない情報。添字は `Color::index()`(黒 = 0、白 = 1)。
#[derive(Clone, Debug)]
pub struct GameMeta {
    pub id: Uuid,
    pub size: usize,
    pub komi: f64,
    pub players: [Player; 2],
    /// 席トークンの SHA-256。エンジンの席は `None`。
    pub token_hashes: [Option<String>; 2],
}

impl GameMeta {
    pub fn player(&self, color: Color) -> &Player {
        &self.players[color.index()]
    }

    /// 待ったが使える対局か(人間対 AI のときだけ)。使えるなら人間の色を返す。
    pub fn undo_seat(&self) -> Option<Color> {
        match (&self.players[0], &self.players[1]) {
            (Player::Human, Player::Engine(_)) => Some(Color::Black),
            (Player::Engine(_), Player::Human) => Some(Color::White),
            _ => None,
        }
    }
}

pub fn color_name(color: Color) -> &'static str {
    match color {
        Color::Black => "black",
        Color::White => "white",
    }
}

/// 黒と白それぞれの値。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Seats<T> {
    pub black: T,
    pub white: T,
}

/// 1 手。パスは `x`・`y` が null。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MoveView {
    pub color: &'static str,
    pub x: Option<usize>,
    pub y: Option<usize>,
}

impl From<PlayedMove> for MoveView {
    fn from(m: PlayedMove) -> MoveView {
        let (x, y) = match m.mv {
            Move::Play(c) => (Some(c.x), Some(c.y)),
            Move::Pass => (None, None),
        };
        MoveView { color: color_name(m.color), x, y }
    }
}

/// 終局時の計算の内訳。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScoreView {
    pub black_stones: usize,
    pub black_territory: usize,
    pub white_stones: usize,
    pub white_territory: usize,
    pub neutral: usize,
    pub komi: f64,
    pub black_total: f64,
    pub white_total: f64,
    /// 各点の帰属。1 行が盤の 1 段で、`b` = 黒、`w` = 白、`.` = 中立。
    pub ownership: Vec<String>,
}

impl From<&Score> for ScoreView {
    fn from(score: &Score) -> ScoreView {
        let ownership = score
            .ownership
            .chunks(score.size)
            .map(|row| {
                row.iter()
                    .map(|owner| match owner {
                        Some(Color::Black) => 'b',
                        Some(Color::White) => 'w',
                        None => '.',
                    })
                    .collect()
            })
            .collect();
        ScoreView {
            black_stones: score.black_stones,
            black_territory: score.black_territory,
            white_stones: score.white_stones,
            white_territory: score.white_territory,
            neutral: score.neutral,
            komi: score.komi,
            black_total: score.black_total(),
            white_total: score.white_total(),
            ownership,
        }
    }
}

/// クライアントへ配信する対局の状態(WebSocket の `state` メッセージの本体)。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Snapshot {
    pub id: Uuid,
    pub board_size: usize,
    pub komi: f64,
    /// 各席のプレイヤー。`human` またはエンジン名。
    pub players: Seats<String>,
    /// 盤面。1 行が盤の 1 段(上から)で、`.` = 空点、`X` = 黒、`O` = 白。
    pub board: Vec<String>,
    /// 手番。
    pub to_move: &'static str,
    /// 手数(パスを含む)。
    pub move_number: usize,
    /// 最終手。
    pub last_move: Option<MoveView>,
    /// 着手履歴。
    pub moves: Vec<MoveView>,
    /// アゲハマ(それぞれが取った石の数)。
    pub captures: Seats<u32>,
    /// 対局状態。
    pub status: Status,
    /// 結果(`B+3.5`、`W+R`)。対局中は null。
    pub result: Option<String>,
    /// 計算の内訳。連続パスで終局したときだけ入る。
    pub score: Option<ScoreView>,
    /// 待ったが使えるか(人間対 AI で、人間の手が 1 手以上あるとき)。
    pub undo_available: bool,
    /// 思考中の AI の色。
    pub thinking: Option<&'static str>,
}

impl Snapshot {
    pub fn build(meta: &GameMeta, game: &Game, status: Status, result: Option<&str>, thinking: Option<Color>) -> Snapshot {
        let size = game.size();
        let board = (0..size)
            .map(|y| {
                (0..size)
                    .map(|x| match game.board().at(Coord::new(x, y)) {
                        Cell::Black => 'X',
                        Cell::White => 'O',
                        _ => '.',
                    })
                    .collect()
            })
            .collect();
        let score = (status == Status::Finished && game.is_over()).then(|| ScoreView::from(&game.score()));
        let undo_available = status == Status::Playing
            && meta.undo_seat().is_some_and(|human| game.moves().iter().any(|m| m.color == human));
        Snapshot {
            id: meta.id,
            board_size: size,
            komi: meta.komi,
            players: Seats {
                black: meta.players[0].as_str().to_string(),
                white: meta.players[1].as_str().to_string(),
            },
            board,
            to_move: color_name(game.to_move()),
            move_number: game.move_number(),
            last_move: game.last_move().map(MoveView::from),
            moves: game.moves().iter().copied().map(MoveView::from).collect(),
            captures: Seats { black: game.captures(Color::Black), white: game.captures(Color::White) },
            status,
            result: result.map(str::to_string),
            score,
            undo_available,
            thinking: thinking.map(color_name),
        }
    }
}
