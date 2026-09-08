FROM rust:1.95-slim AS build

# musl-tools carries musl-gcc, which the bundled sqlite is compiled with. Everything else
# in the server is Rust, and nothing in it opens a TLS connection or resolves a name, so
# the whole program links statically and the image below can hold nothing but the binary.
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-unknown-linux-musl

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir -p /out/data \
    && cargo build --release --target x86_64-unknown-linux-musl -p w2b-web \
    && cp target/x86_64-unknown-linux-musl/release/w2b-web /out/

# Nothing but the program. No shell, no package manager, no libc, no setuid binary and
# nothing on disk to execute, so a flaw that gets the process to run a command has no
# command to run. It was a debian-slim before, which is a working userland handed to
# whoever finds a way to ask for it.
#
# The cost is that `docker compose exec` cannot open a shell in here, because there is
# none. `docker inspect` answers what the process is running as, and the program says the
# rest in its log.
FROM scratch

# Carried in with its ownership set, because docker takes a named volume's ownership from
# the image directory it shadows. The uid is a bare number: there is no /etc/passwd to
# give it a name, and the kernel has never needed one.
COPY --from=build --chown=10001:10001 /out/data /data
COPY --from=build /out/w2b-web /w2b-web

USER 10001:10001
ENV W2B_ADDR=0.0.0.0:8731 W2B_DATA_DIR=/data
EXPOSE 8731
VOLUME ["/data"]
ENTRYPOINT ["/w2b-web"]
