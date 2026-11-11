-- 対局。盤面そのものは保存せず、moves の着手履歴から再構築する。
CREATE TABLE games (
    id               UUID PRIMARY KEY,
    board_size       SMALLINT NOT NULL CHECK (board_size BETWEEN 2 AND 25),
    -- コミは x.5 のみ(同点が起こらないようにする)。
    komi             DOUBLE PRECISION NOT NULL CHECK (komi * 2 = floor(komi * 2) AND komi <> floor(komi)),
    status           TEXT NOT NULL DEFAULT 'playing' CHECK (status IN ('playing', 'finished')),
    -- 'human' またはエンジン名。
    black_player     TEXT NOT NULL,
    white_player     TEXT NOT NULL,
    -- 席トークンの SHA-256(16 進)。トークンそのものは保存しない。エンジンの席は NULL。
    black_token_hash TEXT,
    white_token_hash TEXT,
    -- 'B+3.5'、'W+R' など。対局中は NULL。
    result           TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at      TIMESTAMPTZ,
    CHECK ((status = 'finished') = (result IS NOT NULL)),
    CHECK ((status = 'finished') = (finished_at IS NOT NULL)),
    CHECK ((black_player = 'human') = (black_token_hash IS NOT NULL)),
    CHECK ((white_player = 'human') = (white_token_hash IS NOT NULL))
);

CREATE INDEX games_status_idx ON games (status);

-- 着手履歴。パスは x, y が NULL。投了は着手ではないので、ここには入らない(games.result に残る)。
CREATE TABLE moves (
    game_id     UUID NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    move_number INTEGER NOT NULL CHECK (move_number >= 1),
    color       TEXT NOT NULL CHECK (color IN ('B', 'W')),
    x           SMALLINT CHECK (x >= 0),
    y           SMALLINT CHECK (y >= 0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (game_id, move_number),
    CHECK ((x IS NULL) = (y IS NULL))
);
