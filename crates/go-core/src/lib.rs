//! 囲碁のルールエンジン。
//!
//! - 中国ルール(面積計算)
//! - 自殺手は禁止(相手の石を取れる手は自殺手ではない)
//! - ポジショナル・スーパーコウ(手番を問わず、過去の盤面の再現を禁止)
//! - パスは常に合法。連続パスで終局し、盤上の石はすべて生き石として数える
//!
//! このクレートは I/O・非同期処理・DB に一切依存しない。
//! 対局の状態は「着手履歴」を正とし、[`Game::from_moves`] でいつでも再構築できる。

mod board;
mod color;
mod coord;
mod game;
mod score;
pub mod sgf;
pub mod zobrist;

pub use board::{Board, Group, MoveError, SizeError};
pub use color::{Cell, Color};
pub use coord::{Coord, CoordError, GTP_COLUMNS, MAX_SIZE, MIN_SIZE, is_valid_size};
pub use game::{Game, Move, PlayedMove, ReplayError, UndoError};
pub use score::{GameResult, Score};
