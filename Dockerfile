FROM rust:1.95-slim AS build

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir -p /out \
    && cargo build --release -p hots-web \
    && cargo build --release -p hots-parse --target wasm32-unknown-unknown \
    && cp target/release/hots-web /out/ \
    && cp target/wasm32-unknown-unknown/release/hots_parse.wasm /out/

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data hots \
    && mkdir -p /data \
    && chown hots /data

# hots-web reads the wasm module from its own folder.
COPY --from=build /out/hots-web /usr/local/bin/hots-web
COPY --from=build /out/hots_parse.wasm /usr/local/bin/hots_parse.wasm

USER hots
ENV HOTS_ADDR=0.0.0.0:8731 HOTS_DATA_DIR=/data
EXPOSE 8731
VOLUME ["/data"]
CMD ["hots-web"]
