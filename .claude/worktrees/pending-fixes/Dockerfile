FROM rust:1.95-slim AS build

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir -p /out \
    && cargo build --release -p w2b-web \
    && cp target/release/w2b-web /out/

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data w2b \
    && mkdir -p /data \
    && chown w2b /data

COPY --from=build /out/w2b-web /usr/local/bin/w2b-web

USER w2b
ENV W2B_ADDR=0.0.0.0:8731 W2B_DATA_DIR=/data
EXPOSE 8731
VOLUME ["/data"]
CMD ["w2b-web"]
