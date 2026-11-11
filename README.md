# 囲碁対局

ブラウザで遊べる囲碁対戦アプリです。

- オンラインでの人間同士の対局(招待 URL を相手に送る)
- 人間対 AI の対局(待ったができる)
- AI 同士の対局(観戦用)

AI は GTP で差し替えられます。自作エンジン(MCTS 版とランダム版)と KataGo を同梱しています。

## 起動

```bash
docker compose up --build
```

起動したら <http://localhost:8080> を開きます。初回は KataGo のビルドとネットワーク重みの取得に数分かかります。

| 環境変数 | 既定値 | 内容 |
|---|---|---|
| `IGO_PORT` | `8080` | 公開するポート |
| `AI_MAX_CONCURRENT` | `2` | 全エンジン合計で、同時に思考させる数の上限 |
| `AI_MOVE_TIMEOUT_SECS` | `60` | GTP コマンド 1 つの応答を待つ秒数 |
| `SELF_TIME_MS` | `2000` | 自作エンジン(MCTS)の 1 手あたりの思考時間 |
| `KATAGO_MAX_VISITS` / `KATAGO_MAX_TIME` / `KATAGO_THREADS` | `400` / `3.0` / `4` | KataGo の思考量 |
| `POSTGRES_PASSWORD` | `igo` | DB のパスワード(外部に公開する場合は必ず変更する) |

## ルール

- 中国ルール(面積計算)。得点は「盤上の自分の石」と「自分の石だけに囲まれた空点」の合計で、白にコミを加えます。
- コミは 7.5 目が既定で、対局作成時に変更できます。同点が起きないよう、`x.5` の値だけを受け付けます。
- 自殺手は禁止です(相手の石を取れる手は自殺手ではありません)。
- 同形反復は禁止です(ポジショナル・スーパーコウ。手番を問わず、過去の盤面の再現を禁止)。
- パスは常に合法です。両者が続けてパスすると終局し、**盤上の石はすべて生きている石として数えます**。死石の判定や除去は行わないので、死石は取り切ってからパスしてください。
- 結果は `B+3.5`、`W+R`(R は投了)の形式で表記します。

## 構成

```
crates/go-core     ルールエンジン(I/O・非同期・DB に依存しない純粋なロジック)
crates/go-engine   自作の思考エンジン(標準入出力で GTP を話す)と、対戦ツール gtp-match
crates/server      REST と WebSocket、対局管理、DB への永続化、GTP クライアント
web                フロントエンド(React + TypeScript + Vite。盤は SVG)
docker             Dockerfile と nginx・KataGo の設定
```

`docker compose` のサービス:

| サービス | 内容 |
|---|---|
| `db` | PostgreSQL |
| `server` | REST / WebSocket サーバー。起動時にマイグレーション(sqlx migrate)を適用する |
| `engine-self` | 自作エンジン(MCTS)。TCP 接続ごとに `go-engine` を 1 プロセス起動する(socat) |
| `engine-random` | 自作エンジンのランダム版(同じイメージを起動引数だけ変えて使う) |
| `engine-katago` | KataGo の CPU 版(Eigen)。同じ方式で GTP を TCP で公開する |
| `web` | ビルド済みの React を配信し、`/api` と `/ws` を `server` へ中継する(nginx) |

### エンジンの追加・差し替え

エンジンは「TCP で GTP を話す相手」であれば何でも構いません。`docker-compose.yml` にサービスを追加し、
`server` の環境変数 `ENGINES`(JSON)に 1 行足すだけで使えるようになります。

```json
{"name": "gnugo", "label": "GNU Go", "host": "engine-gnugo", "port": 7003, "max_concurrent": 2, "max_board_size": 19}
```

`ENGINES` の代わりに、同じ JSON を書いたファイルのパスを `ENGINES_FILE` で指定することもできます。

### AI の同時実行数

KataGo の CPU 版は重いため、同時に思考する AI の数を制限しています。全体の上限が `AI_MAX_CONCURRENT`、
エンジンごとの上限が `ENGINES` の `max_concurrent` です。上限を超えた対局は拒否されず、順番待ちになります。
また、10 分間使われなかったエンジン接続(= エンジンのプロセス)は閉じ、次の手番でつなぎ直して着手履歴を再生します。

### AI の扱い

- AI の手番では `genmove` を送り、返ってきた手を go-core で検証してから盤面に適用します。
- 不正手・タイムアウト・接続不能は、ログに記録したうえで、そのエンジン側の投了として扱います。
- 待ったや再接続のあとは、`clear_board` から着手履歴を再生して同期し直します。

