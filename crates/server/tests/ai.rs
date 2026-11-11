//! AI との対局:GTP の同期、待った、不正手・タイムアウトの扱い、同時実行数の制限、再起動後の再開。
//! エンジンは、TCP で GTP を話す偽物(中身は自作エンジンのランダム版)を使う。

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use serde_json::{Value, json};

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_else(|| panic!("{key} is not a string in {value}"))
}

/// 盤面から空点を 1 つ探す(左上から)。
fn first_empty(state: &Value) -> (usize, usize) {
    for (y, row) in state["board"].as_array().unwrap().iter().enumerate() {
        if let Some(x) = row.as_str().unwrap().find('.') {
            return (x, y);
        }
    }
    panic!("no empty point");
}

fn gtp_vertex(x: usize, y: usize, size: usize) -> String {
    go_core::Coord::new(x, y).to_gtp(size)
}

#[tokio::test]
async fn human_plays_against_engine_with_undo() {
    let engine = FakeEngine::start(Behavior::Normal).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;

    let (status, engines) = app.get_json("/api/engines").await;
    assert_eq!(status, 200);
    assert_eq!(engines[0]["name"], engine.name.as_str());
    assert_eq!(engines[0]["max_board_size"], 25);

    let created = app.create_game(9, "human", &engine.name).await;
    let id = str_of(&created, "id");
    assert_eq!(created["creator_color"], "black");
    assert_eq!(created["invite_token"], Value::Null);
    let mut black = app.connect(id, Some(str_of(&created, "token"))).await;
    assert_eq!(black.last_state["players"], json!({ "black": "human", "white": engine.name }));
    assert_eq!(black.last_state["undo_available"], false);
    assert_eq!(black.expect_error(json!({ "type": "undo" })).await, "nothing_to_undo");

    // 人間が打つと、AI が応手する。
    black.play_ok(4, 4).await;
    let state = black.wait_for("engine reply", |s| s["move_number"] == 2 && s["thinking"].is_null()).await;
    assert_eq!(state["last_move"]["color"], "white");
    assert_eq!(state["to_move"], "black");
    assert_eq!(state["undo_available"], true);
    // 対局開始時に boardsize・komi・clear_board を送り、人間の着手は play で通知する。
    assert_eq!(engine.commands(1), ["boardsize 9", "komi 7.5", "clear_board", "play B E5", "genmove W"]);

    // 2 手目以降は、エンジンがまだ知らない手だけを通知する。
    let (x, y) = first_empty(&state);
    black.play_ok(x, y).await;
    let state = black.wait_for("second engine reply", |s| s["move_number"] == 4 && s["thinking"].is_null()).await;
    let second = format!("play B {}", gtp_vertex(x, y, 9));
    assert_eq!(engine.commands(1)[5..], [second.clone(), "genmove W".to_string()]);

    // 待った:直前の人間の手まで戻し、そのあとの AI の応手も取り消す。
    let first_reply = state["moves"][1].clone();
    black.send(json!({ "type": "undo" })).await;
    let state = black.wait_for("undo", |s| s["move_number"] == 2).await;
    assert_eq!(state["to_move"], "black");
    assert_eq!(state["moves"].as_array().unwrap().len(), 2);
    assert_eq!(state["moves"][1], first_reply);
    assert_eq!(state["board"][y].as_str().unwrap().as_bytes()[x], b'.');
    let rows: (i64,) = sqlx::query_as("SELECT count(*) FROM moves WHERE game_id = $1::uuid")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(rows.0, 2, "undone moves must be removed from the DB");

    // 待ったのあとは、clear_board から着手履歴を再生して同期し直す。
    black.play_ok(x, y).await;
    black.wait_for("reply after undo", |s| s["move_number"] == 4 && s["thinking"].is_null()).await;
    let log = engine.commands(1);
    let reply_vertex = gtp_vertex(
        first_reply["x"].as_u64().unwrap() as usize,
        first_reply["y"].as_u64().unwrap() as usize,
        9,
    );
    assert_eq!(
        log[7..],
        [
            "boardsize 9".to_string(),
            "komi 7.5".to_string(),
            "clear_board".to_string(),
            "play B E5".to_string(),
            format!("play W {reply_vertex}"),
            second,
            "genmove W".to_string(),
        ]
    );
    // 対局ごとに接続を 1 本保持している。
    assert_eq!(engine.connection_count(), 1);

    // 最初まで戻す。
    black.send(json!({ "type": "undo" })).await;
    black.wait_for("undo", |s| s["move_number"] == 2).await;
    black.send(json!({ "type": "undo" })).await;
    let state = black.wait_for("undo to the start", |s| s["move_number"] == 0).await;
    assert_eq!(state["undo_available"], false);
    assert_eq!(state["captures"], json!({ "black": 0, "white": 0 }));
    assert_eq!(black.expect_error(json!({ "type": "undo" })).await, "nothing_to_undo");

    // 投了で終局し、エンジンとの接続は閉じられる。
    black.send(json!({ "type": "resign" })).await;
    let state = black.wait_for("resignation", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "W+R");
}

