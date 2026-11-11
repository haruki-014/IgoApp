//! 段階 2:MCTS(UCT)。
//!
//! - プレイアウトの着手選択は段階 1 と同じ方針(自分の眼を埋めないランダム選択)。
//! - ルート局面の候補手は go-core の合法手(同形反復・自殺手を除く)から作る。
//! - 木の中とプレイアウトでは、速度のために同形反復を「直前のコウの取り返し」だけで近似する。
//!   実際に打つ手はルートの候補から選ぶので、不正手を返すことはない。
//! - このアプリは連続パスで即終局し、盤上の石をすべて生き石として数える。探索の中でも
//!   同じ扱いにしているので、相手の死石が残っているうちにパスすると損になる局面では、
//!   パスの評価が自然に下がる。

use std::time::{Duration, Instant};

use go_core::{Board, Cell, Color, Coord, Game, Move, Score};

use crate::random::is_own_eye;
use crate::rng::Rng;

#[derive(Clone, Debug)]
pub struct MctsConfig {
    /// 1 手あたりのプレイアウト回数の上限。
    pub playouts: Option<u32>,
    /// 1 手あたりの思考時間の上限。
    pub time: Option<Duration>,
    /// UCT の探索項の係数。
    pub exploration: f64,
}

/// どちらも指定がないときの思考時間。
pub const DEFAULT_TIME: Duration = Duration::from_millis(2000);

impl Default for MctsConfig {
    fn default() -> MctsConfig {
        MctsConfig { playouts: None, time: Some(DEFAULT_TIME), exploration: 0.6 }
    }
}

/// 探索の結果。
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub mv: Move,
    /// 実行したプレイアウトの数。
    pub playouts: u32,
    /// 選んだ手の評価値(手番側から見たもの。ほぼ勝率で、得点差を少し加味している)。
    pub winrate: f64,
}

/// プレイアウト用の軽い局面。同形反復は「直前のコウ」だけを見る。
#[derive(Clone)]
struct Sim {
    board: Board,
    to_move: Color,
    /// コウで打てない点。
    ko: Option<usize>,
    /// 直前から続いているパスの数。
    passes: u8,
    /// 空点の一覧と、各点がその一覧の何番目にあるか。
    empties: Vec<u16>,
    slot: Vec<u16>,
}

const NO_SLOT: u16 = u16::MAX;

impl Sim {
    fn new(game: &Game, to_move: Color) -> Sim {
        let board = game.board().clone();
        let mut empties = Vec::with_capacity(board.size() * board.size());
        let mut slot = vec![NO_SLOT; board.cells().len()];
        for index in board.points() {
            if board.cell(index) == Cell::Empty {
                slot[index] = empties.len() as u16;
                empties.push(index as u16);
            }
        }
        let passes = match game.last_move() {
            Some(last) if last.mv == Move::Pass => 1,
            _ => 0,
        };
        Sim { board, to_move, ko: None, passes, empties, slot }
    }

    fn remove_empty(&mut self, index: usize) {
        let at = self.slot[index] as usize;
        let last = *self.empties.last().expect("empties is not empty");
        self.empties.swap_remove(at);
        if at < self.empties.len() {
            self.slot[last as usize] = at as u16;
        }
        self.slot[index] = NO_SLOT;
    }

    fn add_empty(&mut self, index: usize) {
        self.slot[index] = self.empties.len() as u16;
        self.empties.push(index as u16);
    }

    fn pass(&mut self) {
        self.ko = None;
        self.passes = self.passes.saturating_add(1);
        self.to_move = self.to_move.opponent();
    }

