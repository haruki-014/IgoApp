use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

use go_engine::gtp::{Config, ENGINE_NAME, ENGINE_VERSION, Engine, Mode};

const USAGE: &str = "\
igo-engine: GTP を標準入出力で話す囲碁思考エンジン

使い方:
  go-engine [オプション]

オプション:
  --mode <mcts|random>  思考方法(既定: mcts)
                          mcts    モンテカルロ木探索(UCT)
                          random  自分の眼を埋めない合法手からランダムに選ぶ
  --time-ms <ミリ秒>    mcts の 1 手あたりの思考時間(既定: 2000)
  --playouts <回数>     mcts の 1 手あたりのプレイアウト回数の上限
                        (--time-ms と両方指定した場合は、先に達したほうで打ち切る。
                          --playouts だけを指定した場合は、時間では打ち切らない)
  --seed <整数>         乱数のシード(省略時は時刻から決める)
  --verbose             思考の経過を標準エラー出力に書く
  -h, --help            この説明を表示する
  -V, --version         バージョンを表示する
";

fn parse_args(args: &[String]) -> Result<Option<Config>, String> {
    let mut config = Config::default();
    let mut time_ms: Option<u64> = None;
    let mut playouts: Option<u32> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or(format!("{name} には値が必要です"));
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("{ENGINE_NAME} {ENGINE_VERSION}");
                return Ok(None);
            }
            "--mode" => {
                config.mode = match value("--mode")?.as_str() {
                    "random" => Mode::Random,
                    "mcts" => Mode::Mcts,
                    other => return Err(format!("未対応の mode です: {other}")),
                };
            }
            "--seed" => {
                let v = value("--seed")?;
                config.seed = Some(v.parse().map_err(|_| format!("--seed の値が整数ではありません: {v}"))?);
            }
            "--time-ms" => {
                let v = value("--time-ms")?;
                let ms: u64 = v.parse().map_err(|_| format!("--time-ms の値が整数ではありません: {v}"))?;
                if ms == 0 {
                    return Err("--time-ms は 1 以上にしてください".to_string());
                }
                time_ms = Some(ms);
            }
            "--playouts" => {
                let v = value("--playouts")?;
                let n: u32 = v.parse().map_err(|_| format!("--playouts の値が整数ではありません: {v}"))?;
                if n == 0 {
                    return Err("--playouts は 1 以上にしてください".to_string());
                }
                playouts = Some(n);
            }
            "--verbose" => config.verbose = true,
            other => return Err(format!("不明な引数です: {other}")),
        }
    }
    if time_ms.is_some() || playouts.is_some() {
        config.mcts.time = time_ms.map(Duration::from_millis);
        config.mcts.playouts = playouts;
    }
    Ok(Some(config))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(Some(config)) => config,
        Ok(None) => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let mut engine = Engine::new(config);
    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    // 入力が閉じられたら(EOF)終了する。TCP 接続につないだ場合は切断がこれにあたる。
    for line in stdin.lines() {
        let Ok(line) = line else { break };
        let Some(reply) = engine.handle_line(&line) else { continue };
        if stdout.write_all(reply.text.as_bytes()).and_then(|_| stdout.flush()).is_err() {
            break;
        }
        if reply.quit {
            break;
        }
    }
    ExitCode::SUCCESS
}
