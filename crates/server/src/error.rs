//! エラー。クライアントにはエラーコードと日本語のメッセージを返す。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use go_core::MoveError;
use serde_json::json;

/// 対局中の操作に対するエラー(WebSocket の `error` メッセージになる)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameError {
    Occupied,
    OutOfBounds,
    Suicide,
    Superko,
    NotYourTurn,
    NotAPlayer,
    GameOver,
    UndoNotAllowed,
    NothingToUndo,
    InvalidToken,
    BadRequest,
    Internal,
}

impl GameError {
    pub fn code(self) -> &'static str {
        match self {
            GameError::Occupied => "occupied",
            GameError::OutOfBounds => "out_of_bounds",
            GameError::Suicide => "suicide",
            GameError::Superko => "superko",
            GameError::NotYourTurn => "not_your_turn",
            GameError::NotAPlayer => "not_a_player",
            GameError::GameOver => "game_over",
            GameError::UndoNotAllowed => "undo_not_allowed",
            GameError::NothingToUndo => "nothing_to_undo",
            GameError::InvalidToken => "invalid_token",
            GameError::BadRequest => "bad_request",
            GameError::Internal => "internal",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            GameError::Occupied => "そこにはすでに石があります",
            GameError::OutOfBounds => "盤の外には打てません",
            GameError::Suicide => "自殺手は打てません(相手の石を取れない、呼吸点のない手です)",
            GameError::Superko => "コウ・同形反復のため打てません(以前と同じ盤面になります)",
            GameError::NotYourTurn => "あなたの手番ではありません",
            GameError::NotAPlayer => "観戦中のため操作できません",
            GameError::GameOver => "この対局はすでに終了しています",
            GameError::UndoNotAllowed => "待ったは人間対 AI の対局でのみ使えます",
            GameError::NothingToUndo => "取り消せる手がありません",
            GameError::InvalidToken => "席トークンが正しくありません。観戦者として接続します",
            GameError::BadRequest => "メッセージの形式が正しくありません",
            GameError::Internal => "サーバー内部でエラーが発生しました。もう一度お試しください",
        }
    }
}

impl From<MoveError> for GameError {
    fn from(e: MoveError) -> GameError {
        match e {
            MoveError::Occupied => GameError::Occupied,
            MoveError::OutOfBounds => GameError::OutOfBounds,
            MoveError::Suicide => GameError::Suicide,
            MoveError::Superko => GameError::Superko,
        }
    }
}

/// REST API のエラー。
#[derive(Debug)]
pub enum ApiError {
    NotFound,
    /// 入力の誤り。エラーコードとメッセージ。
    Invalid(&'static str, String),
    Internal(String),
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> ApiError {
        ApiError::Internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", "対局が見つかりません".to_string()),
            ApiError::Invalid(code, message) => (StatusCode::BAD_REQUEST, code, message),
            ApiError::Internal(detail) => {
                tracing::error!(%detail, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "サーバー内部でエラーが発生しました".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}
