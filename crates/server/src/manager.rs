//! 対局の作成と、対局アクターの管理。
//!
//! アクターは必要になったときに DB の着手履歴から起動する。サーバーを再起動しても、
//! 対局 URL を開けば続きから再開できる。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use go_core::Color;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::db::{self, StoredGame};
use crate::error::ApiError;
use crate::model::{GameMeta, HUMAN, Player, Status};
use crate::room::{self, Deps, RoomHandle};
use crate::token;

/// 対局作成の指定。
#[derive(Clone, Debug)]
pub struct NewGame {
    pub size: usize,
    pub komi: f64,
    pub black: Player,
    pub white: Player,
    /// 作成者が座る席。両方が人間のときに使う(省略時は黒)。
    pub creator: Option<Color>,
}

/// 作成した対局。トークンはここでしか得られない(DB にはハッシュだけを保存する)。
#[derive(Clone, Debug)]
pub struct Created {
    pub id: Uuid,
    /// 作成者の席。AI 同士の対局なら `None`。
    pub creator: Option<Color>,
    /// 作成者の席トークン。
    pub token: Option<String>,
    /// 相手の席トークン(招待 URL に入れる)。相手が AI なら `None`。
    pub invite_token: Option<String>,
}

pub struct Manager {
    deps: Arc<Deps>,
    rooms: Mutex<HashMap<Uuid, RoomHandle>>,
}

fn invalid(code: &'static str, message: impl Into<String>) -> ApiError {
    ApiError::Invalid(code, message.into())
}

impl Manager {
    pub fn new(deps: Deps) -> Arc<Manager> {
        Arc::new(Manager { deps: Arc::new(deps), rooms: Mutex::new(HashMap::new()) })
    }

    pub fn deps(&self) -> &Deps {
        &self.deps
    }

    fn validate(&self, req: &NewGame) -> Result<(), ApiError> {
        if !go_core::is_valid_size(req.size) {
            return Err(invalid("invalid_board_size", "盤サイズは 2〜25 路で指定してください"));
        }
        // コミは x.5 のみ(同点が起こらないようにする)。
        let doubled = req.komi * 2.0;
        let is_half = doubled.is_finite() && doubled.fract() == 0.0 && (doubled as i64).rem_euclid(2) == 1;
        if !is_half || req.komi.abs() > 99.5 {
            return Err(invalid("invalid_komi", "コミは 0.5、6.5、7.5 のように端数 0.5 付きで指定してください"));
        }
        for player in [&req.black, &req.white] {
            if let Player::Engine(name) = player {
                let engine = self
                    .deps
                    .engines
                    .get(name)
                    .ok_or_else(|| invalid("unknown_engine", format!("エンジン {name:?} は設定されていません")))?;
                if req.size > engine.spec.max_board_size {
                    return Err(invalid(
                        "unsupported_board_size",
                        format!("{} は {} 路までしか対応していません", engine.spec.label(), engine.spec.max_board_size),
                    ));
                }
            }
        }
        Ok(())
    }

    /// 対局を作成する。人間の各席にランダムな席トークンを発行する。
    pub async fn create(&self, req: NewGame) -> Result<Created, ApiError> {
        self.validate(&req)?;

        let creator = match (req.black.is_human(), req.white.is_human()) {
            (true, true) => Some(req.creator.unwrap_or(Color::Black)),
            (true, false) => Some(Color::Black),
            (false, true) => Some(Color::White),
            (false, false) => None,
        };
        if let (Some(requested), Some(actual)) = (req.creator, creator)
            && requested != actual
        {
            return Err(invalid("invalid_creator_color", "作成者の席には人間を指定してください"));
        }

        let players = [req.black, req.white];
        let tokens: [Option<String>; 2] = [0, 1].map(|i| players[i].is_human().then(token::generate));
        let meta = GameMeta {
            id: Uuid::new_v4(),
            size: req.size,
            komi: req.komi,
            players,
            token_hashes: [0, 1].map(|i| tokens[i].as_deref().map(token::hash)),
        };
        db::insert_game(&self.deps.pool, &meta).await?;
        let id = meta.id;
        tracing::info!(
            game = %id, size = meta.size, komi = meta.komi,
            black = meta.players[0].as_str(), white = meta.players[1].as_str(), "対局を作成しました"
        );

        let stored = StoredGame { meta, status: Status::Playing, result: None, moves: Vec::new() };
        let handle = room::spawn(stored, self.deps.clone()).map_err(|e| ApiError::Internal(e.to_string()))?;
        self.rooms.lock().await.insert(id, handle);

        let [black_token, white_token] = tokens;
        let (token, invite_token) = match creator {
            Some(Color::Black) => (black_token, white_token),
            Some(Color::White) => (white_token, black_token),
            None => (None, None),
        };
        Ok(Created { id, creator, token, invite_token })
    }

