# syntax=docker/dockerfile:1

# Hirsel backend image.
#
# This image builds and runs the headless `hirsel serve` binary only.
# It intentionally does not bundle legacy ACP adapters or the Tauri GUI stack.

FROM rust:1-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Copy only the server crate sources needed for a headless build.
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs ./src-tauri/
COPY src-tauri/src ./src-tauri/src

WORKDIR /build/src-tauri

RUN cargo build --release --no-default-features --features server


FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    libssl3 \
    openssh-client \
    tini \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --home-dir /home/hirsel --shell /bin/sh hirsel \
    && mkdir -p /data \
    && chown -R hirsel:hirsel /data /home/hirsel

COPY --from=builder /build/src-tauri/target/release/hirsel /usr/local/bin/hirsel

ENV HOME=/home/hirsel
ENV HIRSEL_ROOT=/data

USER hirsel
WORKDIR /home/hirsel

VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl --fail --silent http://127.0.0.1:8080/health >/dev/null || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["hirsel", "serve", "--port", "8080"]
