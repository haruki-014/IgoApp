//! ルールエンジンのテスト。図は上が 1 行目(y = 0)、`X` が黒、`O` が白、`.` が空点。

use std::collections::HashSet;

use go_core::{Board, Cell, Color, Coord, Game, GameResult, Move, MoveError, PlayedMove, sgf};

const B: Color = Color::Black;
const W: Color = Color::White;

fn at(x: usize, y: usize) -> Move {
    Move::Play(Coord::new(x, y))
}

/// 図の局面を、着手履歴を持つ対局として作る(黒石をすべて置いてから白石を置く)。
/// 並べる途中で石が取られる図は扱えない。
fn game_from(diagram: &str, komi: f64) -> Game {
    let target = Board::from_diagram(diagram).expect("valid diagram");
    let size = target.size();
    let mut game = Game::new(size, komi).unwrap();
    for color in [B, W] {
        for y in 0..size {
            for x in 0..size {
                if target.at(Coord::new(x, y)) == Cell::from(color) {
                    game.play_as(color, at(x, y)).expect("setup move is legal");
                }
            }
        }
    }
    assert_eq!(game.board(), &target, "setup must not capture anything");
    assert_eq!(game.captures(B) + game.captures(W), 0);
    game
}

/// 不正手を試し、対局の状態がまったく変わらないことも確認する。
fn assert_rejected(game: &mut Game, color: Color, mv: Move, expected: MoveError) {
    let before = game.clone();
    assert_eq!(game.check(color, mv), Err(expected));
    assert!(!game.is_legal(color, mv));
    assert_eq!(game.play_as(color, mv), Err(expected));
    assert_eq!(game.board(), before.board(), "board must be unchanged");
    assert_eq!(game.board().hash(), before.board().hash());
    assert_eq!(game.moves(), before.moves());
    assert_eq!(game.captures(B), before.captures(B));
    assert_eq!(game.captures(W), before.captures(W));
    assert_eq!(game.to_move(), before.to_move());
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

// ───────────────────────── 石を取る ─────────────────────────

#[test]
fn captures_a_single_stone() {
    let mut game = game_from(
        ". . . . .
         . . X . .
         . X O X .
         . . . . .
         . . . . .",
        7.5,
    );
    let captured = game.play_as(B, at(2, 3)).unwrap();
    assert_eq!(captured, vec![Coord::new(2, 2)]);
    assert_eq!(game.board().at(Coord::new(2, 2)), Cell::Empty);
    assert_eq!(game.captures(B), 1);
    assert_eq!(game.captures(W), 0);
    assert_eq!(game.board().count(W), 0);
}

#[test]
fn captures_a_multi_stone_group() {
    let mut game = game_from(
        ". X X . .
         X O O X .
         . X O X .
         . . . . .
         . . . . .",
        7.5,
    );
    let mut captured = game.play_as(B, at(2, 3)).unwrap();
    captured.sort();
    assert_eq!(captured, vec![Coord::new(1, 1), Coord::new(2, 1), Coord::new(2, 2)]);
    assert_eq!(game.captures(B), 3);
    assert_eq!(game.board().count(W), 0);
}

#[test]
fn captures_several_groups_at_once() {
    // (2,1) は、左右の白 1 子と下の白 2 子、あわせて 3 つの連の最後の呼吸点。
    let mut game = game_from(
        ". X . X .
         X O . O X
         . X O X .
         . X O X .
         . . X . .",
        7.5,
    );
    let mut captured = game.play_as(B, at(2, 1)).unwrap();
    captured.sort();
    assert_eq!(
        captured,
        vec![Coord::new(1, 1), Coord::new(2, 2), Coord::new(2, 3), Coord::new(3, 1)]
    );
    assert_eq!(game.captures(B), 4);
    assert_eq!(game.board().count(W), 0);
    assert_eq!(game.board().hash(), game.board().compute_hash());
}

#[test]
fn captures_in_the_corner() {
    let mut game = game_from(
        "O X . . .
         . . . . .
         . . . . .
         . . . . .
         . . . . .",
        7.5,
    );
    assert_eq!(game.play_as(B, at(0, 1)).unwrap(), vec![Coord::new(0, 0)]);
    assert_eq!(game.board().at(Coord::new(0, 0)), Cell::Empty);

    // 右下隅の 2 子。
    let mut game = game_from(
        ". . . . .
         . . . . .
         . . . . .
         . . . X X
         . . . O O",
        7.5,
    );
    let mut captured = game.play_as(B, at(2, 4)).unwrap();
    captured.sort();
    assert_eq!(captured, vec![Coord::new(3, 4), Coord::new(4, 4)]);
    assert_eq!(game.captures(B), 2);
    assert_eq!(game.board().count(W), 0);
}

#[test]
fn captures_on_the_edge() {
    // 上辺の 1 子。
    let mut game = game_from(
        ". X O X .
         . . . . .
         . . . . .
         . . . . .
         . . . . .",
        7.5,
    );
    assert_eq!(game.play_as(B, at(2, 1)).unwrap(), vec![Coord::new(2, 0)]);

    // 左辺の 2 子。
    let mut game = game_from(
        "X . . . .
         O X . . .
         O . . . .
         X . . . .
         . . . . .",
        7.5,
    );
    let mut captured = game.play_as(B, at(1, 2)).unwrap();
    captured.sort();
    assert_eq!(captured, vec![Coord::new(0, 1), Coord::new(0, 2)]);
    assert_eq!(game.captures(B), 2);
}

// ───────────────────────── 自殺手 ─────────────────────────

#[test]
fn rejects_single_stone_suicide() {
    // 隅。
    let mut game = game_from(
        ". X . . .
         X . . . .
         . . . . .
         . . . . .
         . . . . .",
        7.5,
    );
    assert_rejected(&mut game, W, at(0, 0), MoveError::Suicide);
    // 黒自身は打てる。
    assert!(game.is_legal(B, at(0, 0)));

    // 中央。
    let mut game = game_from(
        ". . X . .
         . X . X .
         . . X . .
         . . . . .
         . . . . .",
        7.5,
    );
    assert_rejected(&mut game, W, at(2, 1), MoveError::Suicide);
}

#[test]
fn rejects_multi_stone_suicide() {
    // 白 3 子の最後の呼吸点 (0,0) に白が打つと、4 子の連の呼吸点が 0 になる。
    let mut game = game_from(
        ". O X . .
         O O X . .
         X X . . .
         . . . . .
         . . . . .",
        7.5,
    );
    assert_rejected(&mut game, W, at(0, 0), MoveError::Suicide);
    assert_eq!(game.board().count(W), 3);
    // 黒が打てば白 3 子を取れる。
    assert_eq!(game.play_as(B, at(0, 0)).unwrap().len(), 3);
}

#[test]
fn allows_capture_that_looks_like_suicide() {
    // (0,0) は白 2 子に囲まれているが、その 2 子はどちらも (0,0) が最後の呼吸点。
    let mut game = game_from(
        ". O X . .
         O X . . .
         X . . . .
         . . . . .
         . . . . .",
        7.5,
    );
    let mut captured = game.play_as(B, at(0, 0)).unwrap();
    captured.sort();
    assert_eq!(captured, vec![Coord::new(0, 1), Coord::new(1, 0)]);
    assert_eq!(game.board().at(Coord::new(0, 0)), Cell::Black);
    assert_eq!(game.captures(B), 2);

    // 中央で、四方を相手に囲まれた点に打って 1 子を取る。
    let mut game = game_from(
        ". X O . .
         X O . O .
         . X O . .
         . . . . .
         . . . . .",
        7.5,
    );
    assert_eq!(game.play_as(B, at(2, 1)).unwrap(), vec![Coord::new(1, 1)]);
}

#[test]
fn rejects_occupied_and_out_of_bounds() {
    let mut game = Game::new(9, 7.5).unwrap();
    game.play(at(4, 4)).unwrap();
    assert_rejected(&mut game, W, at(4, 4), MoveError::Occupied);
    assert_rejected(&mut game, B, at(4, 4), MoveError::Occupied);
    assert_rejected(&mut game, W, at(9, 0), MoveError::OutOfBounds);
    assert_rejected(&mut game, W, at(0, 9), MoveError::OutOfBounds);
    assert_rejected(&mut game, W, at(usize::MAX, usize::MAX), MoveError::OutOfBounds);
}

// ───────────────────────── コウ・同形反復 ─────────────────────────

const KO: &str = "
    . X O . .
    X O . O .
    . X O . .
    . . . . .
    . . . . .";

#[test]
fn rejects_simple_ko() {
    let mut game = game_from(KO, 7.5);
    // 黒がコウを取る。
    assert_eq!(game.play_as(B, at(2, 1)).unwrap(), vec![Coord::new(1, 1)]);
    // 白はすぐには取り返せない。
    assert_rejected(&mut game, W, at(1, 1), MoveError::Superko);
    assert!(!game.legal_points(W).contains(&Coord::new(1, 1)));

    // 白がコウ立てを打ち、黒が受けると、盤面が変わるので取り返せる。
    game.play_as(W, at(4, 4)).unwrap();
    game.play_as(B, at(4, 3)).unwrap();
    assert_eq!(game.play_as(W, at(1, 1)).unwrap(), vec![Coord::new(2, 1)]);
    // 今度は黒がすぐには取り返せない。
    assert_rejected(&mut game, B, at(2, 1), MoveError::Superko);
}

#[test]
fn ko_is_still_forbidden_after_passes() {
    // パスでは盤面が変わらないので、パスを挟んでも同じ盤面には戻せない。
    let mut game = game_from(KO, 7.5);
    game.play_as(B, at(2, 1)).unwrap();
    game.play_as(W, Move::Pass).unwrap();
    game.play_as(B, Move::Pass).unwrap();
    assert_rejected(&mut game, W, at(1, 1), MoveError::Superko);
}

#[test]
fn rejects_three_move_cycle_regardless_of_side_to_move() {
    // 「二子取らせて一子取り返す」形。黒 (3,0) → 白 (1,0) で 2 子取り → 黒 (2,0) で
    // 1 子を取り返すと、3 手で最初の盤面に戻る。最初は黒番、戻ったときは白番で
    // 手番が異なるが、盤面だけを比べるので不正。単純なコウ(直前の 1 子取り返し)ではない。
    let mut game = game_from(
        "X . X . O . .
         X X O O O . .
         . . . . . . .
         . . . . . . .
         . . . . . . .
         . . . . . . .
         . . . . . . .",
        7.5,
    );
    let start = game.board().clone();
    assert_eq!(game.to_move(), B);

    assert!(game.play_as(B, at(3, 0)).unwrap().is_empty());
    let mut captured = game.play_as(W, at(1, 0)).unwrap();
    captured.sort();
    assert_eq!(captured, vec![Coord::new(2, 0), Coord::new(3, 0)]);

    // 黒 (2,0) は白 1 子を取れる手だが、盤面が最初と同じになる。
    let mut probe = game.board().clone();
    assert_eq!(probe.place(B, Coord::new(2, 0).to_index(7)).unwrap().len(), 1);
    assert_eq!(probe, start);
    assert_rejected(&mut game, B, at(2, 0), MoveError::Superko);

    // 同じ点でも、盤面が変わったあとなら打てる。
    game.play_as(B, at(6, 6)).unwrap();
    game.play_as(W, at(5, 5)).unwrap();
    assert_eq!(game.play_as(B, at(2, 0)).unwrap(), vec![Coord::new(1, 0)]);
}

#[test]
fn rejects_six_move_cycle_in_triple_ko() {
    // 3 つのコウ。上と中は白石がコウの中にあり(黒が取れる)、下は黒石が中にある(白が取れる)。
    let mut game = game_from(
        ". X O . . . . . .
         X O . O . . . . .
         . X O . . . . . .
         . X O . . . . . .
         X O . O . . . . .
         . X O . . . . . .
         . X O . . . . . .
         X . X O . . . . .
         . X O . . . . . .",
        7.5,
    );
    let start = game.board().clone();
    let black_takes = |ko: usize| at(2, 1 + ko * 3);
    let white_takes = |ko: usize| at(1, 1 + ko * 3);

    // どの手も直前とは別のコウを取るので、単純なコウの取り返しは一度もない。
    assert_eq!(game.play_as(B, black_takes(0)).unwrap().len(), 1);
    assert_eq!(game.play_as(W, white_takes(2)).unwrap().len(), 1);
    assert_eq!(game.play_as(B, black_takes(1)).unwrap().len(), 1);
    assert_eq!(game.play_as(W, white_takes(0)).unwrap().len(), 1);
    assert_eq!(game.play_as(B, black_takes(2)).unwrap().len(), 1);

    // 6 手目で最初の盤面に戻る。
    let mut probe = game.board().clone();
    probe.place(W, Coord::new(1, 4).to_index(9)).unwrap();
    assert_eq!(probe, start);
    assert_rejected(&mut game, W, white_takes(1), MoveError::Superko);
    assert_eq!(game.captures(B), 3);
    assert_eq!(game.captures(W), 2);
}

/// ランダムに打ち進め、石を打った後の盤面が過去のどの盤面とも一致しないことを、
/// Zobrist ハッシュとは独立に(盤面そのものの比較で)確認する。
#[test]
fn no_position_ever_repeats_in_random_games() {
    for (size, seed) in [(2, 11), (3, 12), (3, 13), (4, 14), (5, 15)] {
        let mut rng = Rng(seed);
        let mut game = Game::new(size, 7.5).unwrap();
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(game.board().to_diagram());
        let mut superko_rejections = 0;
        for _ in 0..2000 {
            let color = game.to_move();
            let coord = Coord::new(rng.below(size), rng.below(size));
            let mut probe = game.board().clone();
            let placed = probe.place(color, coord.to_index(size));
            match game.play(Move::Play(coord)) {
                Ok(_) => {
                    assert!(placed.is_ok());
                    assert!(seen.insert(game.board().to_diagram()), "position repeated on {size}x{size}");
                }
                Err(MoveError::Superko) => {
                    assert!(placed.is_ok());
                    assert!(seen.contains(&probe.to_diagram()));
                    superko_rejections += 1;
                    // 打てる点が尽きるのを避けるため、ときどきパスで手番を渡す。
                    if rng.below(3) == 0 {
                        game.play(Move::Pass).unwrap();
                    }
                }
                Err(e) => {
                    assert_eq!(placed.err(), Some(e));
                    if rng.below(3) == 0 {
                        game.play(Move::Pass).unwrap();
                    }
                }
            }
        }
        assert!(superko_rejections > 0, "{size}x{size}: expected superko to trigger at least once");
    }
}

// ───────────────────────── 得点計算 ─────────────────────────

#[test]
fn scores_empty_board() {
    for size in [2, 9, 13, 19, 25] {
        let game = Game::new(size, 7.5).unwrap();
        let score = game.score();
        assert_eq!(score.black_stones, 0);
        assert_eq!(score.black_territory, 0);
        assert_eq!(score.white_stones, 0);
        assert_eq!(score.white_territory, 0);
        assert_eq!(score.neutral, size * size);
        assert!(score.ownership.iter().all(|o| o.is_none()));
        assert_eq!(score.black_total(), 0.0);
        assert_eq!(score.white_total(), 7.5);
        assert_eq!(game.result_by_score().to_string(), "W+7.5");
    }
}

#[test]
fn scores_single_stone_owns_the_board() {
    let mut game = Game::new(9, 7.5).unwrap();
    game.play(at(4, 4)).unwrap();
    let score = game.score();
    assert_eq!(score.black_stones, 1);
    assert_eq!(score.black_territory, 80);
    assert_eq!(score.white_area(), 0);
    assert_eq!(score.neutral, 0);
    assert_eq!(game.result_by_score().to_string(), "B+73.5");
}

#[test]
fn scores_simple_territory() {
    let board = Board::from_diagram(
        ". . X O . .
         . . X O . .
         . . X O . .
         . . X O . .
         . . X O . .
         . . X O . .",
    )
    .unwrap();
    let score = go_core::Score::of(&board, 0.5);
    assert_eq!((score.black_stones, score.black_territory), (6, 12));
    assert_eq!((score.white_stones, score.white_territory), (6, 12));
    assert_eq!(score.neutral, 0);
    assert_eq!(score.result().to_string(), "W+0.5");
    assert_eq!(score.ownership[0], Some(B));
    assert_eq!(score.ownership[2], Some(B));
    assert_eq!(score.ownership[3], Some(W));
    assert_eq!(score.ownership[5], Some(W));
}

#[test]
fn scores_position_with_seki() {
    // 中央で、眼のない白の連(c 列と d 列の 3 子)と黒の連(e 列と d 列の 2 子)が
    // 呼吸点 (3,0)・(3,1) を共有するセキ。どちらも先に打つと取られる。
    // 共有している 2 点はどちらの得点にもならない。
    let diagram = "
        . X O . X O .
        . X O . X O .
        . X O O X O .
        . X O O X O .
        . X O O X O .
        . X O X X O .
        . X O X X O .";
    let game = game_from(diagram, 7.5);
    let board = game.board();

    // セキであること:両方の連の呼吸点は共有の 2 点だけ。
    let white_group = board.group(Coord::new(2, 0).to_index(7)).unwrap();
    let black_group = board.group(Coord::new(4, 0).to_index(7)).unwrap();
    assert_eq!((white_group.stones.len(), white_group.liberties), (10, 2));
    assert_eq!((black_group.stones.len(), black_group.liberties), (9, 2));

    let score = game.score();
    assert_eq!(score.black_stones, 16);
    assert_eq!(score.black_territory, 7);
    assert_eq!(score.white_stones, 17);
    assert_eq!(score.white_territory, 7);
    assert_eq!(score.neutral, 2);
    assert_eq!(score.black_area() + score.white_area() + score.neutral, 49);
    assert_eq!(score.black_total(), 23.0);
    assert_eq!(score.white_total(), 31.5);
    assert_eq!(game.result_by_score(), GameResult::Score { winner: W, margin: 8.5 });
    assert_eq!(game.result_by_score().to_string(), "W+8.5");

    // 帰属:左端の列は黒地、右端の列は白地、共有の呼吸点は中立。
    let own = |x: usize, y: usize| score.ownership[y * 7 + x];
    for y in 0..7 {
        assert_eq!(own(0, y), Some(B));
        assert_eq!(own(6, y), Some(W));
    }
    assert_eq!(own(3, 0), None);
    assert_eq!(own(3, 1), None);
    assert_eq!(own(3, 2), Some(W));
    assert_eq!(own(3, 5), Some(B));

    // セキの中に打つ手は自殺手ではない(自分の呼吸点を 1 つ残す)ので合法。
    assert!(game.is_legal(B, at(3, 0)));
    assert!(game.is_legal(W, at(3, 1)));
}

#[test]
fn scores_dead_stones_as_they_stand() {
    // 死石の判定はしない。黒の囲いの中に残った白 1 子は白の石として数え、
    // その周りの空点は両方の色に接するので中立になる。
    let board = Board::from_diagram(
        ". . . X O .
         . O . X O .
         . . . X O .
         X X X X O .
         O O O O O .
         . . . . . .",
    )
    .unwrap();
    let score = go_core::Score::of(&board, 7.5);
    assert_eq!(score.black_stones, 7);
    assert_eq!(score.black_territory, 0);
    assert_eq!(score.white_stones, 10);
    assert_eq!(score.white_territory, 11);
    assert_eq!(score.neutral, 8);
    assert_eq!(score.result().to_string(), "W+21.5");
}

#[test]
fn result_notation() {
    assert_eq!(GameResult::Score { winner: B, margin: 3.5 }.to_string(), "B+3.5");
    assert_eq!(GameResult::Score { winner: W, margin: 12.5 }.to_string(), "W+12.5");
    assert_eq!(GameResult::Resign { winner: W }.to_string(), "W+R");
    assert_eq!(GameResult::Resign { winner: B }.to_string(), "B+R");
    assert_eq!(GameResult::Resign { winner: B }.winner(), Some(B));
    assert_eq!(GameResult::Draw.winner(), None);
}

// ───────────────────────── パス・終局・履歴 ─────────────────────────

#[test]
fn pass_is_always_legal_and_two_passes_end_the_game() {
    let mut game = Game::new(9, 7.5).unwrap();
    assert!(!game.is_over());
    assert!(game.is_legal(B, Move::Pass));
    game.play(Move::Pass).unwrap();
    assert!(!game.is_over());
    assert_eq!(game.to_move(), W);
    game.play(at(2, 2)).unwrap();
    assert!(!game.is_over());
    game.play(Move::Pass).unwrap();
    assert!(!game.is_over(), "passes must be consecutive");
    game.play(Move::Pass).unwrap();
    assert!(game.is_over());
    assert_eq!(game.move_number(), 4);
    // 盤上は白 1 子だけなので、全体が白地。
    assert_eq!(game.result_by_score().to_string(), "W+88.5");
}

#[test]
fn turn_alternates_and_legal_moves_are_enumerated() {
    let mut game = Game::new(3, 7.5).unwrap();
    assert_eq!(game.to_move(), B);
    assert_eq!(game.legal_points(B).len(), 9);
    let moves = game.legal_moves(B);
    assert_eq!(moves.len(), 10);
    assert_eq!(moves.last(), Some(&Move::Pass));

    game.play(at(1, 1)).unwrap();
    assert_eq!(game.to_move(), W);
    assert_eq!(game.last_move(), Some(PlayedMove { color: B, mv: at(1, 1) }));
    assert_eq!(game.legal_points(W).len(), 8);

    // 列挙の結果は、1 点ずつ調べた結果と一致する(自殺手とコウを除く)。
    let mut game = game_from(KO, 7.5);
    game.play_as(B, at(2, 1)).unwrap();
    for color in [B, W] {
        let listed: HashSet<Coord> = game.legal_points(color).into_iter().collect();
        for y in 0..5 {
            for x in 0..5 {
                let c = Coord::new(x, y);
                assert_eq!(listed.contains(&c), game.is_legal(color, Move::Play(c)), "{color} {c:?}");
            }
        }
    }
    assert!(!game.is_legal(W, at(1, 1)), "ko");
    assert!(!game.is_legal(B, at(2, 1)), "occupied");
}

#[test]
fn history_rebuilds_the_same_game_and_undo_works() {
    let mut rng = Rng(2024);
    let mut game = Game::new(9, 6.5).unwrap();
    while game.move_number() < 150 {
        let mv = if rng.below(20) == 0 {
            Move::Pass
        } else {
            at(rng.below(9), rng.below(9))
        };
        let _ = game.play(mv);
    }
    assert!(game.captures(B) + game.captures(W) > 0, "the random game should contain captures");

    // 履歴からの再構築。
    let rebuilt = Game::from_moves(9, 6.5, game.moves()).unwrap();
    assert_eq!(rebuilt.board(), game.board());
    assert_eq!(rebuilt.moves(), game.moves());
    assert_eq!(rebuilt.captures(B), game.captures(B));
    assert_eq!(rebuilt.captures(W), game.captures(W));
    assert_eq!(rebuilt.to_move(), game.to_move());

    // 待った:途中まで戻した状態は、そこまでの履歴から作った対局と一致する。
    for count in [1, 2, 7, 50] {
        let mut undone = game.clone();
        undone.undo(count).unwrap();
        let expected = Game::from_moves(9, 6.5, &game.moves()[..150 - count]).unwrap();
        assert_eq!(undone.board(), expected.board());
        assert_eq!(undone.moves(), expected.moves());
        assert_eq!(undone.captures(B), expected.captures(B));
        assert_eq!(undone.captures(W), expected.captures(W));
        assert_eq!(undone.komi(), 6.5);
        // 戻したあとも、同形反復の履歴が正しく引き継がれている。
        for y in 0..9 {
            for x in 0..9 {
                for color in [B, W] {
                    assert_eq!(undone.check(color, at(x, y)), expected.check(color, at(x, y)));
                }
            }
        }
    }

    let mut all = game.clone();
    all.undo(150).unwrap();
    assert_eq!(all.board(), Game::new(9, 6.5).unwrap().board());
    assert_eq!(all.move_number(), 0);
    assert!(game.clone().undo(151).is_err());
}

#[test]
fn undo_lifts_the_ko_ban() {
    let mut game = game_from(KO, 7.5);
    game.play_as(B, at(2, 1)).unwrap();
    assert!(!game.is_legal(W, at(1, 1)));
    game.undo(1).unwrap();
    // 黒の取りを取り消したので、白石は (1,1) に戻っている。
    assert_eq!(game.board().at(Coord::new(1, 1)), Cell::White);
    assert_eq!(game.captures(B), 0);
    assert!(game.is_legal(B, at(2, 1)));
}

#[test]
fn replay_reports_the_illegal_move() {
    let moves = [
        PlayedMove { color: B, mv: at(0, 0) },
        PlayedMove { color: W, mv: at(0, 0) },
    ];
    match Game::from_moves(9, 7.5, &moves) {
        Err(go_core::ReplayError::Move { index, error }) => {
            assert_eq!(index, 1);
            assert_eq!(error, MoveError::Occupied);
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(Game::from_moves(1, 7.5, &[]).is_err());
}

// ───────────────────────── Zobrist ハッシュ ─────────────────────────

/// 差分更新した値が、盤面全体から計算し直した値と常に一致する。
#[test]
fn zobrist_incremental_matches_recomputed() {
    for (size, seed) in [(2, 1), (3, 2), (5, 3), (9, 4), (13, 5), (19, 6), (25, 7)] {
        let mut rng = Rng(seed * 7919);
        let mut game = Game::new(size, 7.5).unwrap();
        assert_eq!(game.board().hash(), 0);
        let mut hashes_by_position = std::collections::HashMap::new();
        let mut captures_seen = 0;
        for _ in 0..(size * size * 6) {
            let color = game.to_move();
            let coord = Coord::new(rng.below(size), rng.below(size));
            let before = game.board().hash();
            match game.play_as(color, Move::Play(coord)) {
                Ok(captured) => captures_seen += captured.len(),
                Err(_) => {
                    assert_eq!(game.board().hash(), before, "illegal move must not change the hash");
                    if rng.below(4) == 0 {
                        game.play(Move::Pass).unwrap();
                    }
                }
            }
            assert_eq!(game.board().hash(), game.board().compute_hash(), "size {size}");
            // 同じ盤面は同じハッシュ、異なる盤面は異なるハッシュになっている。
            let previous = hashes_by_position.insert(game.board().to_diagram(), game.board().hash());
            if let Some(previous) = previous {
                assert_eq!(previous, game.board().hash());
            }
        }
        let distinct: HashSet<u64> = hashes_by_position.values().copied().collect();
        assert_eq!(distinct.len(), hashes_by_position.len(), "hash collision on size {size}");
        assert!(captures_seen > 0, "size {size}: the playout should include captures");

        // 履歴から作り直した盤面も同じハッシュになる。
        let rebuilt = Game::from_moves(size, 7.5, game.moves()).unwrap();
        assert_eq!(rebuilt.board().hash(), game.board().hash());
        // 図から直接作った盤面も同じハッシュになる。
        let from_diagram = Board::from_diagram(&game.board().to_diagram()).unwrap();
        assert_eq!(from_diagram.hash(), game.board().hash());
        assert_eq!(&from_diagram, game.board());
    }
}

#[test]
fn zobrist_hash_does_not_depend_on_move_order() {
    let mut a = Game::new(9, 7.5).unwrap();
    a.play_as(B, at(2, 2)).unwrap();
    a.play_as(W, at(6, 6)).unwrap();
    a.play_as(B, at(2, 6)).unwrap();
    let mut b = Game::new(9, 7.5).unwrap();
    b.play_as(B, at(2, 6)).unwrap();
    b.play_as(B, at(2, 2)).unwrap();
    b.play_as(W, at(6, 6)).unwrap();
    assert_eq!(a.board().hash(), b.board().hash());
    assert_ne!(a.board().hash(), 0);

    // 色が違えばハッシュも違う。
    let mut c = Game::new(9, 7.5).unwrap();
    c.play_as(W, at(2, 2)).unwrap();
    let mut d = Game::new(9, 7.5).unwrap();
    d.play_as(B, at(2, 2)).unwrap();
    assert_ne!(c.board().hash(), d.board().hash());
}

// ───────────────────────── 盤サイズ ─────────────────────────

#[test]
fn rejects_unsupported_sizes() {
    for size in [0, 1, 26, 27, 100] {
        assert!(Board::new(size).is_err(), "size {size}");
        assert!(Game::new(size, 7.5).is_err(), "size {size}");
    }
}

#[test]
fn board_has_sentinel_walls() {
    for size in [2, 9, 13, 19, 25] {
        let board = Board::new(size).unwrap();
        let width = size + 2;
        assert_eq!(board.width(), width);
        assert_eq!(board.cells().len(), width * width);
        let walls = board.cells().iter().filter(|&&c| c == Cell::Wall).count();
        assert_eq!(walls, width * width - size * size);
        assert_eq!(board.points().count(), size * size);
        for index in board.points() {
            assert_eq!(board.cell(index), Cell::Empty);
            // 隣接点は ±1 と ±(盤サイズ + 2)。
            assert_eq!(board.neighbors(index), [index - 1, index + 1, index - width, index + width]);
        }
        // 隅の点は 2 方向が番兵。
        let corner = Coord::new(0, 0).to_index(size);
        let wall_neighbors = board.neighbors(corner).iter().filter(|&&n| board.cell(n) == Cell::Wall).count();
        assert_eq!(wall_neighbors, 2);
        assert_eq!(board.at(Coord::new(size, 0)), Cell::Wall);
    }
}

#[test]
fn smallest_board_2x2() {
    let mut game = Game::new(2, 0.5).unwrap();
    game.play(at(0, 0)).unwrap(); // 黒
    game.play(at(1, 1)).unwrap(); // 白
    game.play(at(1, 0)).unwrap(); // 黒:2 子の呼吸点は (0,1) だけ
    // 白 (0,1) は黒 2 子を取る。
    let mut captured = game.play(at(0, 1)).unwrap();
    captured.sort();
    assert_eq!(captured, vec![Coord::new(0, 0), Coord::new(1, 0)]);
    assert_eq!(game.captures(W), 2);
    assert_eq!(game.board().count(B), 0);
    assert_eq!(game.board().count(W), 2);
    // 黒 (0,0) は呼吸点 (1,0) が残るので打てる。
    assert!(game.is_legal(B, at(0, 0)));
    game.play(at(0, 0)).unwrap();
    // 白 (1,0) で黒 1 子を取ると、盤上は白 3 子だけになる。
    assert_eq!(game.play(at(1, 0)).unwrap(), vec![Coord::new(0, 0)]);
    assert_eq!(game.captures(W), 3);
    // 黒 (0,0) は白 3 子を取れる手だが、盤面が 1 手目の直後(黒 1 子だけ)と同じになる。
    assert_rejected(&mut game, B, at(0, 0), MoveError::Superko);
    assert_eq!(game.legal_moves(B), vec![Move::Pass]);
    let score = game.score();
    assert_eq!(score.black_area() + score.white_area() + score.neutral, 4);
    assert_eq!(game.result_by_score().to_string(), "W+4.5");
    assert_eq!(Move::from_gtp("B2", 2), Ok(at(1, 0)));
}

#[test]
fn largest_board_25x25() {
    let mut game = Game::new(25, 7.5).unwrap();
    // 四隅と中央。
    for mv in ["A1", "Z25", "A25", "Z1", "N13"] {
        let mv = Move::from_gtp(mv, 25).unwrap();
        game.play(mv).unwrap();
    }
    assert_eq!(game.board().at(Coord::new(0, 24)), Cell::Black); // A1
    assert_eq!(game.board().at(Coord::new(24, 0)), Cell::White); // Z25
    assert_eq!(game.board().at(Coord::new(12, 12)), Cell::Black); // N13
    assert_eq!(game.board().hash(), game.board().compute_hash());

    // 隅の石を取る。
    game.play_as(B, Move::from_gtp("Y25", 25).unwrap()).unwrap();
    let captured = game.play_as(B, Move::from_gtp("Z24", 25).unwrap()).unwrap();
    assert_eq!(captured, vec![Coord::new(24, 0)]);
    let score = game.score();
    assert_eq!(score.black_area() + score.white_area() + score.neutral, 625);
    assert_eq!(score.black_stones, 5);
    assert_eq!(score.white_stones, 1);
}

/// 2〜25 路のすべてで、ランダムな対局が最後まで矛盾なく進む。
#[test]
fn random_games_on_every_size() {
    for size in 2..=25 {
        let mut rng = Rng(size as u64 * 104_729 + 1);
        let mut game = Game::new(size, 7.5).unwrap();
        let limit = size * size * 3;
        while !game.is_over() && game.move_number() < limit {
            let color = game.to_move();
            let legal = game.legal_points(color);
            let mv = if legal.is_empty() || rng.below(40) == 0 {
                Move::Pass
            } else {
                Move::Play(legal[rng.below(legal.len())])
            };
            game.play(mv).unwrap_or_else(|e| panic!("size {size}: listed move rejected: {e}"));
            assert_eq!(game.board().hash(), game.board().compute_hash());
        }
        let board = game.board();
        // 盤上に呼吸点 0 の連が残っていない。
        for index in board.points() {
            if let Some(group) = board.group(index) {
                assert!(group.liberties > 0, "size {size}: group without liberties");
            }
        }
        let score = game.score();
        assert_eq!(score.black_stones, board.count(B));
        assert_eq!(score.white_stones, board.count(W));
        assert_eq!(score.black_area() + score.white_area() + score.neutral, size * size);
        // 石の数は、置いた数から取られた数を引いたものと一致する。
        let placed = |c: Color| game.moves().iter().filter(|m| m.color == c && m.mv != Move::Pass).count();
        assert_eq!(board.count(B), placed(B) - game.captures(W) as usize);
        assert_eq!(board.count(W), placed(W) - game.captures(B) as usize);
        let rebuilt = Game::from_moves(size, 7.5, game.moves()).unwrap();
        assert_eq!(rebuilt.board(), board);
    }
}

// ───────────────────────── 着手表記・SGF ─────────────────────────

#[test]
fn move_notation_roundtrip() {
    for size in [2, 9, 13, 19, 25] {
        assert_eq!(Move::from_gtp("pass", size), Ok(Move::Pass));
        assert_eq!(Move::from_gtp("PASS", size), Ok(Move::Pass));
        assert_eq!(Move::Pass.to_gtp(size), "pass");
        assert_eq!(Move::from_sgf("", size), Ok(Move::Pass));
        assert_eq!(Move::Pass.to_sgf(), "");
        for y in 0..size {
            for x in 0..size {
                let mv = at(x, y);
                assert_eq!(Move::from_gtp(&mv.to_gtp(size), size), Ok(mv));
                assert_eq!(Move::from_sgf(&mv.to_sgf(), size), Ok(mv));
            }
        }
    }
    // 19 路以下では tt もパス。それより大きい盤では tt は実在の点。
    assert_eq!(Move::from_sgf("tt", 19), Ok(Move::Pass));
    assert_eq!(Move::from_sgf("tt", 25), Ok(at(19, 19)));
    assert!(Move::from_gtp("resign", 19).is_err());
}

#[test]
fn sgf_output() {
    let mut game = Game::new(9, 7.5).unwrap();
    game.play(Move::from_gtp("E5", 9).unwrap()).unwrap();
    game.play(Move::from_gtp("C3", 9).unwrap()).unwrap();
    game.play(Move::Pass).unwrap();
    game.play(Move::Pass).unwrap();
    let text = sgf::to_sgf(
        &game,
        &sgf::SgfInfo { black: "human", white: "katago [b6]", result: Some("B+R") },
    );
    assert!(text.starts_with("(;GM[1]FF[4]"));
    assert!(text.contains("SZ[9]"));
    assert!(text.contains("KM[7.5]"));
    assert!(text.contains("RU[Chinese]"));
    assert!(text.contains("PB[human]"));
    assert!(text.contains("PW[katago [b6\\]]"), "']' must be escaped: {text}");
    assert!(text.contains("RE[B+R]"));
    assert!(text.contains(";B[ee];W[cg];B[];W[]"));
    assert!(text.trim_end().ends_with(')'));

    let unfinished = sgf::to_sgf(&Game::new(19, 6.5).unwrap(), &sgf::SgfInfo { black: "a", white: "b", result: None });
    assert!(unfinished.contains("SZ[19]KM[6.5]"));
    assert!(!unfinished.contains("RE["));
}
