# syntax=docker/dockerfile:1
# KataGo の CPU 版(Eigen バックエンド)。公式リポジトリのタグからソースビルドする。
# 公式リリースの Linux バイナリは x64 のみなので、arm64(Apple Silicon など)でも
# 同じ手順で動くようにソースからビルドしている。
#
# ビルド手順は公式の Compiling.md(v1.18.1)に従う:
#   https://github.com/lightvector/KataGo/blob/v1.18.1/Compiling.md
# ネットワーク重みは公式の配布サイトから取得する:
#   https://katagotraining.org/networks/

FROM debian:trixie-20261104-slim AS build
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl git cmake make g++ libeigen3-dev zlib1g-dev libzip-dev \
 && rm -rf /var/lib/apt/lists/*

# CUDA 以外のバックエンドでは v1.18.1 が最新(v1.18.2 は CUDA 向けの最適化のみ)。
ARG KATAGO_VERSION=v1.18.1
ARG KATAGO_COMMIT=92ee95c0a4b25fec214da00951ab69e97e207729
RUN git clone --depth 1 --branch "${KATAGO_VERSION}" https://github.com/lightvector/KataGo.git /katago \
 && test "$(git -C /katago rev-parse HEAD)" = "${KATAGO_COMMIT}"

# x86_64 では公式手順どおり AVX2 を有効にする(2013 年以降の CPU が必要)。
# 古い CPU で動かす場合は --build-arg KATAGO_USE_AVX2=0 を指定する。
ARG KATAGO_USE_AVX2=auto
RUN cd /katago/cpp \
 && avx2=0 \
 && if [ "${KATAGO_USE_AVX2}" = "1" ] || { [ "${KATAGO_USE_AVX2}" = "auto" ] && [ "$(uname -m)" = "x86_64" ]; }; then avx2=1; fi \
 && cmake . -DUSE_BACKEND=EIGEN -DUSE_AVX2="${avx2}" -DCMAKE_BUILD_TYPE=Release \
 && make -j"$(nproc)" \
 && ./katago version

# ネットワーク重み。CPU で 1 手数秒に収めるため、小さめのネットワークを使う。
ARG KATAGO_MODEL_URL=https://media.katagotraining.org/uploaded/networks/models/kata1/kata1-b15c192-s1672170752-d466197061.txt.gz
ARG KATAGO_MODEL_SHA256=09456899f1b9155217f4ca059cf8a68f79b7eba22cf6b7c00ffb7f17ce067967
RUN mkdir /model \
 && cd /model \
 && curl -fsSL -A curl -O "${KATAGO_MODEL_URL}" \
 && echo "${KATAGO_MODEL_SHA256}  $(basename "${KATAGO_MODEL_URL}")" | sha256sum -c -

FROM debian:trixie-20261104-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends socat libzip5 zlib1g libgomp1 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --create-home katago
COPY --from=build /katago/cpp/katago /usr/local/bin/katago
COPY --from=build /model /opt/katago/model
COPY docker/katago/gtp.cfg /opt/katago/gtp.cfg
COPY docker/katago/katago-gtp.sh /usr/local/bin/katago-gtp
COPY docker/gtp-listen.sh /usr/local/bin/gtp-listen
USER katago
WORKDIR /home/katago
ENV GTP_PORT=7002 \
    GTP_MAX_CONNECTIONS=4 \
    ENGINE_COMMAND="katago-gtp"
EXPOSE 7002
CMD ["gtp-listen"]