    /// 石を打つ。打てない(石がある・自殺手・コウ)なら何も変えずに `false`。
    fn play(&mut self, index: usize) -> bool {
        if self.ko == Some(index) {
            return false;
        }
        let color = self.to_move;
        let Ok(captured) = self.board.place(color, index) else { return false };
        self.remove_empty(index);
        for &stone in &captured {
            self.add_empty(stone);
        }
        // 1 子を取り、打った石が 1 子で呼吸点が 1 つだけなら、取った跡はコウ。
        self.ko = None;
        if captured.len() == 1 {
            let own = Cell::from(color);
            let neighbors = self.board.neighbors(index);
            let alone = neighbors.iter().all(|&n| self.board.cell(n) != own);
            let liberties = neighbors.iter().filter(|&&n| self.board.cell(n) == Cell::Empty).count();
            if alone && liberties == 1 {
                self.ko = Some(captured[0]);
            }
        }
        self.passes = 0;
        self.to_move = color.opponent();
        true
    }

    fn apply(&mut self, mv: Move) -> bool {
        match mv {
            Move::Pass => {
                self.pass();
                true
            }
            Move::Play(coord) => self.play(coord.to_index(self.board.size())),
        }
    }

    fn is_over(&self) -> bool {
        self.passes >= 2
    }

    /// 段階 1 と同じ方針で 1 手進める:自分の眼を埋めない手からランダムに選び、なければパス。
    fn random_step(&mut self, rng: &mut Rng) {
        let count = self.empties.len();
        if count > 0 {
            let start = rng.below(count);
            for i in 0..count {
                let index = self.empties[(start + i) % count] as usize;
                if !is_own_eye(&self.board, index, self.to_move) && self.play(index) {
                    return;
                }
            }
        }
        self.pass();
    }

    /// 終局まで打ち進め、黒から見た得点差を返す。
    fn playout(mut self, komi: f64, rng: &mut Rng) -> f64 {
        // 取ったり取られたりが続く場合に備えて、手数に上限を設ける。
        let limit = self.board.size() * self.board.size() * 3;
        let mut moves = 0;
        while !self.is_over() && moves < limit {
            self.random_step(rng);
            moves += 1;
        }
        Score::of(&self.board, komi).margin()
    }
}

/// 勝ち負けに対する、得点差の重み。
///
/// 勝ち負けだけで評価すると、勝ちが決まった局面ではどの手も同じ評価になり、
/// 地を増やす手や石を取る手を打たなくなる。得点差を少しだけ加えて、それを防ぐ。
const MARGIN_WEIGHT: f64 = 0.15;

/// プレイアウトの結果を、`color` から見た 0〜1 の評価値にする。
fn reward(black_margin: f64, color: Color, area: f64) -> f64 {
    let margin = if color == Color::Black { black_margin } else { -black_margin };
    let win = if margin > 0.0 { 1.0 } else { 0.0 };
    let scaled = (margin / area).clamp(-1.0, 1.0) * 0.5 + 0.5;
    win * (1.0 - MARGIN_WEIGHT) + scaled * MARGIN_WEIGHT
}

/// いまの盤面をそのまま数えたときの勝者(盤上の石はすべて生き石として数える)。
fn winner_on(board: &Board, komi: f64) -> Color {
    if Score::of(board, komi).margin() > 0.0 { Color::Black } else { Color::White }
}

struct Node {
    /// このノードに至る手と、それを打った色。
    mv: Move,
    color: Color,
    visits: u32,
    /// `color` から見た評価値(勝ち = 1、負け = 0 に得点差を少し加えたもの)の合計。
    wins: f64,
    children: Vec<u32>,
    /// まだ展開していない候補手。
    untried: Vec<Move>,
    /// 候補手を作ったか。
    expanded: bool,
}

impl Node {
    fn new(mv: Move, color: Color) -> Node {
        Node { mv, color, visits: 0, wins: 0.0, children: Vec::new(), untried: Vec::new(), expanded: false }
    }

    fn winrate(&self) -> f64 {
        if self.visits == 0 { 0.0 } else { self.wins / self.visits as f64 }
    }
}

/// ルートの候補手:自分の眼を埋めない合法手。同形反復・自殺手は go-core が除く。
fn root_candidates(game: &Game, color: Color) -> Vec<Coord> {
    crate::random::candidate_points(game, color)
}

