//! WebSocket。メッセージは JSON。
//!
//! クライアント → サーバー:
//! - `{"type":"join","token":"…"}` 席トークンで着席する(トークンなしなら観戦者)
//! - `{"type":"play","x":3,"y":4}` / `{"type":"pass"}` / `{"type":"resign"}` / `{"type":"undo"}`
//!
//! サーバー → クライアント:
//! - `{"type":"state", …}` 対局の状態。状態が変わるたびに、同じ対局のすべての接続へ送る
//! - `{"type":"error","code":"superko","message":"…"}`

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use go_core::{Color, Coord};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, GameError};
use crate::manager::Manager;
use crate::model::{Snapshot, color_name};
use crate::room::{Action, RoomHandle};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Join {
        #[serde(default)]
        token: Option<String>,
    },
    Play {
        x: usize,
        y: usize,
    },
    Pass,
    Resign,
    Undo,
}

#[derive(Serialize)]
struct StateMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    state: &'a Snapshot,
    /// この接続の席(`black` / `white`)。観戦者は null。
    you: Option<&'static str>,
}

#[derive(Serialize)]
struct ErrorMessage {
    #[serde(rename = "type")]
    kind: &'static str,
    code: &'static str,
    message: &'static str,
}

pub async fn handler(
    upgrade: WebSocketUpgrade,
    State(manager): State<Arc<Manager>>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let room = manager.get(id).await?.ok_or(ApiError::NotFound)?;
    Ok(upgrade.max_message_size(16 * 1024).on_upgrade(move |socket| async move {
        // 切断はエラーとして返ってくるが、異常ではないので記録しない。
        let _ = serve(socket, room).await;
    }))
}

async fn send_state(socket: &mut WebSocket, state: &Snapshot, seat: Option<Color>) -> Result<(), axum::Error> {
    let message = StateMessage { kind: "state", state, you: seat.map(color_name) };
    let json = serde_json::to_string(&message).expect("state is serializable");
    socket.send(Message::from(json)).await
}

async fn send_error(socket: &mut WebSocket, error: GameError) -> Result<(), axum::Error> {
    let message = ErrorMessage { kind: "error", code: error.code(), message: error.message() };
    let json = serde_json::to_string(&message).expect("error is serializable");
    socket.send(Message::from(json)).await
}

async fn serve(mut socket: WebSocket, room: RoomHandle) -> Result<(), axum::Error> {
    let mut updates = room.subscribe();
    // join するまでは観戦者。
    let mut seat: Option<Color> = None;
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let state = updates.borrow_and_update().clone();
    send_state(&mut socket, &state, seat).await?;

    loop {
        tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() {
                    // 対局アクターが終了した(通常は起こらない)。切断して、クライアントの再接続に任せる。
                    return Ok(());
                }
                let state = updates.borrow_and_update().clone();
                send_state(&mut socket, &state, seat).await?;
            }
            incoming = socket.recv() => {
                let Some(message) = incoming else { return Ok(()) };
                let text = match message? {
                    Message::Text(text) => text,
                    Message::Close(_) => return Ok(()),
                    _ => continue,
                };
                let Ok(message) = serde_json::from_str::<ClientMessage>(text.as_str()) else {
                    send_error(&mut socket, GameError::BadRequest).await?;
                    continue;
                };
                let action = match message {
                    ClientMessage::Join { token } => {
                        seat = token.as_deref().and_then(|t| room.seat_for_token(t));
                        if token.is_some() && seat.is_none() {
                            send_error(&mut socket, GameError::InvalidToken).await?;
                        }
                        // 席を伝えるために、状態を送り直す。
                        let state = updates.borrow_and_update().clone();
                        send_state(&mut socket, &state, seat).await?;
                        continue;
                    }
                    ClientMessage::Play { x, y } => Action::Play(Coord::new(x, y)),
                    ClientMessage::Pass => Action::Pass,
                    ClientMessage::Resign => Action::Resign,
                    ClientMessage::Undo => Action::Undo,
                };
                // トークンなしの接続は閲覧のみ。
                let result = match seat {
                    Some(color) => room.act(color, action).await,
                    None => Err(GameError::NotAPlayer),
                };
                if let Err(error) = result {
                    send_error(&mut socket, error).await?;
                }
            }
            _ = ping.tick() => {
                socket.send(Message::Ping(Default::default())).await?;
            }
        }
    }
}
