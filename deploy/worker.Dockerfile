# syntax=docker/dockerfile:1.7
FROM rust:1-bookworm AS builder

ARG HIRSEL_WORKER_CARGO_PROFILE=release

WORKDIR /build
ENV CARGO_TARGET_DIR=/build/target

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

COPY src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs ./src-tauri/
COPY src-tauri/src ./src-tauri/src

WORKDIR /build/src-tauri

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/git/checkouts \
    --mount=type=cache,target=/build/target \
    if [ "$HIRSEL_WORKER_CARGO_PROFILE" = "release" ]; then \
        cargo build --release --locked --no-default-features --bin hirsel-worker && \
        cp /build/target/release/hirsel-worker /tmp/hirsel-worker; \
    else \
        cargo build --locked --no-default-features --bin hirsel-worker && \
        cp /build/target/debug/hirsel-worker /tmp/hirsel-worker; \
    fi

FROM ubuntu:24.04

ARG HIRSEL_WORKER_BUILD_LABEL=dev
LABEL org.hirsel.worker-build="${HIRSEL_WORKER_BUILD_LABEL}"

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    git \
    passwd \
    xz-utils \
    libssl3 \
    zlib1g \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/hirsel-worker /usr/local/bin/hirsel-worker

RUN chmod +x /usr/local/bin/hirsel-worker

CMD ["/usr/local/bin/hirsel-worker", "--build-info"]