#[tokio::test]
async fn engine_moves_first_when_it_plays_black() {
    let engine = FakeEngine::start(Behavior::Normal).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    let created = app.create_game(9, &engine.name, "human").await;
    assert_eq!(created["creator_color"], "white");
    let id = str_of(&created, "id");
    let mut white = app.connect(id, Some(str_of(&created, "token"))).await;
    assert_eq!(white.last_state["you"], "white");

    let state = white.wait_for("engine's first move", |s| s["move_number"] == 1 && s["thinking"].is_null()).await;
    assert_eq!(state["last_move"]["color"], "black");
    assert_eq!(engine.commands(1), ["boardsize 9", "komi 7.5", "clear_board", "genmove B"]);
    // 人間はまだ打っていないので、待ったはできない。
    assert_eq!(white.expect_error(json!({ "type": "undo" })).await, "nothing_to_undo");

    let (x, y) = first_empty(&state);
    white.play_ok(x, y).await;
    white.wait_for("engine's second move", |s| s["move_number"] == 3 && s["thinking"].is_null()).await;
    // 待ったで、人間の手と AI の応手の 2 手が消える。
    white.send(json!({ "type": "undo" })).await;
    let state = white.wait_for("undo", |s| s["move_number"] == 1).await;
    assert_eq!(state["to_move"], "white");
}

#[tokio::test]
async fn undo_while_the_engine_is_thinking() {
    let engine = FakeEngine::start(Behavior::Slow(Duration::from_secs(30))).await;
    let mut config = test_config(vec![engine.spec()]);
    config.ai_move_timeout = Duration::from_secs(60);
    let app = TestApp::start(config).await;
    let created = app.create_game(9, "human", &engine.name).await;
    let mut black = app.connect(str_of(&created, "id"), Some(str_of(&created, "token"))).await;

    black.play_ok(4, 4).await;
    let state = black.wait_for("thinking", |s| s["thinking"] == "white").await;
    assert_eq!(state["move_number"], 1);
    // AI の思考中でも待ったができ、思考は打ち切られる。
    black.send(json!({ "type": "undo" })).await;
    let state = black.wait_for("undo", |s| s["move_number"] == 0).await;
    assert_eq!(state["thinking"], Value::Null);
    assert_eq!(state["to_move"], "black");
    // 思考中の投了もできる。
    black.play_ok(2, 2).await;
    black.wait_for("thinking", |s| s["thinking"] == "white").await;
    black.send(json!({ "type": "resign" })).await;
    let state = black.wait_for("resignation", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "W+R");
    assert_eq!(state["thinking"], Value::Null);
}

