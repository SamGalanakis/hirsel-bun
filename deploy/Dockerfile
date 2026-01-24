# Hirsel Server Docker Image
# Multi-stage build: compile from source, then copy to slim runtime image

# ==============================================================================
# Stage 1: Build the Rust binary
# ==============================================================================
FROM rustlang/rust:nightly-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Copy source files
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/
COPY src-tauri/src ./src-tauri/src

# Create stub for optional tauri-plugin-mcp (not needed for server build)
RUN mkdir -p tauri-plugin-mcp/src && \
    echo '[package]\nname = "tauri-plugin-mcp"\nversion = "0.1.0"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"' > tauri-plugin-mcp/Cargo.toml && \
    echo '' > tauri-plugin-mcp/src/lib.rs

# Build server-only binary (cli feature: server + TUI, no GUI)
RUN cargo build --release --no-default-features --features cli --manifest-path src-tauri/Cargo.toml

# ==============================================================================
# Stage 2: Runtime image
# ==============================================================================
FROM node:22-bookworm-slim

# Install dependencies and ACP adapters
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g @zed-industries/claude-code-acp @zed-industries/codex-acp

# Create non-root user for security
RUN useradd -m -s /bin/bash hirsel

# Copy binary from builder
COPY --from=builder /build/src-tauri/target/release/hirsel /usr/local/bin/
RUN chmod +x /usr/local/bin/hirsel

# Create data directory
RUN mkdir -p /home/hirsel/.hirsel && chown -R hirsel:hirsel /home/hirsel

USER hirsel
WORKDIR /home/hirsel

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

CMD ["hirsel", "serve", "--port", "8080"]
