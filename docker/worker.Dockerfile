# Hirsel Worker Base Image
# Use this as the container image for docker runners:
#   [runners.docker.container]
#   image = "ghcr.io/samgalanakis/hirsel-worker:latest"
#
# Or build locally:
#   docker build -t hirsel-worker -f docker/worker.Dockerfile .

FROM ubuntu:24.04

# Install required tools (curl, git, ca-certificates for HTTPS)
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

# Set up PATH to include /tmp/bin where the init script will install hirsel and claude
# NOTE: Don't create /tmp/bin here - it will be created by init script as the running user
ENV PATH="/tmp/bin:$PATH"