    /// 対局アクターの窓口を得る。起動していなければ、DB の着手履歴から起動する。
    pub async fn get(&self, id: Uuid) -> Result<Option<RoomHandle>, ApiError> {
        let mut rooms = self.rooms.lock().await;
        if let Some(handle) = rooms.get(&id) {
            return Ok(Some(handle.clone()));
        }
        let Some(stored) = db::load_game(&self.deps.pool, id).await? else {
            return Ok(None);
        };
        let handle = room::spawn(stored, self.deps.clone())
            .map_err(|e| ApiError::Internal(format!("対局 {id} の着手履歴を再生できません: {e}")))?;
        rooms.insert(id, handle.clone());
        Ok(Some(handle))
    }

    /// サーバー起動時に、AI が座っている対局中の対局を再開する。
    /// 設定から外されたエンジンの対局は再開しない(開かれたときに読み込む)。
    pub async fn resume_engine_games(&self) -> usize {
        let games = match db::playing_engine_games(&self.deps.pool).await {
            Ok(games) => games,
            Err(e) => {
                tracing::error!(error = %e, "再開する対局を取得できませんでした");
                return 0;
            }
        };
        let configured = |player: &str| player == HUMAN || self.deps.engines.contains_key(player);
        let mut resumed = 0;
        for (id, black, white) in games {
            if !configured(&black) || !configured(&white) {
                continue;
            }
            match self.get(id).await {
                Ok(Some(_)) => resumed += 1,
                Ok(None) => {}
                Err(e) => tracing::error!(game = %id, error = ?e, "対局を再開できませんでした"),
            }
        }
        resumed
    }

    /// 誰も見ておらず、AI の手番でもない対局のアクターを定期的に片付ける。
    /// 片付けても DB に着手履歴があるので、次に開かれたときに起動し直せる。
    pub fn spawn_cleanup(self: &Arc<Self>, every: Duration) {
        let manager = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(every);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(manager) = manager.upgrade() else { break };
                manager.cleanup().await;
            }
        });
    }

    async fn cleanup(&self) {
        let mut rooms = self.rooms.lock().await;
        let before = rooms.len();
        rooms.retain(|_, handle| {
            if handle.subscriber_count() > 0 {
                return true;
            }
            let state = handle.snapshot();
            if state.status == Status::Finished {
                return false;
            }
            // 対局中:次の手番が(設定にある)AI なら動かし続ける。
            let to_move = if state.to_move == "black" { Color::Black } else { Color::White };
            let engine_to_move = match handle.meta.player(to_move) {
                Player::Human => false,
                Player::Engine(name) => self.deps.engines.contains_key(name),
            };
            engine_to_move || state.thinking.is_some()
        });
        if rooms.len() < before {
            tracing::debug!(evicted = before - rooms.len(), remaining = rooms.len(), "対局アクターを片付けました");
        }
    }

    /// メモリ上にある対局アクターの数(テスト用)。
    pub async fn loaded_rooms(&self) -> usize {
        self.rooms.lock().await.len()
    }

    /// すべての対局アクターをメモリから下ろす(テスト用。サーバー再起動の代わり)。
    pub async fn unload_all(&self) {
        self.rooms.lock().await.clear();
    }
}