/// 木の中のノードの候補手。合法かどうかは、展開するときに実際に打って確かめる。
fn tree_candidates(sim: &Sim, after_pass: bool, rng: &mut Rng) -> Vec<Move> {
    let size = sim.board.size();
    let mut moves: Vec<Move> = sim
        .empties
        .iter()
        .map(|&index| index as usize)
        .filter(|&index| sim.ko != Some(index) && !is_own_eye(&sim.board, index, sim.to_move))
        .map(|index| Move::Play(Coord::from_index(index, size).expect("empty point is on the board")))
        .collect();
    // 末尾から取り出して展開するので、順序を混ぜておく。
    for i in (1..moves.len()).rev() {
        moves.swap(i, rng.below(i + 1));
    }
    // 相手がパスした直後は、こちらもパスして終局させる手を必ず検討する。
    // それ以外では、打てる手があるうちはパスを検討しない(枝を増やさないため)。
    if after_pass || moves.is_empty() {
        moves.insert(0, Move::Pass);
    }
    moves
}

/// `color` の手番で、次の一手を探索する。
pub fn search(game: &Game, color: Color, config: &MctsConfig, rng: &mut Rng) -> SearchResult {
    let komi = game.komi();
    let candidates = root_candidates(game, color);
    if candidates.is_empty() {
        return SearchResult { mv: Move::Pass, playouts: 0, winrate: 0.0 };
    }
    let opponent_passed = game.last_move().is_some_and(|m| m.mv == Move::Pass && m.color != color);
    // 相手がパスしていて、このまま数えて勝ちなら、パスして勝ちを確定させる。
    if opponent_passed && winner_on(game.board(), komi) == color {
        return SearchResult { mv: Move::Pass, playouts: 0, winrate: 1.0 };
    }

    let root_sim = Sim::new(game, color);
    let area = (game.size() * game.size()) as f64;
    let mut nodes: Vec<Node> = Vec::with_capacity(4096);
    let mut root = Node::new(Move::Pass, color.opponent());
    root.expanded = true;
    // ルートではパスも候補に入れ、ほかの手と比べる。
    root.untried.push(Move::Pass);
    root.untried.extend(candidates.iter().copied().map(Move::Play));
    for i in (1..root.untried.len()).rev() {
        root.untried.swap(i, rng.below(i + 1));
    }
    nodes.push(root);

    let (max_playouts, time) = match (config.playouts, config.time) {
        (None, None) => (u32::MAX, Some(DEFAULT_TIME)),
        (playouts, time) => (playouts.unwrap_or(u32::MAX), time),
    };
    let deadline = time.map(|t| Instant::now() + t);

    let mut playouts = 0u32;
    let mut path: Vec<u32> = Vec::with_capacity(128);
    while playouts < max_playouts {
        // 時計を見るのは数回に 1 度で足りる。
        if playouts.is_multiple_of(32) && deadline.is_some_and(|d| Instant::now() >= d) && playouts > 0 {
            break;
        }

        let mut sim = root_sim.clone();
        path.clear();
        path.push(0);
        let mut current = 0usize;

        // 選択:展開済みのノードを UCT で下りる。展開できる手があれば 1 つ展開して止まる。
        loop {
            if sim.is_over() {
                break;
            }
            if !nodes[current].expanded {
                let after_pass = sim.passes > 0;
                nodes[current].untried = tree_candidates(&sim, after_pass, rng);
                nodes[current].expanded = true;
            }
            // 展開:まだ試していない手を 1 つ取り出す(打てない手は捨てる)。
            let mut expanded_child = None;
            while let Some(mv) = nodes[current].untried.pop() {
                let mover = sim.to_move;
                if sim.apply(mv) {
                    let child = nodes.len() as u32;
                    nodes.push(Node::new(mv, mover));
                    nodes[current].children.push(child);
                    expanded_child = Some(child);
                    break;
                }
            }
            if let Some(child) = expanded_child {
                path.push(child);
                break;
            }
            if nodes[current].children.is_empty() {
                // 打てる手が 1 つもない(候補がすべて打てなかった)。パスとして扱う。
                let mover = sim.to_move;
                sim.pass();
                let child = nodes.len() as u32;
                nodes.push(Node::new(Move::Pass, mover));
                nodes[current].children.push(child);
                path.push(child);
                break;
            }
            // UCT で子を選ぶ。
            let parent_visits = (nodes[current].visits.max(1) as f64).ln();
            let mut best = nodes[current].children[0];
            let mut best_value = f64::NEG_INFINITY;
            for &child in &nodes[current].children {
                let node = &nodes[child as usize];
                let visits = node.visits.max(1) as f64;
                let value = node.wins / visits + config.exploration * (parent_visits / visits).sqrt();
                if value > best_value {
                    best_value = value;
                    best = child;
                }
            }
            let applied = sim.apply(nodes[best as usize].mv);
            debug_assert!(applied, "a move that was legal at expansion is legal on the same path");
            path.push(best);
            current = best as usize;
        }

        // プレイアウトと、結果の逆伝播。
        let black_margin = sim.playout(komi, rng);
        for &index in &path {
            let node = &mut nodes[index as usize];
            node.visits += 1;
            node.wins += reward(black_margin, node.color, area);
        }
        playouts += 1;
    }

    choose(&nodes, playouts)
}

