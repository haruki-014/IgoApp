//! GTP(Go Text Protocol)バージョン 2 のコマンド処理。
//!
//! 入出力そのもの(標準入出力)は `main.rs` が受け持ち、ここは 1 行を受け取って
//! 応答の文字列を返すだけにしてある。

use go_core::{Color, Game, Move};

use crate::mcts::{self, MctsConfig};
use crate::random::random_move;
use crate::rng::Rng;

pub const ENGINE_NAME: &str = "igo-engine";
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_SIZE: usize = 19;
const DEFAULT_KOMI: f64 = 7.5;

/// 対応している GTP コマンド。
pub const COMMANDS: &[&str] = &[
    "protocol_version",
    "name",
    "version",
    "known_command",
    "list_commands",
    "quit",
    "boardsize",
    "clear_board",
    "komi",
    "play",
    "genmove",
    "undo",
    "showboard",
    "final_score",
];

/// 思考方法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// 段階 1:自分の眼を埋めないランダム選択。
    Random,
    /// 段階 2:MCTS(UCT)。
    Mcts,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    /// 乱数のシード。`None` なら時刻から決める。
    pub seed: Option<u64>,
    /// MCTS の設定(`mode` が `Mcts` のときに使う)。
    pub mcts: MctsConfig,
    /// 思考の経過(プレイアウト数と勝率)を標準エラー出力に書く。
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config { mode: Mode::Mcts, seed: None, mcts: MctsConfig::default(), verbose: false }
    }
}

impl Config {
    /// 段階 1(ランダム選択)の設定。
    pub fn random(seed: Option<u64>) -> Config {
        Config { mode: Mode::Random, seed, ..Config::default() }
    }
}

/// 1 コマンドに対する応答。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    /// 末尾の空行まで含む、そのまま書き出せる応答。
    pub text: String,
    /// `quit` を受け取った。応答を書き出したら終了する。
    pub quit: bool,
}

pub struct Engine {
    config: Config,
    game: Game,
    rng: Rng,
}

