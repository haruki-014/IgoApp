#!/usr/bin/env bash
# 開発用:server をローカルで起動する(DB は scripts/dev-db.sh で起動しておく)。
# フロントエンドは web/ で `npm run dev`(/api と /ws をこのサーバーへ中継する)。
set -euo pipefail
cd "$(dirname "$0")/.."

export DATABASE_URL="${DATABASE_URL:-postgres://igo:igo@127.0.0.1:55432/igo}"
export BIND_ADDR="${BIND_ADDR:-127.0.0.1:18080}"
# エンジンをローカルで動かしている場合は ENGINES を指定する。例:
#   ENGINES='[{"name":"self","label":"自作エンジン","host":"127.0.0.1","port":7001}]'
export ENGINES="${ENGINES:-[]}"

exec cargo run -p server
