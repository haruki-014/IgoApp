//! 設定。すべて環境変数から読む。

use std::net::SocketAddr;
use std::time::Duration;

use serde::Deserialize;

use crate::model::HUMAN;

/// 思考エンジン 1 つぶんの設定。GTP を TCP で話す相手を指す。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSpec {
    /// 対局作成時に指定する名前。DB の black_player / white_player にも入る。
    pub name: String,
    /// 画面に表示する名前。省略時は `name`。
    #[serde(default)]
    pub label: Option<String>,
    pub host: String,
    pub port: u16,
    /// このエンジンが同時に思考できる数。
    #[serde(default = "default_engine_concurrency")]
    pub max_concurrent: usize,
    /// このエンジンが対応する最大の盤サイズ。
    #[serde(default = "default_max_board_size")]
    pub max_board_size: usize,
}

fn default_engine_concurrency() -> usize {
    1
}

fn default_max_board_size() -> usize {
    go_core::MAX_SIZE
}

impl EngineSpec {
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub engines: Vec<EngineSpec>,
    /// 全エンジン合計で、同時に思考させる数の上限。
    pub ai_max_concurrent: usize,
    /// GTP コマンド 1 つ(genmove を含む)の応答を待つ時間。
    pub ai_move_timeout: Duration,
    /// AI の 1 手にかける最短時間。一瞬で返るエンジンでも観戦しやすくするため。
    pub ai_min_move: Duration,
    /// 使われていないエンジン接続を切るまでの時間。
    pub engine_idle: Duration,
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn env_parse<T: std::str::FromStr>(name: &str, default: &str) -> Result<T, String> {
    let raw = env_or(name, default);
    raw.trim().parse().map_err(|_| format!("環境変数 {name} の値が不正です: {raw}"))
}

/// エンジン一覧の JSON を読み、名前の重複などを検査する。
pub fn parse_engines(json: &str) -> Result<Vec<EngineSpec>, String> {
    let engines: Vec<EngineSpec> =
        serde_json::from_str(json).map_err(|e| format!("エンジン一覧の JSON を読めません: {e}"))?;
    let mut names = std::collections::HashSet::new();
    for engine in &engines {
        let valid_name = !engine.name.is_empty()
            && engine.name.len() <= 32
            && engine.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
        if !valid_name || engine.name == HUMAN {
            return Err(format!(
                "エンジン名が不正です: {:?}(英小文字・数字・-・_ の 32 文字以内。human は使えません)",
                engine.name
            ));
        }
        if !names.insert(engine.name.clone()) {
            return Err(format!("エンジン名が重複しています: {}", engine.name));
        }
        if engine.max_concurrent == 0 {
            return Err(format!("{}: max_concurrent は 1 以上にしてください", engine.name));
        }
        if !go_core::is_valid_size(engine.max_board_size) {
            return Err(format!("{}: max_board_size は 2〜25 にしてください", engine.name));
        }
    }
    Ok(engines)
}

impl Config {
    /// 環境変数から設定を読む。
    ///
    /// - `DATABASE_URL`(必須)
    /// - `BIND_ADDR`(既定 `0.0.0.0:8080`)
    /// - `ENGINES`:エンジン一覧の JSON。または `ENGINES_FILE`:その JSON を書いたファイルのパス。
    /// - `AI_MAX_CONCURRENT`(既定 2)、`AI_MOVE_TIMEOUT_SECS`(既定 60)、
    ///   `AI_MIN_MOVE_MS`(既定 400)、`ENGINE_IDLE_SECS`(既定 600)
    pub fn from_env() -> Result<Config, String> {
        let database_url =
            std::env::var("DATABASE_URL").map_err(|_| "環境変数 DATABASE_URL が設定されていません".to_string())?;

        let engines_json = match std::env::var("ENGINES_FILE").ok().filter(|p| !p.trim().is_empty()) {
            Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("ENGINES_FILE {path} を読めません: {e}"))?,
            None => env_or("ENGINES", "[]"),
        };

        let ai_max_concurrent: usize = env_parse("AI_MAX_CONCURRENT", "2")?;
        if ai_max_concurrent == 0 {
            return Err("AI_MAX_CONCURRENT は 1 以上にしてください".to_string());
        }

        Ok(Config {
            database_url,
            bind_addr: env_parse("BIND_ADDR", "0.0.0.0:8080")?,
            engines: parse_engines(&engines_json)?,
            ai_max_concurrent,
            ai_move_timeout: Duration::from_secs(env_parse("AI_MOVE_TIMEOUT_SECS", "60")?),
            ai_min_move: Duration::from_millis(env_parse("AI_MIN_MOVE_MS", "400")?),
            engine_idle: Duration::from_secs(env_parse("ENGINE_IDLE_SECS", "600")?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_engine_list() {
        let engines = parse_engines(
            r#"[
                {"name": "self", "label": "自作エンジン", "host": "engine-self", "port": 7001, "max_concurrent": 4},
                {"name": "katago", "host": "engine-katago", "port": 7002, "max_board_size": 19}
            ]"#,
        )
        .unwrap();
        assert_eq!(engines.len(), 2);
        assert_eq!(engines[0].label(), "自作エンジン");
        assert_eq!(engines[0].max_concurrent, 4);
        assert_eq!(engines[0].max_board_size, 25);
        assert_eq!(engines[1].label(), "katago");
        assert_eq!(engines[1].max_concurrent, 1);
        assert_eq!(engines[1].max_board_size, 19);
        assert!(parse_engines("[]").unwrap().is_empty());
    }

    #[test]
    fn rejects_bad_engine_lists() {
        let bad = [
            r#"{"name": "x"}"#,
            r#"[{"name": "human", "host": "h", "port": 1}]"#,
            r#"[{"name": "Bad Name", "host": "h", "port": 1}]"#,
            r#"[{"name": "", "host": "h", "port": 1}]"#,
            r#"[{"name": "a", "host": "h", "port": 1}, {"name": "a", "host": "h", "port": 2}]"#,
            r#"[{"name": "a", "host": "h", "port": 1, "max_concurrent": 0}]"#,
            r#"[{"name": "a", "host": "h", "port": 1, "max_board_size": 26}]"#,
            r#"[{"name": "a", "host": "h", "port": 1, "unknown": true}]"#,
            r#"[{"name": "a", "host": "h"}]"#,
        ];
        for json in bad {
            assert!(parse_engines(json).is_err(), "{json}");
        }
    }
}
