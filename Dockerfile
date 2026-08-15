# Stage 1: Builder
# Keep the container toolchain aligned with the resolved dependency graph.
# wasmtime 44 / cranelift 0.131 require Rust 1.92 or newer.
FROM rust:1.93-slim AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    g++ \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin zaion --locked

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    passwd \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 zaion \
    && useradd --uid 10001 --gid 10001 --create-home \
        --home-dir /home/zaion --shell /usr/sbin/nologin zaion \
    && install -d -m 0700 -o zaion -g zaion \
        /var/lib/zaion /var/lib/zaion/data

COPY --from=builder /build/target/release/zaion /usr/local/bin/zaion

# Non-loopback startup is fail-closed unless the operator supplies a strong
# ZAION_GATEWAY_TOKEN at runtime.
ENV HOME=/home/zaion \
    ZAION_HOME=/var/lib/zaion \
    ZAION_DATA_DIR=/var/lib/zaion/data \
    ZAION_GATEWAY_BIND=0.0.0.0:7821

VOLUME ["/var/lib/zaion"]
WORKDIR /home/zaion

EXPOSE 7821

HEALTHCHECK --interval=30s --timeout=3s --start-period=15s --retries=3 \
    CMD zaion gateway health | grep -F "gateway health: verified" >/dev/null || exit 1

STOPSIGNAL SIGTERM
USER 10001:10001

ENTRYPOINT ["zaion"]
CMD ["_daemon_run"]
