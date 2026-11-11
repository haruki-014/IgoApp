//! 対局 1 つを受け持つアクター。
//!
//! 対局ごとにタスクを 1 つ動かし、着手・投了・待った・AI の手番をすべてその中で順番に
//! 処理する。着手が合法かどうかは必ずここで go-core によって判定する。
//! 状態が変わるたびに、同じ対局のすべての接続へ新しい状態を配信する。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use go_core::{Color, Coord, Game, Move, PlayedMove};
use sqlx::PgPool;
use tokio::sync::{Semaphore, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::{Config, EngineSpec};
use crate::db::{self, StoredGame};
use crate::error::GameError;
use crate::gtp::{GtpClient, GtpError};
use crate::model::{GameMeta, Player, Snapshot, Status};
use crate::token;

/// エンジンの設定と、同時思考数の制限。
pub struct EngineRuntime {
    pub spec: EngineSpec,
    limiter: Semaphore,
}

/// すべての対局で共有するもの。
pub struct Deps {
    pub pool: PgPool,
    pub config: Config,
    pub engines: HashMap<String, Arc<EngineRuntime>>,
    /// 全エンジン合計の同時思考数の制限。
    ai_limiter: Semaphore,
}

impl Deps {
    pub fn new(pool: PgPool, config: Config) -> Deps {
        let engines = config
            .engines
            .iter()
            .map(|spec| {
                let runtime = EngineRuntime { spec: spec.clone(), limiter: Semaphore::new(spec.max_concurrent) };
                (spec.name.clone(), Arc::new(runtime))
            })
            .collect();
        let ai_limiter = Semaphore::new(config.ai_max_concurrent);
        Deps { pool, config, engines, ai_limiter }
    }
}

/// プレイヤーの操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Play(Coord),
    Pass,
    Resign,
    Undo,
}

struct Command {
    color: Color,
    action: Action,
    reply: oneshot::Sender<Result<(), GameError>>,
}

/// 対局アクターへの窓口。複製して接続ごとに持つ。すべて捨てられるとアクターは終了する。
#[derive(Clone)]
pub struct RoomHandle {
    pub meta: Arc<GameMeta>,
    commands: mpsc::Sender<Command>,
    state: watch::Sender<Arc<Snapshot>>,
}

impl RoomHandle {
    /// 現在の状態。
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.state.borrow().clone()
    }

    /// 状態の変化を受け取る。
    pub fn subscribe(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.state.subscribe()
    }

    /// 状態を受け取っている接続の数。
    pub fn subscriber_count(&self) -> usize {
        self.state.receiver_count()
    }

    /// 席トークンから席を求める。どの席のトークンでもなければ `None`。
    pub fn seat_for_token(&self, token: &str) -> Option<Color> {
        let hash = token::hash(token);
        Color::BOTH
            .into_iter()
            .find(|c| self.meta.token_hashes[c.index()].as_deref() == Some(hash.as_str()))
    }

    /// `color` の席として操作する。
    pub async fn act(&self, color: Color, action: Action) -> Result<(), GameError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command { color, action, reply })
            .await
            .map_err(|_| GameError::Internal)?;
        response.await.map_err(|_| GameError::Internal)?
    }
}

/// エンジンが返した手。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EngineMove {
    Move(Move),
    Resign,
}

#[derive(Debug, thiserror::Error)]
enum EngineError {
    #[error(transparent)]
    Gtp(#[from] GtpError),
    #[error("genmove の応答が着手として読めません: {0:?}")]
    BadMove(String),
    #[error("思考タスクが異常終了しました")]
    Crashed,
}

/// エンジンが座っている席。
struct EngineSeat {
    /// 設定から外されたエンジンなら `None`(その席は手を打てない)。
    runtime: Option<Arc<EngineRuntime>>,
    /// 対局ごとに保持する接続。
    conn: Option<GtpClient>,
    /// エンジンが知っている手数。`None` なら clear_board から同期し直す。
    synced: Option<usize>,
    last_used: Instant,
}

struct Thinking {
    color: Color,
    task: JoinHandle<TurnOutcome>,
}

struct TurnOutcome {
    conn: Option<GtpClient>,
    result: Result<EngineMove, EngineError>,
}

/// AI の 1 手ぶんの仕事。対局アクターとは別のタスクで動かす。
struct TurnJob {
    deps: Arc<Deps>,
    runtime: Arc<EngineRuntime>,
    conn: Option<GtpClient>,
    synced: Option<usize>,
    size: usize,
    komi: f64,
    moves: Vec<PlayedMove>,
    color: Color,
}

impl TurnJob {
    async fn run(mut self) -> TurnOutcome {
        let started = Instant::now();
        let (deps, runtime) = (self.deps.clone(), self.runtime.clone());
        let result = {
            // 同時に思考させる数を制限する(KataGo の CPU 版は重い)。
            let _all = deps.ai_limiter.acquire().await;
            let _engine = runtime.limiter.acquire().await;
            let mut result = self.attempt().await;
            if let Err(EngineError::Gtp(e)) = &result
                && e.is_connection_error()
            {
                // 接続が切れていたら、つなぎ直して clear_board から着手履歴を再生し、もう一度だけ試す。
                tracing::info!(engine = %self.runtime.spec.name, error = %e, "エンジンとの接続が切れたため、つなぎ直します");
                self.conn = None;
                result = self.attempt().await;
            }
            result
        };
        if result.is_err() {
            self.conn = None;
        }
        // 一瞬で返るエンジンでも、盤面の変化を目で追えるように少し待つ。
        let min = self.deps.config.ai_min_move;
        if result.is_ok() && started.elapsed() < min {
            tokio::time::sleep(min - started.elapsed()).await;
        }
        TurnOutcome { conn: self.conn, result }
    }

