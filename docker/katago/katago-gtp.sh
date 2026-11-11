#!/bin/sh
# KataGo を GTP モードで起動する。重みはイメージに入っている 1 つを使う。
# 思考量は環境変数で上書きできる(compose の設定だけで調整できるようにするため)。
set -eu
model="$(ls /opt/katago/model/* | head -n 1)"
exec katago gtp \
  -model "$model" \
  -config /opt/katago/gtp.cfg \
  -override-config "maxVisits=${KATAGO_MAX_VISITS:-400},maxTime=${KATAGO_MAX_TIME:-3.0},numSearchThreads=${KATAGO_THREADS:-4}"
