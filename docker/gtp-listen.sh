#!/bin/sh
# GTP エンジンを TCP で公開する。
# 接続ごとに ENGINE_COMMAND を 1 プロセス起動し、その標準入出力を接続につなぐ。
# 接続が切れるとエンジンの標準入力が閉じ、プロセスは終了する。
set -eu
: "${GTP_PORT:?GTP_PORT を指定してください}"
: "${ENGINE_COMMAND:?ENGINE_COMMAND を指定してください}"
echo "GTP を TCP ${GTP_PORT} 番で待ち受けます: ${ENGINE_COMMAND}" >&2
exec socat \
  "TCP-LISTEN:${GTP_PORT},reuseaddr,fork,max-children=${GTP_MAX_CONNECTIONS:-32},keepalive" \
  "EXEC:${ENGINE_COMMAND}"
