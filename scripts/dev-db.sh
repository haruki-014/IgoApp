#!/usr/bin/env bash
# 開発・テスト用の PostgreSQL を Docker で起動する(127.0.0.1:55432)。
set -euo pipefail

if docker ps --format '{{.Names}}' | grep -qx igo-dev-db; then
  echo "igo-dev-db はすでに起動しています"
elif docker ps -a --format '{{.Names}}' | grep -qx igo-dev-db; then
  docker start igo-dev-db
else
  docker run -d --name igo-dev-db \
    -e POSTGRES_USER=igo -e POSTGRES_PASSWORD=igo -e POSTGRES_DB=igo \
    -p 127.0.0.1:55432:5432 postgres:17.11-alpine
fi
echo "DATABASE_URL=postgres://igo:igo@127.0.0.1:55432/igo"