/// 探索の結果から手を選ぶ。
///
/// 基本は、いちばん多く探索された手(パス以外)。パスを選ぶのは、パスがほかの手と同じくらい
/// 探索され(= UCT がパスを見捨てていない)、勝率でも最善手より明確に悪くない場合に限る。
fn choose(nodes: &[Node], playouts: u32) -> SearchResult {
    let root = &nodes[0];
    let mut best: Option<&Node> = None;
    let mut pass: Option<&Node> = None;
    for &child in &root.children {
        let node = &nodes[child as usize];
        if node.mv == Move::Pass {
            pass = Some(node);
        } else if best.is_none_or(|b| node.visits > b.visits) {
            best = Some(node);
        }
    }
    let Some(best) = best else {
        return SearchResult { mv: Move::Pass, playouts, winrate: pass.map_or(0.0, Node::winrate) };
    };
    if let Some(pass) = pass {
        let explored_enough = pass.visits >= 30 && pass.visits as f64 >= best.visits as f64 * 0.8;
        if explored_enough && pass.winrate() >= best.winrate() - 0.01 {
            return SearchResult { mv: Move::Pass, playouts, winrate: pass.winrate() };
        }
    }
    SearchResult { mv: best.mv, playouts, winrate: best.winrate() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::random_move;

    fn game_from(diagram: &str, komi: f64) -> Game {
        let target = Board::from_diagram(diagram).unwrap();
        let size = target.size();
        let mut game = Game::new(size, komi).unwrap();
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

    fn fixed(playouts: u32) -> MctsConfig {
        MctsConfig { playouts: Some(playouts), time: None, ..MctsConfig::default() }
    }

    #[test]
    fn sim_tracks_empties_and_ko() {
        let game = game_from(
            ". X O . .
             X O . O .
             . X O . .
             . . . . .
             . . . . .",
            7.5,
        );
        let mut sim = Sim::new(&game, Color::Black);
        let idx = |x, y| Coord::new(x, y).to_index(5);
        assert_eq!(sim.empties.len(), 25 - 7);
        // 黒がコウを取る。
        assert!(sim.play(idx(2, 1)));
        assert_eq!(sim.ko, Some(idx(1, 1)));
        assert_eq!(sim.empties.len(), 25 - 7);
        assert_eq!(sim.to_move, Color::White);
        // 白はすぐには取り返せない。盤面も変わらない。
        let before = sim.board.clone();
        assert!(!sim.play(idx(1, 1)));
        assert_eq!(sim.board, before);
        assert_eq!(sim.to_move, Color::White);
        // 石のある点には打てない。
        assert!(!sim.play(idx(2, 1)));
        // ほかの手を打てば、コウの禁止は解ける。
        assert!(sim.play(idx(4, 4)));
        assert_eq!(sim.ko, None);
        // 空点の一覧は盤面と一致している。
        let mut listed: Vec<usize> = sim.empties.iter().map(|&i| i as usize).collect();
        listed.sort_unstable();
        let actual: Vec<usize> = sim.board.points().filter(|&i| sim.board.cell(i) == Cell::Empty).collect();
        assert_eq!(listed, actual);
        for (at, &index) in sim.empties.iter().enumerate() {
            assert_eq!(sim.slot[index as usize] as usize, at);
        }
    }

    #[test]
    fn playouts_keep_the_board_consistent() {
        let mut rng = Rng::from_seed(5);
        for size in [2, 5, 9, 13] {
            let game = Game::new(size, 7.5).unwrap();
            for _ in 0..20 {
                let mut sim = Sim::new(&game, Color::Black);
                // 同形反復を直前のコウでしか見ないので、小さい盤では取り合いが続くことがある。
                // そのため playout と同じく手数の上限で打ち切る。
                let mut moves = 0;
                while !sim.is_over() && moves < size * size * 3 {
                    sim.random_step(&mut rng);
                    moves += 1;
                }
                assert_eq!(sim.board.hash(), sim.board.compute_hash());
                for index in sim.board.points() {
                    if let Some(group) = sim.board.group(index) {
                        assert!(group.liberties > 0);
                    }
                }
            }
        }
    }

    #[test]
    fn reward_prefers_wins_and_then_larger_margins() {
        let area = 81.0;
        // 勝ちは負けより必ず高い。
        assert!(reward(0.5, Color::Black, area) > reward(-0.5, Color::Black, area) + 0.8);
        assert!(reward(-0.5, Color::White, area) > reward(0.5, Color::White, area) + 0.8);
        // 同じ勝ちなら、大きく勝つほうが高い。同じ負けなら、小さく負けるほうが高い。
        assert!(reward(20.5, Color::Black, area) > reward(0.5, Color::Black, area));
        assert!(reward(-0.5, Color::Black, area) > reward(-20.5, Color::Black, area));
        // 値は 0〜1 に収まる。
        for margin in [-200.0, -81.0, -0.5, 0.5, 81.0, 200.0] {
            for color in Color::BOTH {
                assert!((0.0..=1.0).contains(&reward(margin, color, area)));
            }
        }
        assert_eq!(reward(81.0, Color::Black, area), 1.0);
        assert_eq!(reward(81.0, Color::White, area), 0.0);
    }

    #[test]
    fn captures_stones_in_atari() {
        // 白 3 子がアタリ。取れば黒の勝ち、取らなければ白に逃げられる。
        let game = game_from(
            ". . . . . . .
             . . X X X . .
             . X O O O . .
             . . X X X . .
             . . . . . . .
             . . . . . . .
             . . . . . . .",
            7.5,
        );
        let mut rng = Rng::from_seed(11);
        let result = search(&game, Color::Black, &fixed(4000), &mut rng);
        assert_eq!(result.mv, Move::Play(Coord::new(5, 2)));
    }

    #[test]
    fn only_returns_legal_moves_and_respects_superko() {
        // コウを取られた直後の白。取り返しは同形反復なので、候補に入らない。
        let mut game = game_from(
            ". X O . .
             X O . O .
             . X O . .
             . . . . .
             . . . . .",
            7.5,
        );
        game.play_as(Color::Black, Move::Play(Coord::new(2, 1))).unwrap();
        let mut rng = Rng::from_seed(13);
        for _ in 0..20 {
            let result = search(&game, Color::White, &fixed(200), &mut rng);
            assert_ne!(result.mv, Move::Play(Coord::new(1, 1)));
            assert!(game.is_legal(Color::White, result.mv));
        }
    }

    #[test]
    fn passes_when_no_move_remains() {
        let game = game_from(
            ". X . X X
             X X X X X
             X X X X X
             X X X X X
             X X X X X",
            7.5,
        );
        let mut rng = Rng::from_seed(14);
        assert_eq!(search(&game, Color::Black, &fixed(100), &mut rng).mv, Move::Pass);
        assert_eq!(search(&game, Color::White, &fixed(100), &mut rng).mv, Move::Pass);
    }

    #[test]
    fn does_not_pass_while_dead_stones_remain() {
        // 黒の囲いの中に、取れる白石(死石)が 2 子残っている。相手はパスした。
        // このまま終局すると、白石は生き石として数えられ、周りの空点も黒地にならないので黒の負け。
        // 取り切れば黒の大勝なので、パスしてはいけない。
        let mut game = game_from(
            ". . . . X O .
             . O . . X O .
             . O . . X O .
             . . . . X O .
             . . . . X O .
             . . . . X O .
             . . . . X O .",
            0.5,
        );
        game.play_as(Color::White, Move::Pass).unwrap();
        assert_eq!(winner_on(game.board(), 0.5), Color::White);
        let mut rng = Rng::from_seed(15);
        for _ in 0..5 {
            let result = search(&game, Color::Black, &fixed(3000), &mut rng);
            assert_ne!(result.mv, Move::Pass, "must keep playing to capture the dead stones");
        }
    }

    #[test]
    fn passes_to_win_when_the_opponent_has_passed() {
        // 相手がパスしていて、このまま数えて勝ちなら、パスして終局させる。
        let mut game = game_from(
            ". . X O . .
             . . X O . .
             . . X O . .
             . . X O . .
             . . X O . .
             . . X O . .",
            0.5,
        );
        game.play_as(Color::Black, Move::Pass).unwrap();
        let mut rng = Rng::from_seed(16);
        // 白はコミで 0.5 目勝っている。
        let result = search(&game, Color::White, &fixed(500), &mut rng);
        assert_eq!(result.mv, Move::Pass);
        assert_eq!(result.playouts, 0);
    }

    #[test]
    fn respects_playout_and_time_limits() {
        let game = Game::new(9, 7.5).unwrap();
        let mut rng = Rng::from_seed(17);
        let result = search(&game, Color::Black, &fixed(300), &mut rng);
        assert_eq!(result.playouts, 300);

        let config = MctsConfig { playouts: None, time: Some(Duration::from_millis(150)), ..MctsConfig::default() };
        let started = Instant::now();
        let result = search(&game, Color::Black, &config, &mut rng);
        let elapsed = started.elapsed();
        assert!(result.playouts > 0);
        assert!(elapsed >= Duration::from_millis(140), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
    }

    /// ランダム版に大きく勝ち越す(少ないプレイアウトでの簡易確認。本格的な対戦は gtp-match で行う)。
    #[test]
    fn beats_the_random_engine() {
        let mut rng = Rng::from_seed(18);
        let config = fixed(400);
        let mut wins = 0;
        let games = 8;
        for round in 0..games {
            let mcts_color = if round % 2 == 0 { Color::Black } else { Color::White };
            let mut game = Game::new(7, 7.5).unwrap();
            while !game.is_over() && game.move_number() < 400 {
                let color = game.to_move();
                let mv = if color == mcts_color {
                    search(&game, color, &config, &mut rng).mv
                } else {
                    random_move(&game, color, &mut rng)
                };
                game.play(mv).expect("engines must play legal moves");
            }
            if game.result_by_score().winner() == Some(mcts_color) {
                wins += 1;
            }
        }
        assert!(wins >= games - 1, "MCTS won only {wins} of {games} games against random play");
    }
}