impl Engine {
    pub fn new(config: Config) -> Engine {
        let rng = match config.seed {
            Some(seed) => Rng::from_seed(seed),
            None => Rng::from_entropy(),
        };
        Engine {
            config,
            game: Game::new(DEFAULT_SIZE, DEFAULT_KOMI).expect("default size is valid"),
            rng,
        }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    /// 入力 1 行を処理する。空行やコメントだけの行は `None`(応答なし)。
    pub fn handle_line(&mut self, line: &str) -> Option<Reply> {
        // GTP の前処理:コメントを捨て、タブを空白にし、その他の制御文字を取り除く。
        let line = line.split('#').next().unwrap_or("");
        let cleaned: String = line
            .chars()
            .filter_map(|c| match c {
                '\t' => Some(' '),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect();
        let mut tokens = cleaned.split_whitespace();
        let mut first = tokens.next()?;

        // 先頭が数字なら、それはコマンドの ID。応答にそのまま付けて返す。
        let mut id = String::new();
        if first.bytes().all(|b| b.is_ascii_digit()) {
            id = first.to_string();
            match tokens.next() {
                Some(name) => first = name,
                None => {
                    return Some(Reply { text: format!("?{id} missing command\n\n"), quit: false });
                }
            }
        }
        let name = first.to_ascii_lowercase();
        let args: Vec<&str> = tokens.collect();

        let quit = name == "quit";
        let text = match self.execute(&name, &args) {
            Ok(body) if body.is_empty() => format!("={id}\n\n"),
            Ok(body) => format!("={id} {body}\n\n"),
            Err(message) => format!("?{id} {message}\n\n"),
        };
        Some(Reply { text, quit })
    }

    fn execute(&mut self, name: &str, args: &[&str]) -> Result<String, String> {
        match name {
            "protocol_version" => Ok("2".to_string()),
            "name" => Ok(ENGINE_NAME.to_string()),
            "version" => Ok(ENGINE_VERSION.to_string()),
            "known_command" => {
                let known = args.first().is_some_and(|c| COMMANDS.contains(&c.to_ascii_lowercase().as_str()));
                Ok(known.to_string())
            }
            "list_commands" => Ok(COMMANDS.join("\n")),
            "quit" => Ok(String::new()),
            "boardsize" => {
                let size: usize = args
                    .first()
                    .and_then(|s| s.parse().ok())
                    .ok_or("boardsize not an integer")?;
                let komi = self.game.komi();
                // GTP の規定どおり、対応していないサイズには "unacceptable size" を返す。
                self.game = Game::new(size, komi).map_err(|_| "unacceptable size")?;
                Ok(String::new())
            }
            "clear_board" => {
                self.game = Game::new(self.game.size(), self.game.komi()).expect("current size is valid");
                Ok(String::new())
            }
            "komi" => {
                let komi: f64 = args
                    .first()
                    .and_then(|s| s.parse().ok())
                    .filter(|k: &f64| k.is_finite())
                    .ok_or("komi not a float")?;
                self.game.set_komi(komi);
                Ok(String::new())
            }
            "play" => {
                let (color, vertex) = match args {
                    [color, vertex, ..] => (color, vertex),
                    _ => return Err("invalid color or coordinate".to_string()),
                };
                let color = Color::parse(color).ok_or("invalid color or coordinate")?;
                let mv = Move::from_gtp(vertex, self.game.size()).map_err(|_| "invalid color or coordinate")?;
                self.game.play_as(color, mv).map_err(|_| "illegal move")?;
                Ok(String::new())
            }
            "genmove" => {
                let color = args.first().and_then(|c| Color::parse(c)).ok_or("invalid color")?;
                let mv = self.choose_move(color);
                self.game
                    .play_as(color, mv)
                    .map_err(|e| format!("internal error: generated an illegal move ({e})"))?;
                Ok(mv.to_gtp(self.game.size()))
            }
            "undo" => {
                self.game.undo(1).map_err(|_| "cannot undo")?;
                Ok(String::new())
            }
            "showboard" => {
                let game = &self.game;
                Ok(format!(
                    "\n{}\nnext: {}  move: {}  komi: {}  captures: B {} / W {}",
                    game.board(),
                    game.to_move(),
                    game.move_number(),
                    game.komi(),
                    game.captures(Color::Black),
                    game.captures(Color::White),
                ))
            }
            "final_score" => Ok(match self.game.result_by_score() {
                go_core::GameResult::Draw => "0".to_string(),
                result => result.to_string(),
            }),
            _ => Err("unknown command".to_string()),
        }
    }

    fn choose_move(&mut self, color: Color) -> Move {
        match self.config.mode {
            Mode::Random => random_move(&self.game, color, &mut self.rng),
            Mode::Mcts => {
                let result = mcts::search(&self.game, color, &self.config.mcts, &mut self.rng);
                if self.config.verbose {
                    eprintln!(
                        "genmove {color}: {} playouts={} winrate={:.3}",
                        result.mv.to_gtp(self.game.size()),
                        result.playouts,
                        result.winrate
                    );
                }
                result.mv
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use go_core::{Cell, Coord};

    fn engine() -> Engine {
        Engine::new(Config::random(Some(1)))
    }

    fn send(engine: &mut Engine, line: &str) -> String {
        engine.handle_line(line).expect("a response").text
    }

    #[test]
    fn administrative_commands() {
        let mut e = engine();
        assert_eq!(send(&mut e, "protocol_version"), "= 2\n\n");
        assert_eq!(send(&mut e, "name"), "= igo-engine\n\n");
        assert_eq!(send(&mut e, "version"), format!("= {ENGINE_VERSION}\n\n"));
        assert_eq!(send(&mut e, "known_command genmove"), "= true\n\n");
        assert_eq!(send(&mut e, "known_command nosuch"), "= false\n\n");
        assert_eq!(send(&mut e, "known_command"), "= false\n\n");
        let list = send(&mut e, "list_commands");
        for command in [
            "protocol_version", "name", "version", "known_command", "list_commands", "quit",
            "boardsize", "clear_board", "komi", "play", "genmove", "undo", "showboard",
        ] {
            assert!(list.lines().any(|l| l.trim_start_matches("= ") == command), "{command} missing");
            assert_eq!(send(&mut e, &format!("known_command {command}")), "= true\n\n");
        }
        assert!(list.ends_with("\n\n"));
    }

    #[test]
    fn ids_comments_and_blank_lines() {
        let mut e = engine();
        assert_eq!(send(&mut e, "12 name"), "=12 igo-engine\n\n");
        assert_eq!(send(&mut e, "7 nosuch"), "?7 unknown command\n\n");
        assert_eq!(send(&mut e, "nosuch"), "? unknown command\n\n");
        assert_eq!(send(&mut e, "  NAME  # comment"), "= igo-engine\n\n");
        assert_eq!(send(&mut e, "3\tboardsize\t9\r"), "=3\n\n");
        assert_eq!(e.game().size(), 9);
        assert_eq!(e.handle_line(""), None);
        assert_eq!(e.handle_line("   "), None);
        assert_eq!(e.handle_line("# only a comment"), None);
        assert_eq!(send(&mut e, "5"), "?5 missing command\n\n");
    }

    #[test]
    fn quit_sets_the_flag() {
        let mut e = engine();
        let reply = e.handle_line("quit").unwrap();
        assert_eq!(reply, Reply { text: "=\n\n".to_string(), quit: true });
        assert!(!e.handle_line("name").unwrap().quit);
    }

    #[test]
    fn setup_commands() {
        let mut e = engine();
        assert_eq!(e.game().size(), 19);
        assert_eq!(send(&mut e, "boardsize 9"), "=\n\n");
        assert_eq!(send(&mut e, "komi 6.5"), "=\n\n");
        assert_eq!(e.game().komi(), 6.5);
        assert_eq!(send(&mut e, "boardsize 1"), "? unacceptable size\n\n");
        assert_eq!(send(&mut e, "boardsize 26"), "? unacceptable size\n\n");
        assert_eq!(send(&mut e, "boardsize x"), "? boardsize not an integer\n\n");
        assert_eq!(send(&mut e, "boardsize"), "? boardsize not an integer\n\n");
        assert_eq!(send(&mut e, "komi abc"), "? komi not a float\n\n");
        assert_eq!(send(&mut e, "komi nan"), "? komi not a float\n\n");
        // 失敗したコマンドは状態を変えない。
        assert_eq!(e.game().size(), 9);
        assert_eq!(e.game().komi(), 6.5);

        // boardsize と clear_board は盤を空にし、コミは保つ。
        send(&mut e, "play b E5");
        assert_eq!(send(&mut e, "clear_board"), "=\n\n");
        assert_eq!(e.game().move_number(), 0);
        assert_eq!(e.game().komi(), 6.5);
        send(&mut e, "play b E5");
        assert_eq!(send(&mut e, "boardsize 13"), "=\n\n");
        assert_eq!(e.game().move_number(), 0);
        assert_eq!(e.game().size(), 13);
        assert_eq!(e.game().komi(), 6.5);
        assert_eq!(send(&mut e, "boardsize 25"), "=\n\n");
        assert_eq!(send(&mut e, "play w Z25"), "=\n\n");
        assert_eq!(send(&mut e, "boardsize 2"), "=\n\n");
    }

    #[test]
    fn play_and_undo() {
        let mut e = engine();
        send(&mut e, "boardsize 9");
        assert_eq!(send(&mut e, "play black E5"), "=\n\n");
        assert_eq!(send(&mut e, "play W c3"), "=\n\n");
        assert_eq!(e.game().board().at(Coord::new(4, 4)), Cell::Black);
        assert_eq!(e.game().board().at(Coord::new(2, 6)), Cell::White);
        assert_eq!(send(&mut e, "play b pass"), "=\n\n");
        assert_eq!(e.game().move_number(), 3);

        assert_eq!(send(&mut e, "play b E5"), "? illegal move\n\n");
        assert_eq!(send(&mut e, "play b K1"), "? invalid color or coordinate\n\n");
        assert_eq!(send(&mut e, "play b I5"), "? invalid color or coordinate\n\n");
        assert_eq!(send(&mut e, "play x E5"), "? invalid color or coordinate\n\n");
        assert_eq!(send(&mut e, "play b"), "? invalid color or coordinate\n\n");
        assert_eq!(e.game().move_number(), 3);

        assert_eq!(send(&mut e, "undo"), "=\n\n");
        assert_eq!(send(&mut e, "undo"), "=\n\n");
        assert_eq!(e.game().board().at(Coord::new(2, 6)), Cell::Empty);
        assert_eq!(send(&mut e, "undo"), "=\n\n");
        assert_eq!(send(&mut e, "undo"), "? cannot undo\n\n");
    }

    #[test]
    fn play_enforces_ko_and_suicide() {
        let mut e = engine();
        send(&mut e, "boardsize 5");
        // コウの形を作る(go-core のテストと同じ形)。
        for mv in ["b B5", "b A4", "b B3", "w C5", "w D4", "w C3", "w B4"] {
            assert_eq!(send(&mut e, &format!("play {mv}")), "=\n\n", "{mv}");
        }
        assert_eq!(send(&mut e, "play b C4"), "=\n\n");
        assert_eq!(e.game().captures(Color::Black), 1);
        assert_eq!(send(&mut e, "play w B4"), "? illegal move\n\n");
        // 自殺手。
        send(&mut e, "clear_board");
        send(&mut e, "play b A2");
        send(&mut e, "play b B1");
        assert_eq!(send(&mut e, "play w A1"), "? illegal move\n\n");
    }

    #[test]
    fn genmove_returns_legal_moves_until_the_game_ends() {
        let mut e = engine();
        send(&mut e, "boardsize 9");
        let mut referee = Game::new(9, 7.5).unwrap();
        let mut color = Color::Black;
        while !referee.is_over() {
            let name = if color == Color::Black { "b" } else { "white" };
            let reply = send(&mut e, &format!("genmove {name}"));
            let vertex = reply.strip_prefix("= ").unwrap().trim();
            let mv = Move::from_gtp(vertex, 9).expect("vertex or pass");
            referee.play_as(color, mv).expect("engine move must be legal");
            assert_eq!(e.game().board(), referee.board());
            color = color.opponent();
            assert!(referee.move_number() < 2000);
        }
        assert_eq!(send(&mut e, "genmove purple"), "? invalid color\n\n");
        assert_eq!(send(&mut e, "genmove"), "? invalid color\n\n");
        let score = send(&mut e, "final_score");
        assert!(score.starts_with("= B+") || score.starts_with("= W+"), "{score}");
        assert_eq!(score.trim_start_matches("= ").trim(), referee.result_by_score().to_string());
    }

    #[test]
    fn genmove_with_mcts_returns_legal_moves() {
        let mut config = Config { seed: Some(3), ..Config::default() };
        assert_eq!(config.mode, Mode::Mcts);
        config.mcts = MctsConfig { playouts: Some(200), time: None, ..MctsConfig::default() };
        let mut e = Engine::new(config);
        send(&mut e, "boardsize 5");
        let mut referee = Game::new(5, 7.5).unwrap();
        let mut color = Color::Black;
        while !referee.is_over() && referee.move_number() < 200 {
            let reply = send(&mut e, &format!("genmove {color}"));
            let mv = Move::from_gtp(reply.strip_prefix("= ").unwrap().trim(), 5).expect("vertex or pass");
            referee.play_as(color, mv).expect("engine move must be legal");
            color = color.opponent();
        }
        assert!(referee.is_over(), "the game should end by two passes");
    }

    #[test]
    fn genmove_then_undo_restores_the_position() {
        let mut e = engine();
        send(&mut e, "boardsize 9");
        send(&mut e, "play b E5");
        let before = e.game().board().clone();
        send(&mut e, "genmove w");
        assert_eq!(e.game().move_number(), 2);
        send(&mut e, "undo");
        assert_eq!(e.game().board(), &before);
    }

    #[test]
    fn showboard_draws_the_position() {
        let mut e = engine();
        send(&mut e, "boardsize 5");
        send(&mut e, "play b A1");
        send(&mut e, "play w E5");
        let text = send(&mut e, "showboard");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "= ");
        assert_eq!(lines[1], "   A B C D E");
        assert_eq!(lines[2], " 5 . . . . O 5");
        assert_eq!(lines[6], " 1 X . . . . 1");
        assert_eq!(lines[7], "   A B C D E");
        assert!(lines[8].contains("next: B"));
        assert!(text.ends_with("\n\n"));
        // 応答の途中に空行があってはならない(空行は応答の終わりを意味する)。
        assert!(!text.trim_end().contains("\n\n"));
    }
}