#[tokio::test]
async fn engines_play_each_other_to_the_end() {
    let engine = FakeEngine::start(Behavior::Normal).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    let created = app.create_game(5, &engine.name, &engine.name).await;
    assert_eq!(created["creator_color"], Value::Null);
    assert_eq!(created["token"], Value::Null);
    let id = str_of(&created, "id");

    // 観戦者として見る。
    let mut watcher = app.connect(id, None).await;
    let state = watcher.wait_for("game over", |s| s["status"] == "finished").await;
    let moves = state["moves"].as_array().unwrap();
    assert!(moves.len() >= 2);
    // ランダム版は投了しないので、連続パスで終わる。
    for m in &moves[moves.len() - 2..] {
        assert!(m["x"].is_null() && m["y"].is_null());
    }
    let result = str_of(&state, "result");
    assert!(result.starts_with("B+") || result.starts_with("W+"), "{result}");
    assert!(!result.ends_with("+R"));
    let score = &state["score"];
    let total = ["black_stones", "black_territory", "white_stones", "white_territory", "neutral"]
        .iter()
        .map(|k| score[*k].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(total, 25);
    assert_eq!(watcher.expect_error(json!({ "type": "pass" })).await, "not_a_player");

    // 席ごとに接続を 1 本ずつ使う。
    assert_eq!(engine.connection_count(), 2);
    let (status,): (String,) = sqlx::query_as("SELECT status FROM games WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "finished");
    let (_, _, sgf) = app.http("GET", &format!("/api/games/{id}/sgf"), None).await;
    assert!(sgf.contains(&format!("PB[Fake {}]", engine.name)));
    assert!(sgf.contains(&format!("RE[{result}]")));
}

#[tokio::test]
async fn illegal_engine_move_counts_as_resignation() {
    let engine = FakeEngine::start(Behavior::AlwaysA1).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    let created = app.create_game(5, "human", &engine.name).await;
    let mut black = app.connect(str_of(&created, "id"), Some(str_of(&created, "token"))).await;

    // 1 回目の A1 は合法。
    black.play_ok(2, 2).await;
    let state = black.wait_for("A1", |s| s["move_number"] == 2).await;
    assert_eq!(state["board"][4], "O....");
    // 2 回目の A1 は石のある点なので不正手。エンジン側の投了として扱う。
    black.play_ok(3, 3).await;
    let state = black.wait_for("forfeit", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "B+R");
    // 不正手は盤面に適用されない。
    assert_eq!(state["move_number"], 3);
    assert_eq!(state["thinking"], Value::Null);
}

#[tokio::test]
async fn unreadable_engine_reply_counts_as_resignation() {
    let engine = FakeEngine::start(Behavior::Garbage).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    // エンジンが黒。初手で着手として読めない応答を返す。
    let created = app.create_game(9, &engine.name, "human").await;
    let mut white = app.connect(str_of(&created, "id"), Some(str_of(&created, "token"))).await;
    let state = white.wait_for("forfeit", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "W+R");
    assert_eq!(state["move_number"], 0);
}

#[tokio::test]
async fn engine_timeout_counts_as_resignation() {
    let engine = FakeEngine::start(Behavior::Silent).await;
    let mut config = test_config(vec![engine.spec()]);
    config.ai_move_timeout = Duration::from_millis(300);
    let app = TestApp::start(config).await;
    let created = app.create_game(9, "human", &engine.name).await;
    let mut black = app.connect(str_of(&created, "id"), Some(str_of(&created, "token"))).await;

    black.play_ok(4, 4).await;
    let state = black.wait_for("timeout forfeit", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "B+R");
    assert_eq!(state["move_number"], 1);
    // タイムアウトでは、つなぎ直してやり直すことはしない。
    assert_eq!(engine.connection_count(), 1);
}

#[tokio::test]
async fn unreachable_engine_counts_as_resignation() {
    // 待ち受けを閉じたポートを指すエンジン。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let name = unique_name("down");
    let spec = server::config::EngineSpec {
        name: name.clone(),
        label: None,
        host: "127.0.0.1".to_string(),
        port,
        max_concurrent: 1,
        max_board_size: 19,
    };
    let app = TestApp::start(test_config(vec![spec])).await;

    let created = app.create_game(9, &name, "human").await;
    let mut white = app.connect(str_of(&created, "id"), Some(str_of(&created, "token"))).await;
    let state = white.wait_for("forfeit", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "W+R");

    // エンジンが対応していない盤サイズでは対局を作れない。
    let (status, reply) = app
        .post_json("/api/games", json!({ "board_size": 21, "komi": 7.5, "black": "human", "white": name }))
        .await;
    assert_eq!(status, 400);
    assert_eq!(reply["error"]["code"], "unsupported_board_size");
}

#[tokio::test]
async fn reconnects_and_replays_history_when_the_connection_drops() {
    let engine = FakeEngine::start(Behavior::DropAfterGenmove).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    let created = app.create_game(9, "human", &engine.name).await;
    let mut black = app.connect(str_of(&created, "id"), Some(str_of(&created, "token"))).await;

    black.play_ok(4, 4).await;
    let state = black.wait_for("first reply", |s| s["move_number"] == 2 && s["thinking"].is_null()).await;
    let reply = state["moves"][1].clone();
    assert_eq!(engine.connection_count(), 1);

    // エンジンは応答の直後に接続を切っている。次の手番では、つなぎ直して
    // clear_board から着手履歴を再生する。
    let (x, y) = first_empty(&state);
    black.play_ok(x, y).await;
    let state = black.wait_for("second reply", |s| s["move_number"] == 4 && s["thinking"].is_null()).await;
    assert_eq!(state["status"], "playing");
    assert_eq!(engine.connection_count(), 2);
    let reply_vertex = gtp_vertex(reply["x"].as_u64().unwrap() as usize, reply["y"].as_u64().unwrap() as usize, 9);
    assert_eq!(
        engine.commands(2),
        [
            "boardsize 9".to_string(),
            "komi 7.5".to_string(),
            "clear_board".to_string(),
            "play B E5".to_string(),
            format!("play W {reply_vertex}"),
            format!("play B {}", gtp_vertex(x, y, 9)),
            "genmove W".to_string(),
        ]
    );
}

async fn max_concurrent_thinking(ai_max_concurrent: usize, engine_max_concurrent: usize) -> usize {
    let engine = FakeEngine::start(Behavior::Slow(Duration::from_millis(60))).await;
    let mut spec = engine.spec();
    spec.max_concurrent = engine_max_concurrent;
    let mut config = test_config(vec![spec]);
    config.ai_max_concurrent = ai_max_concurrent;
    let app = TestApp::start(config).await;

    // AI 同士の対局を 3 つ同時に走らせる。
    let mut watchers = Vec::new();
    for _ in 0..3 {
        let created = app.create_game(5, &engine.name, &engine.name).await;
        watchers.push(app.connect(str_of(&created, "id"), None).await);
    }
    // どの対局も進む(制限に達していても、順番待ちで進む)。
    for watcher in &mut watchers {
        watcher.wait_for("progress", |s| s["move_number"].as_u64().unwrap() >= 3).await;
    }
    let max = engine.max_thinking.load(Ordering::SeqCst);
    app.stop().await;
    max
}

#[tokio::test]
async fn concurrent_ai_thinking_is_limited() {
    // 制限なし(十分に大きい)なら、同時に思考する。
    assert!(max_concurrent_thinking(8, 8).await >= 2);
    // 全体の制限。
    assert_eq!(max_concurrent_thinking(1, 8).await, 1);
    assert_eq!(max_concurrent_thinking(2, 8).await, 2);
    // エンジンごとの制限。
    assert_eq!(max_concurrent_thinking(8, 1).await, 1);
}

#[tokio::test]
async fn ai_games_resume_after_server_restart() {
    let engine = FakeEngine::start(Behavior::Slow(Duration::from_millis(40))).await;
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    let created = app.create_game(5, &engine.name, &engine.name).await;
    let id = str_of(&created, "id").to_string();

    let mut watcher = app.connect(&id, None).await;
    let before = watcher.wait_for("a few moves", |s| s["move_number"].as_u64().unwrap() >= 4).await;
    assert_eq!(before["status"], "playing");
    watcher.close().await;
    app.stop().await;
    let connections_before = engine.connection_count();

    // 起動し直すと、誰も開かなくても AI の手番が再開される。
    let app = TestApp::start(test_config(vec![engine.spec()])).await;
    assert!(app.manager.loaded_rooms().await >= 1);
    let mut watcher = app.connect(&id, None).await;
    let state = watcher.wait_for("game over after restart", |s| s["status"] == "finished").await;
    // 再起動前の着手履歴がそのまま先頭に残っている。
    let n = before["moves"].as_array().unwrap().len();
    assert_eq!(state["moves"].as_array().unwrap()[..n], before["moves"].as_array().unwrap()[..]);
    assert!(engine.connection_count() > connections_before);
}

#[tokio::test]
async fn human_vs_ai_game_resumes_on_the_engine_turn() {
    // 人間が打った直後(AI の手番)にサーバーが落ちた場合。
    let slow = FakeEngine::start(Behavior::Slow(Duration::from_secs(30))).await;
    let mut config = test_config(vec![slow.spec()]);
    config.ai_move_timeout = Duration::from_secs(60);
    let app = TestApp::start(config).await;
    let created = app.create_game(9, "human", &slow.name).await;
    let id = str_of(&created, "id").to_string();
    let token = str_of(&created, "token").to_string();
    let mut black = app.connect(&id, Some(&token)).await;
    black.play_ok(4, 4).await;
    black.wait_for("thinking", |s| s["thinking"] == "white").await;
    black.close().await;
    app.stop().await;

    // 同じ名前で、すぐ応答するエンジンに差し替えて起動し直す(compose の設定だけで差し替えられる)。
    let fast = FakeEngine::start(Behavior::Normal).await;
    let mut spec = fast.spec();
    spec.name = slow.name.clone();
    let app = TestApp::start(test_config(vec![spec])).await;
    let mut black = app.connect(&id, Some(&token)).await;
    let state = black.wait_for("engine reply after restart", |s| s["move_number"] == 2).await;
    assert_eq!(state["last_move"]["color"], "white");
    assert_eq!(fast.commands(1), ["boardsize 9", "komi 7.5", "clear_board", "play B E5", "genmove W"]);
}
