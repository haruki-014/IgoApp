//! DB への永続化。盤面は保存せず、着手履歴だけを残す。

use go_core::{Color, Coord, Move, PlayedMove};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{GameMeta, Player, Status};

#[derive(sqlx::FromRow)]
struct GameRow {
    id: Uuid,
    board_size: i16,
    komi: f64,
    status: String,
    black_player: String,
    white_player: String,
    black_token_hash: Option<String>,
    white_token_hash: Option<String>,
    result: Option<String>,
}

#[derive(sqlx::FromRow)]
struct MoveRow {
    color: String,
    x: Option<i16>,
    y: Option<i16>,
}

/// DB から読み出した対局。
pub struct StoredGame {
    pub meta: GameMeta,
    pub status: Status,
    pub result: Option<String>,
    pub moves: Vec<PlayedMove>,
}

fn color_code(color: Color) -> &'static str {
    match color {
        Color::Black => "B",
        Color::White => "W",
    }
}

pub async fn insert_game(pool: &PgPool, meta: &GameMeta) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO games (id, board_size, komi, black_player, white_player, black_token_hash, white_token_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(meta.id)
    .bind(meta.size as i16)
    .bind(meta.komi)
    .bind(meta.players[0].as_str())
    .bind(meta.players[1].as_str())
    .bind(meta.token_hashes[0].as_deref())
    .bind(meta.token_hashes[1].as_deref())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_game(pool: &PgPool, id: Uuid) -> Result<Option<StoredGame>, sqlx::Error> {
    let row: Option<GameRow> = sqlx::query_as(
        "SELECT id, board_size, komi, status, black_player, white_player,
                black_token_hash, white_token_hash, result
         FROM games WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };

    let move_rows: Vec<MoveRow> =
        sqlx::query_as("SELECT color, x, y FROM moves WHERE game_id = $1 ORDER BY move_number")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let moves = move_rows
        .into_iter()
        .map(|m| PlayedMove {
            color: if m.color == "B" { Color::Black } else { Color::White },
            mv: match (m.x, m.y) {
                (Some(x), Some(y)) => Move::Play(Coord::new(x as usize, y as usize)),
                _ => Move::Pass,
            },
        })
        .collect();

    Ok(Some(StoredGame {
        meta: GameMeta {
            id: row.id,
            size: row.board_size as usize,
            komi: row.komi,
            players: [Player::from_db(&row.black_player), Player::from_db(&row.white_player)],
            token_hashes: [row.black_token_hash, row.white_token_hash],
        },
        status: if row.status == Status::Finished.as_str() { Status::Finished } else { Status::Playing },
        result: row.result,
        moves,
    }))
}

/// 1 手を記録する。その手で終局するなら、同じトランザクションで結果も記録する。
pub async fn record_move(
    pool: &PgPool,
    game_id: Uuid,
    move_number: usize,
    color: Color,
    mv: Move,
    finish_with: Option<&str>,
) -> Result<(), sqlx::Error> {
    let (x, y) = match mv {
        Move::Play(c) => (Some(c.x as i16), Some(c.y as i16)),
        Move::Pass => (None, None),
    };
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO moves (game_id, move_number, color, x, y) VALUES ($1, $2, $3, $4, $5)")
        .bind(game_id)
        .bind(move_number as i32)
        .bind(color_code(color))
        .bind(x)
        .bind(y)
        .execute(&mut *tx)
        .await?;
    if let Some(result) = finish_with {
        finish_query(game_id, result).execute(&mut *tx).await?;
    }
    tx.commit().await
}

fn finish_query(game_id: Uuid, result: &str) -> sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
        "UPDATE games SET status = 'finished', result = $2, finished_at = now()
         WHERE id = $1 AND status = 'playing'",
    )
    .bind(game_id)
    .bind(result)
}

/// 対局を終了させる(投了など、着手を伴わない終局)。
pub async fn finish_game(pool: &PgPool, game_id: Uuid, result: &str) -> Result<(), sqlx::Error> {
    finish_query(game_id, result).execute(pool).await?;
    Ok(())
}

/// 待った:`keep` 手目より後の着手を消す。
pub async fn delete_moves_after(pool: &PgPool, game_id: Uuid, keep: usize) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM moves WHERE game_id = $1 AND move_number > $2")
        .bind(game_id)
        .bind(keep as i32)
        .execute(pool)
        .await?;
    Ok(())
}

/// 対局中で、エンジンが座っている対局。(ID, 黒のプレイヤー, 白のプレイヤー)。
/// サーバー起動時に AI の手番を再開するために使う。
pub async fn playing_engine_games(pool: &PgPool) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, black_player, white_player FROM games
         WHERE status = 'playing' AND (black_player <> 'human' OR white_player <> 'human')
         ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
}