## 使っているもののバージョン

| 対象 | バージョン | 確認元 |
|---|---|---|
| Rust(ビルド用イメージ) | `rust:1.93.1-slim-trixie` | Docker Hub 公式イメージ |
| 実行用イメージ | `debian:trixie-20261104-slim` | Docker Hub 公式イメージ |
| PostgreSQL | `postgres:17.11-alpine` | Docker Hub 公式イメージ |
| Node.js(ビルド用) | `node:22.22.3-alpine3.23` | Docker Hub 公式イメージ |
| nginx | `nginx:1.30.4-alpine` | Docker Hub 公式イメージ |
| KataGo | `v1.18.1`(コミット `92ee95c0`)をソースビルド | [公式リリース](https://github.com/lightvector/KataGo/releases/tag/v1.18.1) |
| KataGo のネットワーク重み | `kata1-b15c192-s1672170752-d466197061`(SHA-256 を検証) | [katagotraining.org](https://katagotraining.org/networks/) |

Rust と npm のライブラリは `Cargo.lock` と `web/package-lock.json` で固定しています。

- KataGo はソースからビルドしています。公式リリースの Linux バイナリは x64 のみで、arm64(Apple Silicon)では動かないためです。
  v1.18.2 は CUDA 向けの最適化だけなので、CPU 版では v1.18.1 が最新です。
- sqlx は 0.8.6 です。0.9.0 は Rust 1.94 以上を要求します。
- KataGo のルール設定は `docker/katago/gtp.cfg` にあります。面積計算・ポジショナルスーパーコウ・自殺手不許可に加えて、
  死石を取り切る前にパスしないよう `friendlyPassOk = false` を指定しています。

## 開発

必要なもの:Rust 1.88 以上、Node.js 22.12 以上、Docker。

```bash
# テスト用・開発用の PostgreSQL(127.0.0.1:55432)
scripts/dev-db.sh

# サーバー(127.0.0.1:18080)
scripts/dev-server.sh

# フロントエンド(http://localhost:5173。/api と /ws をサーバーへ中継する)
cd web && npm ci && npm run dev
```

### テスト

```bash
# ルールエンジンと自作エンジン
cargo test -p go-core -p go-engine

# サーバー(実物の PostgreSQL と、TCP で GTP を話す偽のエンジンを使う)
TEST_DATABASE_URL=postgres://igo:igo@127.0.0.1:55432/igo cargo test -p server

# フロントエンド
cd web && npm test && npm run build
```

### 自作エンジンを手で動かす

```bash
printf 'boardsize 9\nplay b E5\ngenmove w\nshowboard\nquit\n' | cargo run -q --release -p go-engine -- --mode mcts --time-ms 1000
```

対応している GTP コマンド:`protocol_version`、`name`、`version`、`known_command`、`list_commands`、`quit`、
`boardsize`、`clear_board`、`komi`、`play`、`genmove`、`undo`、`showboard`、`final_score`。

### エンジン同士を対戦させる

```bash
cargo build --release -p go-engine
target/release/gtp-match \
  --a "target/release/go-engine --mode mcts --time-ms 1000" \
  --b "target/release/go-engine --mode random" \
  --games 20 --size 9
```

## API

| メソッド | パス | 内容 |
|---|---|---|
| `GET` | `/api/engines` | 対局相手に選べるエンジンの一覧 |
| `POST` | `/api/games` | 対局を作成する。`{"board_size": 9, "komi": 7.5, "black": "human", "white": "katago"}` |
| `GET` | `/api/games/{id}` | 対局の状態 |
| `GET` | `/api/games/{id}/sgf` | SGF のダウンロード |
| `GET` | `/ws/games/{id}` | WebSocket |

WebSocket のメッセージ(JSON):

- クライアント → サーバー:`{"type":"join","token":"…"}`、`{"type":"play","x":3,"y":4}`、`{"type":"pass"}`、`{"type":"resign"}`、`{"type":"undo"}`
- サーバー → クライアント:`{"type":"state", …}`(盤面・手番・手数・最終手・アゲハマ・対局状態・結果)、`{"type":"error","code":"superko","message":"…"}`

ログイン機能はありません。対局作成時に人間の各席へ席トークンを発行し、ブラウザが `localStorage` に保存して再接続に使います。
DB にはトークンのハッシュだけを保存します。トークンなしで対局 URL を開いた人は観戦者になります。

## 初版の範囲外

ユーザー登録・ログイン、持ち時間、レーティング、対局相手のマッチング、SGF の読み込み。
