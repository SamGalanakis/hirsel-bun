# Hirsel Server Deployment

Run `hirsel-server` on a Linux host and let it launch containerized workers for shepherd and threads.

## Requirements

- Linux host
- Docker daemon reachable from the server process
- A project `flake.nix` for normal thread execution
- `HIRSEL_API_KEY` set for client authentication

## Quick Start

From the repo checkout:

```bash
HIRSEL_API_KEY=replace-me cargo run \
  -p hirsel-cli \
  --bin hirsel-server \
  -- --port 8080
```

Or build a release binary first:

```bash
cargo build -p hirsel-cli --release --bin hirsel-server
HIRSEL_API_KEY=replace-me ./target/release/hirsel-server --port 8080
```

## Worker Image

The default worker image is `hirsel-worker:local`.

When `hirsel-server` is running from this repo, it can build that image automatically from [worker.Dockerfile](/home/sam/code/hirsel/deploy/worker.Dockerfile) on first use.

You can also build it yourself:

```bash
DOCKER_BUILDKIT=1 docker build -f deploy/worker.Dockerfile -t hirsel-worker:local .
```

If you want a different worker image, set it in `~/.hirsel/config.toml`:

```toml
[sandbox]
image = "your-worker-image:tag"
```

Project creation and project settings also let you override the worker image per project.

## Verify

```bash
curl http://127.0.0.1:8080/health
docker info >/dev/null && echo ok
```

## Client Setup

Point the desktop shell at the backend URL and API key:

```toml
[backend]
url = "http://backend.example.internal:8080"
api_key = "your-api-key"
```
