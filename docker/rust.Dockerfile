# syntax=docker/dockerfile:1
# server と go-engine(自作エンジン)のイメージ。ビルドは共通で、ターゲットで分ける。
#   docker build -f docker/rust.Dockerfile --target server .
#   docker build -f docker/rust.Dockerfile --target engine-self .

FROM rust:1.93.1-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p server -p go-engine \
 && cp target/release/server target/release/go-engine /usr/local/bin/

FROM debian:trixie-20261104-slim AS server
RUN useradd --system --no-create-home igo
COPY --from=build /usr/local/bin/server /usr/local/bin/server
USER igo
ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["server"]

# 自作エンジン。TCP 接続ごとに go-engine を 1 プロセス起動し、標準入出力をその接続につなぐ。
FROM debian:trixie-20261104-slim AS engine-self
RUN apt-get update \
 && apt-get install -y --no-install-recommends socat \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --no-create-home igo
COPY --from=build /usr/local/bin/go-engine /usr/local/bin/go-engine
COPY docker/gtp-listen.sh /usr/local/bin/gtp-listen
USER igo
ENV GTP_PORT=7001 \
    GTP_MAX_CONNECTIONS=32 \
    ENGINE_COMMAND="go-engine --mode mcts --time-ms 2000"
EXPOSE 7001
CMD ["gtp-listen"]
