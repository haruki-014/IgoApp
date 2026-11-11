//! 段階 1:ランダム選択。
//!
//! 合法手からランダムに選ぶが、自分の眼を埋める手は選ばない。
//! そのような手しか残っていなければパスする。

use go_core::{Board, Cell, Color, Coord, Game, Move};

use crate::rng::Rng;

/// `index` が `color` の眼かどうか。
/// ここでいう眼は「四方が自分の石または盤端である空点」。
#[inline]
pub fn is_own_eye(board: &Board, index: usize, color: Color) -> bool {
    let own = Cell::from(color);
    board.cell(index) == Cell::Empty
        && board.neighbors(index).iter().all(|&n| {
            let cell = board.cell(n);
            cell == own || cell == Cell::Wall
        })
}

/// 自分の眼を埋めない合法手(同形反復・自殺手を除く)をすべて返す。
pub fn candidate_points(game: &Game, color: Color) -> Vec<Coord> {
    let size = game.size();
    game.legal_points(color)
        .into_iter()
        .filter(|c| !is_own_eye(game.board(), c.to_index(size), color))
        .collect()
}

/// ランダムに 1 手選ぶ。候補がなければパス。
pub fn random_move(game: &Game, color: Color, rng: &mut Rng) -> Move {
    let candidates = candidate_points(game, color);
    if candidates.is_empty() {
        Move::Pass
    } else {
        Move::Play(candidates[rng.below(candidates.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use go_core::Board;

    fn game_from(diagram: &str) -> Game {
        let target = Board::from_diagram(diagram).unwrap();
        let size = target.size();
        let mut game = Game::new(size, 7.5).unwrap();
        for color in Color::BOTH {
            for y in 0..size {
                for x in 0..size {
                    if target.at(Coord::new(x, y)) == Cell::from(color) {
                        game.play_as(color, Move::Play(Coord::new(x, y))).unwrap();
                    }
                }
            }
        }
        assert_eq!(game.board(), &target);
        game
    }

    #[test]
    fn detects_eyes() {
        let board = Board::from_diagram(
            ". X . . .
             X X . . .
             . . X . .
             . X . X .
             . . X . O",
        )
        .unwrap();
        let idx = |x, y| Coord::new(x, y).to_index(5);
        // 隅(盤端 2 つ + 自分の石 2 つ)と中央(自分の石 4 つ)。
        assert!(is_own_eye(&board, idx(0, 0), Color::Black));
        assert!(is_own_eye(&board, idx(2, 3), Color::Black));
        // 相手から見れば眼ではない。
        assert!(!is_own_eye(&board, idx(0, 0), Color::White));
        // 隣に空点や相手の石があれば眼ではない。
        assert!(!is_own_eye(&board, idx(2, 0), Color::Black));
        assert!(!is_own_eye(&board, idx(4, 3), Color::Black));
        // 石のある点は眼ではない。
        assert!(!is_own_eye(&board, idx(1, 0), Color::Black));
    }

    #[test]
    fn never_fills_own_eye() {
        // 黒の打てる点は、眼の (0,0) と、白石に隣接していて眼ではない (4,4) だけ。
        let game = game_from(
            ". X X X X
             X X X X X
             X X X X X
             X X X X X
             X X X O .",
        );
        assert_eq!(game.legal_points(Color::Black).len(), 2);
        let mut rng = Rng::from_seed(1);
        for _ in 0..50 {
            assert_eq!(random_move(&game, Color::Black, &mut rng), Move::Play(Coord::new(4, 4)));
        }
    }

    #[test]
    fn passes_when_only_eyes_remain() {
        let game = game_from(
            ". X . X X
             X X X X X
             X X X X X
             X X X X X
             X X X X X",
        );
        let mut rng = Rng::from_seed(2);
        assert!(candidate_points(&game, Color::Black).is_empty());
        assert_eq!(random_move(&game, Color::Black, &mut rng), Move::Pass);
        // 白はどちらの眼にも打てない(自殺手)のでパス。
        assert_eq!(random_move(&game, Color::White, &mut rng), Move::Pass);
    }

    #[test]
    fn chooses_only_legal_moves_and_games_terminate() {
        for (size, seed) in [(5, 3), (9, 4), (9, 5), (13, 6)] {
            let mut rng = Rng::from_seed(seed);
            let mut game = Game::new(size, 7.5).unwrap();
            while !game.is_over() {
                let color = game.to_move();
                let mv = random_move(&game, color, &mut rng);
                game.play(mv).expect("random_move must return a legal move");
                assert!(game.move_number() < size * size * 20, "game does not terminate");
            }
            // 終局時、どちらにも「眼ではない合法手」が残っていない。
            for color in Color::BOTH {
                assert!(candidate_points(&game, color).is_empty());
            }
        }
    }
}
