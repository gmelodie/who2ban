FROM rust:1.95-slim AS build

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir -p /out \
    && cargo build --release -p hots-web \
    && cp target/release/hots-web /out/

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data hots \
    && mkdir -p /data \
    && chown hots /data

COPY --from=build /out/hots-web /usr/local/bin/hots-web

USER hots
ENV HOTS_ADDR=0.0.0.0:8731 HOTS_DATA_DIR=/data
EXPOSE 8731
VOLUME ["/data"]
CMD ["hots-web"]
