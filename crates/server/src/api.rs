//! REST API。

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use go_core::{Color, Coord, Game, Move, PlayedMove, sgf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::manager::{Manager, NewGame};
use crate::model::{HUMAN, Player, Snapshot, color_name};

pub async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
pub struct EngineView {
    name: String,
    label: String,
    max_board_size: usize,
}

/// 対局相手に選べるエンジンの一覧。
pub async fn list_engines(State(manager): State<Arc<Manager>>) -> Json<Vec<EngineView>> {
    let engines = manager
        .deps()
        .config
        .engines
        .iter()
        .map(|e| EngineView { name: e.name.clone(), label: e.label().to_string(), max_board_size: e.max_board_size })
        .collect();
    Json(engines)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGameRequest {
    board_size: usize,
    #[serde(default = "default_komi")]
    komi: f64,
    /// `human` またはエンジン名。
    black: String,
    white: String,
    /// 作成者が座る席(`black` / `white`)。両方が人間のときに使う。
    #[serde(default)]
    creator_color: Option<String>,
}

fn default_komi() -> f64 {
    7.5
}

#[derive(Serialize)]
pub struct CreateGameResponse {
    id: Uuid,
    /// 作成者の席。AI 同士の対局なら null。
    creator_color: Option<&'static str>,
    /// 作成者の席トークン。ブラウザが保存し、再接続に使う。
    token: Option<String>,
    /// 相手の席トークン。招待 URL に入れて相手に渡す。
    invite_token: Option<String>,
}

pub async fn create_game(
    State(manager): State<Arc<Manager>>,
    body: Result<Json<CreateGameRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CreateGameResponse>), ApiError> {
    let Json(req) = body.map_err(|e| ApiError::Invalid("bad_request", e.body_text()))?;
    let creator = match req.creator_color.as_deref() {
        None => None,
        Some("black") => Some(Color::Black),
        Some("white") => Some(Color::White),
        Some(_) => {
            return Err(ApiError::Invalid(
                "invalid_creator_color",
                "creator_color は black か white で指定してください".to_string(),
            ));
        }
    };
    let created = manager
        .create(NewGame {
            size: req.board_size,
            komi: req.komi,
            black: Player::from_db(&req.black),
            white: Player::from_db(&req.white),
            creator,
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateGameResponse {
            id: created.id,
            creator_color: created.creator.map(color_name),
            token: created.token,
            invite_token: created.invite_token,
        }),
    ))
}

/// 対局の現在の状態(観戦者と同じ内容)。
pub async fn get_game(State(manager): State<Arc<Manager>>, Path(id): Path<Uuid>) -> Result<Json<Snapshot>, ApiError> {
    let room = manager.get(id).await?.ok_or(ApiError::NotFound)?;
    Ok(Json((*room.snapshot()).clone()))
}

fn player_name(manager: &Manager, player: &str) -> String {
    if player == HUMAN {
        return "Human".to_string();
    }
    match manager.deps().engines.get(player) {
        Some(engine) => engine.spec.label().to_string(),
        None => player.to_string(),
    }
}

/// SGF のダウンロード。
pub async fn get_sgf(State(manager): State<Arc<Manager>>, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let room = manager.get(id).await?.ok_or(ApiError::NotFound)?;
    let state = room.snapshot();
    let moves: Vec<PlayedMove> = state
        .moves
        .iter()
        .map(|m| PlayedMove {
            color: if m.color == "black" { Color::Black } else { Color::White },
            mv: match (m.x, m.y) {
                (Some(x), Some(y)) => Move::Play(Coord::new(x, y)),
                _ => Move::Pass,
            },
        })
        .collect();
    let game =
        Game::from_moves(state.board_size, state.komi, &moves).map_err(|e| ApiError::Internal(e.to_string()))?;
    let text = sgf::to_sgf(
        &game,
        &sgf::SgfInfo {
            black: &player_name(&manager, &state.players.black),
            white: &player_name(&manager, &state.players.white),
            result: state.result.as_deref(),
        },
    );
    let headers = [
        (header::CONTENT_TYPE, "application/x-go-sgf; charset=utf-8".to_string()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"igo-{id}.sgf\"")),
        (header::CACHE_CONTROL, "no-store".to_string()),
    ];
    Ok((headers, text).into_response())
}
