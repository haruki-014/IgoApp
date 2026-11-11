//! 囲碁対戦サーバー。REST と WebSocket、対局管理、DB への永続化、GTP クライアント。

pub mod api;
pub mod config;
pub mod db;
pub mod error;
pub mod gtp;
pub mod manager;
pub mod model;
pub mod room;
pub mod token;
pub mod ws;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::{get, post};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::config::Config;
use crate::manager::Manager;
use crate::room::Deps;

/// DB に接続する。DB の起動を待つため、しばらくは失敗しても繰り返す。
pub async fn connect_db(url: &str) -> Result<PgPool, sqlx::Error> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let connecting = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await;
        match connecting {
            Ok(pool) => return Ok(pool),
            Err(e) if attempt < 30 => {
                tracing::warn!(attempt, error = %e, "DB に接続できません。1 秒後にもう一度試します");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// マイグレーションを適用する(sqlx migrate。SQL は `migrations/` にあり、バイナリに埋め込む)。
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

pub fn router(manager: Arc<Manager>) -> Router {
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/engines", get(api::list_engines))
        .route("/api/games", post(api::create_game))
        .route("/api/games/{id}", get(api::get_game))
        .route("/api/games/{id}/sgf", get(api::get_sgf))
        .route("/ws/games/{id}", get(ws::handler))
        .with_state(manager)
}

/// 対局管理を組み立て、AI が座っている対局を再開する。
pub async fn build(config: Config, pool: PgPool) -> Arc<Manager> {
    let manager = Manager::new(Deps::new(pool, config));
    let resumed = manager.resume_engine_games().await;
    if resumed > 0 {
        tracing::info!(resumed, "AI が座っている対局を再開しました");
    }
    manager.spawn_cleanup(Duration::from_secs(60));
    manager
}
