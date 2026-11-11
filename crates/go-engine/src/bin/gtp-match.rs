//! 2 つの GTP エンジンを対戦させる(強さの確認用)。
//!
//! エンジンは子プロセスとして起動し、標準入出力で GTP を話す。審判は go-core が務め、
//! 不正手を返したエンジンはその対局を負けとする。先後は 1 局ごとに入れ替える。
//!
//! 例:
//!   cargo run --release -p go-engine --bin gtp-match -- \
//!     --a "target/release/go-engine --mode mcts --time-ms 1000" \
//!     --b "target/release/go-engine --mode random" \
//!     --games 20 --size 9

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio};

use go_core::{Color, Game, Move};

const USAGE: &str = "\
使い方:
  gtp-match --a \"<エンジン A のコマンド>\" --b \"<エンジン B のコマンド>\" [オプション]

オプション:
  --games <局数>   対局数(既定: 20)
  --size <路数>    盤サイズ(既定: 9)
  --komi <コミ>    コミ(既定: 7.5)
  --quiet          1 局ごとの結果を表示しない
";

struct Player {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Player {
    fn start(command: &str) -> Result<Player, String> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or("エンジンのコマンドが空です")?;
        let mut child = Command::new(program)
            .args(parts)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{command} を起動できません: {e}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        Ok(Player { child, stdin, stdout })
    }

    /// コマンドを送り、応答の本文を返す。エンジンがエラー(`?`)を返したら `Err`。
    fn send(&mut self, command: &str) -> Result<String, String> {
        writeln!(self.stdin, "{command}").and_then(|_| self.stdin.flush()).map_err(|e| e.to_string())?;
        let mut body = String::new();
        let mut started = false;
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("エンジンが終了しました".to_string());
            }
            let line = line.trim_end();
            if !started {
                if line.is_empty() {
                    continue;
                }
                started = true;
                body.push_str(line);
            } else if line.is_empty() {
                break;
            } else {
                body.push('\n');
                body.push_str(line);
            }
        }
        match body.strip_prefix('=') {
            Some(text) => Ok(text.trim().to_string()),
            None => Err(format!("{command}: {body}")),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
    }
}

struct Outcome {
    winner: Color,
    result: String,
    moves: usize,
}

/// 1 局打つ。`players[0]` が黒、`players[1]` が白。
fn play_game(players: &mut [&mut Player; 2], size: usize, komi: f64) -> Result<Outcome, String> {
    for player in players.iter_mut() {
        player.send(&format!("boardsize {size}"))?;
        player.send(&format!("komi {komi}"))?;
        player.send("clear_board")?;
    }
    let mut game = Game::new(size, komi).map_err(|e| e.to_string())?;
    // 取り合いが延々と続く場合に備えた上限。
    let limit = size * size * 6;
    while !game.is_over() && game.move_number() < limit {
        let color = game.to_move();
        let (mover, other) = (color.index(), color.opponent().index());
        let reply = players[mover].send(&format!("genmove {color}"))?;
        if reply.eq_ignore_ascii_case("resign") {
            return Ok(Outcome { winner: color.opponent(), result: format!("{}+R", color.opponent()), moves: game.move_number() });
        }
        let legal = Move::from_gtp(&reply, size).ok().filter(|&mv| game.play_as(color, mv).is_ok());
        let Some(mv) = legal else {
            eprintln!("  {color} が不正手 {reply:?} を返しました。{color} の負けとします");
            return Ok(Outcome {
                winner: color.opponent(),
                result: format!("{}+R(不正手)", color.opponent()),
                moves: game.move_number(),
            });
        };
        players[other].send(&format!("play {color} {}", mv.to_gtp(size)))?;
    }
    let result = game.result_by_score();
    let winner = result.winner().unwrap_or(Color::White);
    Ok(Outcome { winner, result: result.to_string(), moves: game.move_number() })
}

struct Args {
    a: String,
    b: String,
    games: usize,
    size: usize,
    komi: f64,
    quiet: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { a: String::new(), b: String::new(), games: 20, size: 9, komi: 7.5, quiet: false };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or(format!("{name} には値が必要です"));
        match arg.as_str() {
            "--a" => args.a = value("--a")?,
            "--b" => args.b = value("--b")?,
            "--games" => args.games = value("--games")?.parse().map_err(|_| "--games の値が不正です")?,
            "--size" => args.size = value("--size")?.parse().map_err(|_| "--size の値が不正です")?,
            "--komi" => args.komi = value("--komi")?.parse().map_err(|_| "--komi の値が不正です")?,
            "--quiet" => args.quiet = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("不明な引数です: {other}")),
        }
    }
    if args.a.is_empty() || args.b.is_empty() {
        return Err("--a と --b の両方を指定してください".to_string());
    }
    Ok(args)
}

fn run(args: &Args) -> Result<[usize; 2], String> {
    let mut a = Player::start(&args.a)?;
    let mut b = Player::start(&args.b)?;
    println!("A: {} ({})", a.send("name")?, args.a);
    println!("B: {} ({})", b.send("name")?, args.b);
    println!("{} 路盤、コミ {}、{} 局(先後は 1 局ごとに交替)\n", args.size, args.komi, args.games);

    // wins[0] = A の勝ち数、wins[1] = B の勝ち数。
    let mut wins = [0usize; 2];
    for round in 0..args.games {
        let a_is_black = round % 2 == 0;
        let outcome = if a_is_black {
            play_game(&mut [&mut a, &mut b], args.size, args.komi)?
        } else {
            play_game(&mut [&mut b, &mut a], args.size, args.komi)?
        };
        let a_won = (outcome.winner == Color::Black) == a_is_black;
        wins[if a_won { 0 } else { 1 }] += 1;
        if !args.quiet {
            println!(
                "第 {:>2} 局: 黒 {} / 白 {}  {:<8} {:>3} 手  → {} の勝ち(通算 A {} - B {})",
                round + 1,
                if a_is_black { "A" } else { "B" },
                if a_is_black { "B" } else { "A" },
                outcome.result,
                outcome.moves,
                if a_won { "A" } else { "B" },
                wins[0],
                wins[1],
            );
        }
    }
    Ok(wins)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}\n");
            }
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(wins) => {
            let total = wins[0] + wins[1];
            println!(
                "\n結果: A {} 勝 - B {} 勝(A の勝率 {:.0}%)",
                wins[0],
                wins[1],
                if total > 0 { wins[0] as f64 * 100.0 / total as f64 } else { 0.0 }
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("対戦を続けられません: {message}");
            ExitCode::FAILURE
        }
    }
}
