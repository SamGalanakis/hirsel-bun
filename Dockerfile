# Hirsel - Docker image for server/worker deployment
#
# Pre-built binary from CI: docker/hirsel-linux-amd64
#
# Build locally (after cargo build --release --no-default-features --features cli):
#   cp target/release/hirsel docker/hirsel-linux-amd64
#   docker build -t hirsel .
#
# Run as server:
#   docker run -p 3000:3000 -e HIRSEL_API_KEY=secret hirsel serve --port 3000
#
# Run as worker (connects to remote orchestrator):
#   docker run -e ANTHROPIC_API_KEY=... hirsel __remote-worker ...

FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    openssh-client \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash hirsel

# Set working directory
WORKDIR /app

# Copy binary
COPY docker/hirsel-linux-amd64 /usr/local/bin/hirsel
RUN chmod +x /usr/local/bin/hirsel

# Create data directory
RUN mkdir -p /home/hirsel/.hirsel && chown -R hirsel:hirsel /home/hirsel

# Switch to non-root user
USER hirsel

# Default environment
ENV HOME=/home/hirsel
ENV HIRSEL_DATA_DIR=/home/hirsel/.hirsel

# Health check for server mode
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD hirsel daemon status || exit 1

# Default to showing help
ENTRYPOINT ["hirsel"]
CMD ["--help"]