    async fn attempt(&mut self) -> Result<EngineMove, EngineError> {
        let timeout = self.deps.config.ai_move_timeout;
        let spec = &self.runtime.spec;
        if self.conn.is_none() {
            self.conn = Some(GtpClient::connect(&spec.host, spec.port, timeout).await?);
            self.synced = None;
        }
        let conn = self.conn.as_mut().expect("connected above");
        let size = self.size;
        let play = |m: &PlayedMove| format!("play {} {}", m.color, m.mv.to_gtp(size));

        match self.synced {
            // 同期済み:エンジンがまだ知らない手(相手の着手)だけを通知する。
            Some(known) if known <= self.moves.len() => {
                for m in &self.moves[known..] {
                    conn.command(&play(m), timeout).await?;
                }
            }
            // 新しい接続、待ったの後など:盤を初期化して着手履歴をすべて再生する。
            _ => {
                conn.command(&format!("boardsize {size}"), timeout).await?;
                conn.command(&format!("komi {}", self.komi), timeout).await?;
                conn.command("clear_board", timeout).await?;
                for m in &self.moves {
                    conn.command(&play(m), timeout).await?;
                }
            }
        }
        self.synced = Some(self.moves.len());

        let reply = conn.command(&format!("genmove {}", self.color), timeout).await?;
        if reply.eq_ignore_ascii_case("resign") {
            return Ok(EngineMove::Resign);
        }
        Move::from_gtp(&reply, size)
            .map(EngineMove::Move)
            .map_err(|_| EngineError::BadMove(reply))
    }
}

struct Room {
    meta: Arc<GameMeta>,
    deps: Arc<Deps>,
    game: Game,
    status: Status,
    result: Option<String>,
    state: watch::Sender<Arc<Snapshot>>,
    seats: [Option<EngineSeat>; 2],
    thinking: Option<Thinking>,
    /// DB への書き込みに失敗したあと、AI の手番をやり直す時刻。
    retry_at: Option<Instant>,
}

/// DB から読んだ対局のアクターを起動する。
pub fn spawn(stored: StoredGame, deps: Arc<Deps>) -> Result<RoomHandle, go_core::ReplayError> {
    let StoredGame { meta, status, result, moves } = stored;
    let game = Game::from_moves(meta.size, meta.komi, &moves)?;
    let meta = Arc::new(meta);

    let seats = Color::BOTH.map(|color| match meta.player(color) {
        Player::Human => None,
        Player::Engine(name) => {
            let runtime = deps.engines.get(name).cloned();
            if runtime.is_none() && status == Status::Playing {
                tracing::warn!(game = %meta.id, engine = %name, "設定にないエンジンの対局です。このエンジンは着手できません");
            }
            Some(EngineSeat { runtime, conn: None, synced: None, last_used: Instant::now() })
        }
    });

    let snapshot = Snapshot::build(&meta, &game, status, result.as_deref(), None);
    let (state, _) = watch::channel(Arc::new(snapshot));
    let (commands, receiver) = mpsc::channel(32);
    let room = Room {
        meta: meta.clone(),
        deps,
        game,
        status,
        result,
        state: state.clone(),
        seats,
        thinking: None,
        retry_at: None,
    };
    tokio::spawn(room.run(receiver));
    Ok(RoomHandle { meta, commands, state })
}

async fn join_thinking(thinking: &mut Option<Thinking>) -> Result<TurnOutcome, tokio::task::JoinError> {
    match thinking {
        Some(t) => (&mut t.task).await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

impl Room {
    async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        loop {
            self.close_idle_engines();
            self.start_engine_turn();
            let wake_at = self.next_wake();
            tokio::select! {
                command = commands.recv() => {
                    // すべての窓口が捨てられたら終了する。
                    let Some(Command { color, action, reply }) = command else { break };
                    let _ = reply.send(self.handle(color, action).await);
                }
                joined = join_thinking(&mut self.thinking) => {
                    let color = self.thinking.take().expect("a thinking task just finished").color;
                    self.on_engine_outcome(color, joined).await;
                }
                _ = sleep_until(wake_at) => {}
            }
        }
        if let Some(thinking) = self.thinking.take() {
            thinking.task.abort();
        }
    }

    fn publish(&self) {
        let thinking = self.thinking.as_ref().map(|t| t.color);
        let snapshot = Snapshot::build(&self.meta, &self.game, self.status, self.result.as_deref(), thinking);
        self.state.send_replace(Arc::new(snapshot));
    }

    // ───────────── プレイヤーの操作 ─────────────

    async fn handle(&mut self, color: Color, action: Action) -> Result<(), GameError> {
        if self.status == Status::Finished {
            return Err(GameError::GameOver);
        }
        match action {
            Action::Play(coord) => self.human_move(color, Move::Play(coord)).await,
            Action::Pass => self.human_move(color, Move::Pass).await,
            Action::Resign => self.finish(format!("{}+R", color.opponent())).await,
            Action::Undo => self.undo(color).await,
        }
    }

    async fn human_move(&mut self, color: Color, mv: Move) -> Result<(), GameError> {
        if !self.meta.player(color).is_human() || self.game.to_move() != color {
            return Err(GameError::NotYourTurn);
        }
        self.game.check(color, mv)?;
        self.apply_move(color, mv).await
    }

    /// 合法と確認済みの手を DB に記録し、盤面に適用して配信する。
    async fn apply_move(&mut self, color: Color, mv: Move) -> Result<(), GameError> {
        let mut next = self.game.clone();
        next.play_as(color, mv)?;
        // 両者が連続でパスしたら終局し、その時点の盤面をそのまま計算する。
        let result = next.is_over().then(|| next.result_by_score().to_string());
        db::record_move(&self.deps.pool, self.meta.id, next.move_number(), color, mv, result.as_deref())
            .await
            .map_err(|e| {
                tracing::error!(game = %self.meta.id, error = %e, "着手を記録できませんでした");
                GameError::Internal
            })?;
        self.game = next;
        if let Some(result) = result {
            tracing::info!(game = %self.meta.id, %result, "終局(連続パス)");
            self.status = Status::Finished;
            self.result = Some(result);
            self.release_engines();
        }
        self.publish();
        Ok(())
    }

    /// 対局を終了させる(投了、エンジンの反則負け)。
    async fn finish(&mut self, result: String) -> Result<(), GameError> {
        db::finish_game(&self.deps.pool, self.meta.id, &result).await.map_err(|e| {
            tracing::error!(game = %self.meta.id, error = %e, "終局を記録できませんでした");
            GameError::Internal
        })?;
        tracing::info!(game = %self.meta.id, %result, "終局");
        self.status = Status::Finished;
        self.result = Some(result);
        self.release_engines();
        self.publish();
        Ok(())
    }

    /// 待った。直前の人間の手まで盤面を戻し、そのあとの AI の応手も取り消す。
    async fn undo(&mut self, color: Color) -> Result<(), GameError> {
        if self.meta.undo_seat() != Some(color) {
            return Err(GameError::UndoNotAllowed);
        }
        let keep = self
            .game
            .moves()
            .iter()
            .rposition(|m| m.color == color)
            .ok_or(GameError::NothingToUndo)?;
        db::delete_moves_after(&self.deps.pool, self.meta.id, keep).await.map_err(|e| {
            tracing::error!(game = %self.meta.id, error = %e, "待ったを記録できませんでした");
            GameError::Internal
        })?;

        // AI が思考中なら打ち切る。接続は思考タスクと一緒に閉じ、次の手番でつなぎ直す。
        if let Some(thinking) = self.thinking.take() {
            thinking.task.abort();
        }
        let count = self.game.move_number() - keep;
        self.game.undo(count).expect("count does not exceed the history");
        // エンジンの盤面は、次の手番で clear_board から着手履歴を再生して同期し直す。
        for seat in self.seats.iter_mut().flatten() {
            seat.synced = None;
        }
        self.publish();
        Ok(())
    }

    // ───────────── AI の手番 ─────────────

    /// 次の手番がエンジンなら、思考タスクを起動する。
    fn start_engine_turn(&mut self) {
        if self.status != Status::Playing || self.thinking.is_some() {
            return;
        }
        if let Some(at) = self.retry_at {
            if Instant::now() < at {
                return;
            }
            self.retry_at = None;
        }
        let color = self.game.to_move();
        let Some(seat) = self.seats[color.index()].as_mut() else { return };
        let Some(runtime) = seat.runtime.clone() else { return };
        let job = TurnJob {
            deps: self.deps.clone(),
            runtime,
            conn: seat.conn.take(),
            synced: seat.synced.take(),
            size: self.game.size(),
            komi: self.game.komi(),
            moves: self.game.moves().to_vec(),
            color,
        };
        self.thinking = Some(Thinking { color, task: tokio::spawn(job.run()) });
        self.publish();
    }

    async fn on_engine_outcome(&mut self, color: Color, joined: Result<TurnOutcome, tokio::task::JoinError>) {
        let outcome = joined.unwrap_or_else(|e| {
            tracing::error!(game = %self.meta.id, error = %e, "思考タスクが異常終了しました");
            TurnOutcome { conn: None, result: Err(EngineError::Crashed) }
        });
        let engine = self.meta.player(color).as_str().to_string();
        {
            let seat = self.seats[color.index()].as_mut().expect("engine seat");
            seat.conn = outcome.conn;
            seat.synced = None;
            seat.last_used = Instant::now();
        }

        // 返ってきた手を go-core で検証してから盤面に適用する。
        // 不正手・タイムアウト・その他のエラーはログに記録し、そのエンジン側の投了として扱う。
        let forfeit = match outcome.result {
            Ok(EngineMove::Resign) => {
                tracing::info!(game = %self.meta.id, %engine, "エンジンが投了しました");
                true
            }
            Ok(EngineMove::Move(mv)) => match self.game.check(color, mv) {
                Ok(()) => {
                    match self.apply_move(color, mv).await {
                        // エンジンは自分の手を盤面に適用済みなので、ここまで同期できている。
                        Ok(()) => {
                            if let Some(seat) = self.seats[color.index()].as_mut() {
                                seat.synced = Some(self.game.move_number());
                            }
                        }
                        // DB の障害。少し待ってから、この手番をやり直す。
                        Err(_) => self.retry_at = Some(Instant::now() + Duration::from_secs(3)),
                    }
                    false
                }
                Err(reason) => {
                    tracing::warn!(
                        game = %self.meta.id, %engine, color = %color,
                        mv = %mv.to_gtp(self.game.size()), %reason,
                        "エンジンが不正手を返しました。エンジン側の投了として扱います"
                    );
                    true
                }
            },
            Err(error) => {
                tracing::warn!(
                    game = %self.meta.id, %engine, color = %color, %error,
                    "エンジンから手を得られませんでした。エンジン側の投了として扱います"
                );
                true
            }
        };
        if forfeit && self.finish(format!("{}+R", color.opponent())).await.is_err() {
            self.retry_at = Some(Instant::now() + Duration::from_secs(3));
        }
        // 思考中の表示を消す(着手や終局で配信済みでも、もう一度配信して構わない)。
        self.publish();
    }

    /// 終局したら、エンジンとの接続を閉じる。
    fn release_engines(&mut self) {
        if let Some(thinking) = self.thinking.take() {
            thinking.task.abort();
        }
        for seat in self.seats.iter_mut().flatten() {
            seat.synced = None;
            if let Some(conn) = seat.conn.take() {
                tokio::spawn(conn.quit());
            }
        }
    }

    /// 長く使われていない接続を閉じる(エンジンのプロセスを抱え続けないため)。
    /// 次の手番でつなぎ直し、clear_board から同期し直す。
    fn close_idle_engines(&mut self) {
        let idle = self.deps.config.engine_idle;
        let now = Instant::now();
        for seat in self.seats.iter_mut().flatten() {
            if seat.conn.is_some() && now.duration_since(seat.last_used) >= idle {
                tracing::debug!(game = %self.meta.id, "使われていないエンジン接続を閉じます");
                seat.synced = None;
                if let Some(conn) = seat.conn.take() {
                    tokio::spawn(conn.quit());
                }
            }
        }
    }

    /// 次に自分から起きる必要のある時刻(接続のアイドル切断、AI の手番のやり直し)。
    fn next_wake(&self) -> Option<Instant> {
        let idle = self.deps.config.engine_idle;
        self.seats
            .iter()
            .flatten()
            .filter(|seat| seat.conn.is_some())
            .map(|seat| seat.last_used + idle)
            .chain(self.retry_at)
            .min()
    }
}
