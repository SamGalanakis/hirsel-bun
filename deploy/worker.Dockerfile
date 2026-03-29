FROM rust:1-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

COPY src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs ./src-tauri/
COPY src-tauri/src ./src-tauri/src

WORKDIR /build/src-tauri

RUN cargo build --release --locked --bin hirsel-worker

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

COPY --from=builder /build/src-tauri/target/release/hirsel-worker /usr/local/bin/hirsel-worker

RUN chmod +x /usr/local/bin/hirsel-worker

CMD ["/usr/local/bin/hirsel-worker", "--build-info"]
