//! 結合テスト用の道具。実物の PostgreSQL と、TCP で GTP を話す偽のエンジンを使う。
//!
//! 環境変数 `TEST_DATABASE_URL` が必要。設定されていなければテストは失敗する
//! (黙って飛ばすと、確認したつもりで確認できていない状態になるため)。

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use server::config::{Config, EngineSpec};
use server::manager::Manager;
use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

pub const WAIT: Duration = Duration::from_secs(10);

pub fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .expect("結合テストには TEST_DATABASE_URL(テスト用 PostgreSQL の URL)が必要です")
}

/// テストごとに重ならないエンジン名を作る(DB を共有するので、他のテストの対局と混ざらないようにする)。
pub fn unique_name(prefix: &str) -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &id[..12])
}

pub fn test_config(engines: Vec<EngineSpec>) -> Config {
    Config {
        database_url: database_url(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        engines,
        ai_max_concurrent: 8,
        ai_move_timeout: Duration::from_secs(5),
        ai_min_move: Duration::from_millis(0),
        engine_idle: Duration::from_secs(600),
    }
}

/// 起動したテスト用サーバー。
pub struct TestApp {
    pub addr: SocketAddr,
    pub manager: Arc<Manager>,
    pub pool: PgPool,
    server: JoinHandle<()>,
}

impl TestApp {
    pub async fn start(config: Config) -> TestApp {
        let pool = server::connect_db(&config.database_url).await.expect("connect to test DB");
        server::migrate(&pool).await.expect("migrate");
        let manager = server::build(config, pool.clone()).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = server::router(manager.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        TestApp { addr, manager, pool, server }
    }

    /// サーバーを止める(再起動のテスト用)。メモリ上の対局はすべて失われる。
    pub async fn stop(self) {
        self.server.abort();
        let _ = self.server.await;
        self.manager.unload_all().await;
        drop(self.manager);
        self.pool.close().await;
    }

    /// 素の HTTP/1.1 でリクエストを送り、(ステータス, ヘッダ, 本文) を返す。
    pub async fn http(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, String, String) {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let raw = String::from_utf8(raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").expect("HTTP response");
        let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, head.to_string(), body.to_string())
    }

    pub async fn get_json(&self, path: &str) -> (u16, Value) {
        let (status, _, body) = self.http("GET", path, None).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    pub async fn post_json(&self, path: &str, body: Value) -> (u16, Value) {
        let (status, _, body) = self.http("POST", path, Some(&body)).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    /// 対局を作成し、応答の JSON を返す。
    pub async fn create_game(&self, size: usize, black: &str, white: &str) -> Value {
        let (status, body) = self
            .post_json(
                "/api/games",
                json!({ "board_size": size, "komi": 7.5, "black": black, "white": white }),
            )
            .await;
        assert_eq!(status, 201, "create_game failed: {body}");
        body
    }

    pub async fn connect(&self, game_id: &str, token: Option<&str>) -> Client {
        let url = format!("ws://{}/ws/games/{game_id}", self.addr);
        let (socket, _) = tokio_tungstenite::connect_async(url).await.expect("websocket connect");
        let mut client = Client { socket, last_state: Value::Null };
        // 接続すると、まず現在の状態が届く。
        client.next_state().await;
        if let Some(token) = token {
            client.send(json!({ "type": "join", "token": token })).await;
            client.next_state().await;
        }
        client
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

/// WebSocket のクライアント(ブラウザの代わり)。
pub struct Client {
    socket: Socket,
    pub last_state: Value,
}

impl Client {
    pub async fn send(&mut self, message: Value) {
        self.socket.send(Message::text(message.to_string())).await.unwrap();
    }

    /// 次の JSON メッセージ(state または error)を受け取る。
    pub async fn next_message(&mut self) -> Value {
        loop {
            let message = tokio::time::timeout(WAIT, self.socket.next())
                .await
                .expect("timed out waiting for a websocket message")
                .expect("websocket closed")
                .expect("websocket error");
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(text.as_str()).unwrap();
                if value["type"] == "state" {
                    self.last_state = value.clone();
                }
                return value;
            }
        }
    }

    pub async fn next_state(&mut self) -> Value {
        loop {
            let message = self.next_message().await;
            if message["type"] == "state" {
                return message;
            }
        }
    }

    /// 条件を満たす state が届くまで待つ(すでに届いている最新の state も調べる)。
    pub async fn wait_for(&mut self, what: &str, condition: impl Fn(&Value) -> bool) -> Value {
        if condition(&self.last_state) {
            return self.last_state.clone();
        }
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let remaining = deadline
                .checked_duration_since(tokio::time::Instant::now())
                .unwrap_or_else(|| panic!("timed out waiting for: {what}; last state: {}", self.last_state));
            let state = tokio::time::timeout(remaining, self.next_state())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for: {what}; last state: {}", self.last_state));
            if condition(&state) {
                return state;
            }
        }
    }

    /// 操作を送り、error が返ることを確かめてエラーコードを返す。
    pub async fn expect_error(&mut self, message: Value) -> String {
        self.send(message).await;
        loop {
            let reply = self.next_message().await;
            if reply["type"] == "error" {
                assert!(reply["message"].as_str().is_some_and(|m| !m.is_empty()));
                return reply["code"].as_str().unwrap().to_string();
            }
        }
    }

    pub async fn play(&mut self, x: usize, y: usize) {
        self.send(json!({ "type": "play", "x": x, "y": y })).await;
    }

    /// 着手を送り、その手が反映された state を待つ。
    ///
    /// 状態の配信は最新のものだけが届けば十分なので、途中の状態は飛ばされることがある
    /// (AI がすぐ応手した場合など)。そのため「自分の色の最後の手がこの手であること」で判定する。
    pub async fn play_ok(&mut self, x: usize, y: usize) -> Value {
        let mine = json!({ "color": self.last_state["you"], "x": x, "y": y });
        self.play(x, y).await;
        self.wait_for("the move to be applied", |s| {
            let moves = s["moves"].as_array().unwrap();
            moves.iter().rev().find(|m| m["color"] == mine["color"]) == Some(&mine)
        })
        .await
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

// ───────────────────────── 偽のエンジン ─────────────────────────

/// 偽のエンジンの振る舞い。
#[derive(Clone, Copy, Debug)]
pub enum Behavior {
    /// 自作エンジン(ランダム版)そのまま。
    Normal,
    /// genmove に時間がかかる。
    Slow(Duration),
    /// genmove に常に A1 と答える(2 回目以降は不正手になる)。
    AlwaysA1,
    /// genmove に答えない(タイムアウトさせる)。
    Silent,
    /// genmove に答えた直後に接続を切る(毎回つなぎ直しが必要になる)。
    DropAfterGenmove,
    /// genmove に着手ではない文字列を返す。
    Garbage,
}

/// TCP で GTP を話す偽のエンジン。接続ごとに盤面を 1 つ持つ(本番の socat と同じ形)。
pub struct FakeEngine {
    pub name: String,
    pub port: u16,
    /// 受け取ったコマンド。(接続番号, コマンド)。
    pub log: Arc<Mutex<Vec<(usize, String)>>>,
    pub connections: Arc<AtomicUsize>,
    /// 同時に実行された genmove の最大数。
    pub max_thinking: Arc<AtomicUsize>,
    thinking: Arc<AtomicUsize>,
    listener: JoinHandle<()>,
}

impl FakeEngine {
    pub async fn start(behavior: Behavior) -> FakeEngine {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let thinking = Arc::new(AtomicUsize::new(0));
        let max_thinking = Arc::new(AtomicUsize::new(0));

        let (log2, connections2, thinking2, max2) =
            (log.clone(), connections.clone(), thinking.clone(), max_thinking.clone());
        let listener = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                let number = connections2.fetch_add(1, Ordering::SeqCst) + 1;
                let (log, thinking, max) = (log2.clone(), thinking2.clone(), max2.clone());
                tokio::spawn(serve_connection(stream, number, behavior, log, thinking, max));
            }
        });
        FakeEngine {
            name: unique_name("fake"),
            port,
            log,
            connections,
            max_thinking,
            thinking,
            listener,
        }
    }

    pub fn spec(&self) -> EngineSpec {
        EngineSpec {
            name: self.name.clone(),
            label: Some(format!("Fake {}", self.name)),
            host: "127.0.0.1".to_string(),
            port: self.port,
            max_concurrent: 8,
            max_board_size: 25,
        }
    }

    /// `connection` 番目(1 始まり)の接続で受け取ったコマンド。
    pub fn commands(&self, connection: usize) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| *n == connection)
            .map(|(_, c)| c.clone())
            .collect()
    }

    pub fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl Drop for FakeEngine {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

async fn serve_connection(
    stream: TcpStream,
    number: usize,
    behavior: Behavior,
    log: Arc<Mutex<Vec<(usize, String)>>>,
    thinking: Arc<AtomicUsize>,
    max_thinking: Arc<AtomicUsize>,
) {
    use go_engine::gtp::{Config, Engine, Mode};
    let mut engine = Engine::new(Config { mode: Mode::Random, seed: Some(number as u64) });
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut parts = line.split_whitespace();
        let id = parts.next().unwrap_or("").to_string();
        let command: String = parts.collect::<Vec<_>>().join(" ");
        log.lock().unwrap().push((number, command.clone()));

        let is_genmove = command.starts_with("genmove");
        let mut reply = None;
        if is_genmove {
            let now = thinking.fetch_add(1, Ordering::SeqCst) + 1;
            max_thinking.fetch_max(now, Ordering::SeqCst);
            match behavior {
                Behavior::Slow(delay) => tokio::time::sleep(delay).await,
                Behavior::AlwaysA1 => reply = Some(format!("={id} A1\n\n")),
                Behavior::Garbage => reply = Some(format!("={id} banana\n\n")),
                Behavior::Silent => {
                    thinking.fetch_sub(1, Ordering::SeqCst);
                    std::future::pending::<()>().await;
                }
                Behavior::Normal | Behavior::DropAfterGenmove => {}
            }
            thinking.fetch_sub(1, Ordering::SeqCst);
        }
        let reply = match reply {
            Some(reply) => reply,
            None => match engine.handle_line(&line) {
                Some(reply) => reply.text,
                None => continue,
            },
        };
        if writer.write_all(reply.as_bytes()).await.is_err() {
            break;
        }
        if command == "quit" || (is_genmove && matches!(behavior, Behavior::DropAfterGenmove)) {
            break;
        }
    }
}
