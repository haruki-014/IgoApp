//! オンラインでの人間同士の対局:REST での作成、WebSocket での対局、再接続、サーバー再起動。

mod common;

use common::*;
use serde_json::{Value, json};

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_else(|| panic!("{key} is not a string in {value}"))
}

#[tokio::test]
async fn create_game_validates_input() {
    let app = TestApp::start(test_config(vec![])).await;

    let (status, body) = app.get_json("/api/engines").await;
    assert_eq!(status, 200);
    assert_eq!(body, json!([]));

    let create = |body: Value| app.post_json("/api/games", body);
    let base = json!({ "board_size": 9, "komi": 7.5, "black": "human", "white": "human" });

    let cases = [
        ("board_size", json!(1), "invalid_board_size"),
        ("board_size", json!(26), "invalid_board_size"),
        ("komi", json!(7.0), "invalid_komi"),
        ("komi", json!(7.25), "invalid_komi"),
        ("komi", json!(1000.5), "invalid_komi"),
        ("white", json!("no-such-engine"), "unknown_engine"),
        ("creator_color", json!("green"), "invalid_creator_color"),
    ];
    for (field, value, code) in cases {
        let mut body = base.clone();
        body[field] = value.clone();
        let (status, reply) = create(body).await;
        assert_eq!(status, 400, "{field} = {value}");
        assert_eq!(reply["error"]["code"], code, "{field} = {value}: {reply}");
    }
    // 形式の誤り。
    let (status, reply) = create(json!({ "board_size": "nine" })).await;
    assert_eq!(status, 400);
    assert_eq!(reply["error"]["code"], "bad_request");

    // 2 路と 25 路、マイナスのコミ(逆コミ)は受け付ける。
    for (size, komi) in [(2, 0.5), (25, -5.5), (19, 6.5)] {
        let (status, reply) =
            create(json!({ "board_size": size, "komi": komi, "black": "human", "white": "human" })).await;
        assert_eq!(status, 201, "{reply}");
        let (status, state) = app.get_json(&format!("/api/games/{}", str_of(&reply, "id"))).await;
        assert_eq!(status, 200);
        assert_eq!(state["board_size"], size);
        assert_eq!(state["komi"], komi);
        assert_eq!(state["board"].as_array().unwrap().len(), size);
    }

    // 存在しない対局。
    let (status, reply) = app.get_json("/api/games/00000000-0000-4000-8000-000000000000").await;
    assert_eq!(status, 404);
    assert_eq!(reply["error"]["code"], "not_found");
    let (status, _, _) = app.http("GET", "/api/games/00000000-0000-4000-8000-000000000000/sgf", None).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn seat_tokens_are_issued_and_only_hashes_are_stored() {
    let app = TestApp::start(test_config(vec![])).await;

    // 作成者が白を選ぶ。
    let (status, created) = app
        .post_json(
            "/api/games",
            json!({ "board_size": 9, "komi": 7.5, "black": "human", "white": "human", "creator_color": "white" }),
        )
        .await;
    assert_eq!(status, 201);
    assert_eq!(created["creator_color"], "white");
    let (token, invite) = (str_of(&created, "token"), str_of(&created, "invite_token"));
    assert_ne!(token, invite);
    assert_eq!(token.len(), 64);

    let id = str_of(&created, "id");
    let white = app.connect(id, Some(token)).await;
    assert_eq!(white.last_state["you"], "white");
    let black = app.connect(id, Some(invite)).await;
    assert_eq!(black.last_state["you"], "black");

    // DB にはハッシュだけが入っている。
    let row: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT black_token_hash, white_token_hash FROM games WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(row.0.as_deref(), Some(server::token::hash(invite).as_str()));
    assert_eq!(row.1.as_deref(), Some(server::token::hash(token).as_str()));
    assert_ne!(row.0.as_deref(), Some(invite));
    assert_ne!(row.1.as_deref(), Some(token));

    // 状態にトークンやハッシュは含まれない。
    let (_, state) = app.get_json(&format!("/api/games/{id}")).await;
    let text = state.to_string();
    assert!(!text.contains(token) && !text.contains(invite));
    assert!(!text.contains(row.0.as_deref().unwrap()));
}

#[tokio::test]
async fn two_humans_play_online_with_a_spectator() {
    let app = TestApp::start(test_config(vec![])).await;
    let created = app.create_game(5, "human", "human").await;
    let id = str_of(&created, "id");
    assert_eq!(created["creator_color"], "black");

    let mut black = app.connect(id, Some(str_of(&created, "token"))).await;
    let mut white = app.connect(id, Some(str_of(&created, "invite_token"))).await;
    // トークンなしで開いた人は観戦者。
    let mut watcher = app.connect(id, None).await;
    assert_eq!(black.last_state["you"], "black");
    assert_eq!(white.last_state["you"], "white");
    assert_eq!(watcher.last_state["you"], Value::Null);
    assert_eq!(black.last_state["status"], "playing");
    assert_eq!(black.last_state["to_move"], "black");
    assert_eq!(black.last_state["undo_available"], false);

    // 黒が打つと、両対局者と観戦者の全員に配信される。
    black.play(1, 0).await;
    for client in [&mut black, &mut white, &mut watcher] {
        let state = client.wait_for("first move", |s| s["move_number"] == 1).await;
        assert_eq!(state["board"][0], ".X...");
        assert_eq!(state["to_move"], "white");
        assert_eq!(state["last_move"], json!({ "color": "black", "x": 1, "y": 0 }));
        assert_eq!(state["moves"].as_array().unwrap().len(), 1);
    }

    // 手番違い・観戦者・石のある点。
    assert_eq!(black.expect_error(json!({ "type": "play", "x": 3, "y": 3 })).await, "not_your_turn");
    assert_eq!(watcher.expect_error(json!({ "type": "play", "x": 3, "y": 3 })).await, "not_a_player");
    assert_eq!(watcher.expect_error(json!({ "type": "pass" })).await, "not_a_player");
    assert_eq!(watcher.expect_error(json!({ "type": "resign" })).await, "not_a_player");
    assert_eq!(white.expect_error(json!({ "type": "play", "x": 1, "y": 0 })).await, "occupied");
    assert_eq!(white.expect_error(json!({ "type": "play", "x": 5, "y": 0 })).await, "out_of_bounds");
    assert_eq!(white.expect_error(json!({ "type": "nonsense" })).await, "bad_request");
    // オンライン対人戦では待ったは使えない。
    assert_eq!(white.expect_error(json!({ "type": "undo" })).await, "undo_not_allowed");

    // コウの形を作る:黒 (1,0)(0,1)(1,2)、白 (2,0)(3,1)(2,2)(1,1)。
    white.play_ok(2, 0).await;
    black.play_ok(0, 1).await;
    white.play_ok(3, 1).await;
    black.play_ok(1, 2).await;
    white.play_ok(2, 2).await;
    black.play_ok(4, 4).await;
    white.play_ok(1, 1).await;
    // 黒がコウを取る。アゲハマが 1 つ増える。
    let state = black.play_ok(2, 1).await;
    assert_eq!(state["captures"], json!({ "black": 1, "white": 0 }));
    assert_eq!(state["board"][1], "X.XO.");

    // 白はすぐには取り返せない(コウ・同形反復)。隅は自殺手。
    assert_eq!(white.expect_error(json!({ "type": "play", "x": 1, "y": 1 })).await, "superko");
    assert_eq!(white.expect_error(json!({ "type": "play", "x": 0, "y": 0 })).await, "suicide");
    // 不正手では盤面も手番も変わらない。
    let (_, state) = app.get_json(&format!("/api/games/{id}")).await;
    assert_eq!(state["move_number"], 9);
    assert_eq!(state["to_move"], "white");

    // 両者が連続でパスすると終局し、その時点の盤面をそのまま計算する。
    white.send(json!({ "type": "pass" })).await;
    let state = black.wait_for("white's pass", |s| s["move_number"] == 10).await;
    assert_eq!(state["last_move"], json!({ "color": "white", "x": null, "y": null }));
    assert_eq!(state["status"], "playing");
    black.send(json!({ "type": "pass" })).await;
    for client in [&mut black, &mut white, &mut watcher] {
        let state = client.wait_for("game over", |s| s["status"] == "finished").await;
        // 黒 5 子 + 黒地 2((0,0) と (1,1))= 7、白 3 子 + コミ 7.5 = 10.5。
        assert_eq!(state["result"], "W+3.5");
        let score = &state["score"];
        assert_eq!(score["black_stones"], 5);
        assert_eq!(score["black_territory"], 2);
        assert_eq!(score["white_stones"], 3);
        assert_eq!(score["white_territory"], 0);
        assert_eq!(score["neutral"], 15);
        assert_eq!(score["black_total"], 7.0);
        assert_eq!(score["white_total"], 10.5);
        assert_eq!(score["ownership"][0], "bbw..");
        assert_eq!(score["ownership"][1], "bbbw.");
    }

    // 終局後は操作できない。
    assert_eq!(black.expect_error(json!({ "type": "play", "x": 3, "y": 3 })).await, "game_over");
    assert_eq!(white.expect_error(json!({ "type": "resign" })).await, "game_over");

    // DB には着手履歴と結果だけが残る。
    let (status, result, finished): (String, Option<String>, bool) =
        sqlx::query_as("SELECT status, result, finished_at IS NOT NULL FROM games WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), result.as_deref(), finished), ("finished", Some("W+3.5"), true));
    let moves: Vec<(i32, String, Option<i16>, Option<i16>)> =
        sqlx::query_as("SELECT move_number, color, x, y FROM moves WHERE game_id = $1::uuid ORDER BY move_number")
            .bind(id)
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(moves.len(), 11);
    assert_eq!(moves[0], (1, "B".to_string(), Some(1), Some(0)));
    assert_eq!(moves[9], (10, "W".to_string(), None, None));
    assert_eq!(moves[10], (11, "B".to_string(), None, None));

    // SGF のダウンロード。
    let (status, head, sgf) = app.http("GET", &format!("/api/games/{id}/sgf"), None).await;
    assert_eq!(status, 200);
    assert!(head.to_lowercase().contains("content-type: application/x-go-sgf"));
    assert!(head.to_lowercase().contains("content-disposition: attachment; filename=\"igo-"));
    for part in ["SZ[5]", "KM[7.5]", "RU[Chinese]", "PB[Human]", "PW[Human]", "RE[W+3.5]", ";B[ba];W[ca]", ";W[]", ";B[])"] {
        assert!(sgf.contains(part), "SGF lacks {part}: {sgf}");
    }
}

#[tokio::test]
async fn resignation_ends_the_game() {
    let app = TestApp::start(test_config(vec![])).await;
    let created = app.create_game(9, "human", "human").await;
    let id = str_of(&created, "id");
    let mut black = app.connect(id, Some(str_of(&created, "token"))).await;
    let mut white = app.connect(id, Some(str_of(&created, "invite_token"))).await;

    black.play_ok(4, 4).await;
    // 投了は手番でなくてもできる。黒が投了すると白の勝ち。
    black.send(json!({ "type": "resign" })).await;
    for client in [&mut black, &mut white] {
        let state = client.wait_for("resignation", |s| s["status"] == "finished").await;
        assert_eq!(state["result"], "W+R");
        // 投了では計算の内訳は出さない。
        assert_eq!(state["score"], Value::Null);
        assert_eq!(state["move_number"], 1);
    }
    let (_, _, sgf) = app.http("GET", &format!("/api/games/{id}/sgf"), None).await;
    assert!(sgf.contains("RE[W+R]"));

    // 白の投了は黒の勝ち。
    let created = app.create_game(9, "human", "human").await;
    let id = str_of(&created, "id");
    let mut white = app.connect(id, Some(str_of(&created, "invite_token"))).await;
    white.send(json!({ "type": "resign" })).await;
    let state = white.wait_for("resignation", |s| s["status"] == "finished").await;
    assert_eq!(state["result"], "B+R");
}

#[tokio::test]
async fn reconnecting_with_the_token_restores_the_seat() {
    let app = TestApp::start(test_config(vec![])).await;
    let created = app.create_game(9, "human", "human").await;
    let id = str_of(&created, "id");
    let token = str_of(&created, "token");
    let mut black = app.connect(id, Some(token)).await;
    let mut white = app.connect(id, Some(str_of(&created, "invite_token"))).await;

    black.play_ok(2, 2).await;
    white.play_ok(6, 6).await;

    // 黒が切断し、同じトークンでつなぎ直す。
    black.close().await;
    let mut black = app.connect(id, Some(token)).await;
    assert_eq!(black.last_state["you"], "black");
    assert_eq!(black.last_state["move_number"], 2);
    assert_eq!(black.last_state["board"][2], "..X......");
    assert_eq!(black.last_state["board"][6], "......O..");
    // 続きを打てる。
    black.play_ok(2, 6).await;
    let state = white.wait_for("black's move after reconnect", |s| s["move_number"] == 3).await;
    assert_eq!(state["to_move"], "white");

    // 誤ったトークンでは観戦者になる。
    let url_token = "0".repeat(64);
    let mut intruder = app.connect(id, None).await;
    let code = intruder.expect_error(json!({ "type": "join", "token": url_token })).await;
    assert_eq!(code, "invalid_token");
    let state = intruder.next_state().await;
    assert_eq!(state["you"], Value::Null);
    assert_eq!(intruder.expect_error(json!({ "type": "play", "x": 0, "y": 0 })).await, "not_a_player");
    // 相手の席のトークンでは、相手の席にしか座れない。
    let other = app.connect(id, Some(str_of(&created, "invite_token"))).await;
    assert_eq!(other.last_state["you"], "white");
}

#[tokio::test]
async fn game_continues_after_server_restart() {
    let app = TestApp::start(test_config(vec![])).await;
    let created = app.create_game(9, "human", "human").await;
    let id = str_of(&created, "id").to_string();
    let token = str_of(&created, "token").to_string();
    let invite = str_of(&created, "invite_token").to_string();

    let mut black = app.connect(&id, Some(&token)).await;
    let mut white = app.connect(&id, Some(&invite)).await;
    // 石を取る手を含める(アゲハマも履歴から復元されることを確かめる)。
    black.play_ok(1, 0).await;
    white.play_ok(0, 0).await;
    let before = black.play_ok(0, 1).await;
    assert_eq!(before["captures"]["black"], 1);
    white.wait_for("capture", |s| s["move_number"] == 3).await;

    // サーバーを止め、同じ DB で起動し直す。
    black.close().await;
    white.close().await;
    app.stop().await;
    let app = TestApp::start(test_config(vec![])).await;

    // 盤面は DB の着手履歴から再構築される。
    let (status, state) = app.get_json(&format!("/api/games/{id}")).await;
    assert_eq!(status, 200);
    for key in ["board", "moves", "captures", "to_move", "move_number", "status", "last_move"] {
        assert_eq!(state[key], before[key], "{key} differs after restart");
    }

    // 同じトークンで着席し、続きを打てる。
    let mut black = app.connect(&id, Some(&token)).await;
    let mut white = app.connect(&id, Some(&invite)).await;
    assert_eq!(black.last_state["you"], "black");
    assert_eq!(white.last_state["you"], "white");
    assert_eq!(black.expect_error(json!({ "type": "play", "x": 5, "y": 5 })).await, "not_your_turn");
    // 取られた跡 (0,0) は白にとって自殺手のまま。
    assert_eq!(white.expect_error(json!({ "type": "play", "x": 0, "y": 0 })).await, "suicide");
    white.play_ok(5, 5).await;
    let state = black.wait_for("white's move after restart", |s| s["move_number"] == 4).await;
    assert_eq!(state["board"][5], ".....O...");
    assert_eq!(state["captures"]["black"], 1);
}

#[tokio::test]
async fn idle_rooms_are_reloaded_on_demand() {
    let app = TestApp::start(test_config(vec![])).await;
    let created = app.create_game(9, "human", "human").await;
    let id = str_of(&created, "id");
    let mut black = app.connect(id, Some(str_of(&created, "token"))).await;
    black.play_ok(3, 3).await;
    black.close().await;

    // メモリから下ろしても、次に開けば DB から読み直される。
    app.manager.unload_all().await;
    assert_eq!(app.manager.loaded_rooms().await, 0);
    let (_, state) = app.get_json(&format!("/api/games/{id}")).await;
    assert_eq!(state["move_number"], 1);
    assert_eq!(app.manager.loaded_rooms().await, 1);
}
