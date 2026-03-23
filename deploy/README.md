# Hirsel Server Deployment

Deploy Hirsel as a headless backend server.

## What this Docker setup is

This image runs the server binary only:

- `hirsel serve`
- local Hirsel state under `/data`
- basic host tools needed by the backend (`git`, `ssh`, `curl`)

It does **not** bundle the old ACP adapters or the Tauri GUI stack.

## Network architecture

Hirsel expects the backend to be reachable at a stable URL from client devices:

```text
Desktop / phone app ──┐
                      │ your network / VPN / reverse proxy
Backend host ─────────┘  backend.example.internal:8080
```

Clients connect to whatever URL you provide. Hirsel does not manage the network layer itself.

## Quick start with Docker Compose

```bash
git clone https://github.com/SamGalanakis/hirsel.git
cd hirsel/deploy

cp .env.example .env
# Edit .env and set HIRSEL_API_KEY (generate with: openssl rand -hex 32)

docker compose up -d --build
```

First build compiles the Rust server binary, so it takes a few minutes.

## Direct `docker run`

If you do not want Compose:

```bash
docker build -f deploy/Dockerfile -t hirsel-server .

docker volume create hirsel-data

docker run -d \
  --name hirsel \
  --restart unless-stopped \
  -p 8080:8080 \
  -e HIRSEL_API_KEY=replace-me \
  -e HIRSEL_ROOT=/data \
  -v hirsel-data:/data \
  hirsel-server
```

## Verify

```bash
curl http://backend.example.internal:8080/health
```

## Client configuration

On each client device, configure the backend target in `~/.hirsel/config.toml`:

```toml
[backend]
url = "http://backend.example.internal:8080"
api_key = "your-api-key"
```

## Environment variables

| Variable | Description |
|----------|-------------|
| `HIRSEL_API_KEY` | API key for authenticating client requests. Required. |
| `HIRSEL_ROOT` | Server data directory inside the container. Defaults to `/data`. |

## Data and backups

This deployment stores Hirsel state in the `hirsel-data` Docker volume mounted at `/data`.

### Backup

```bash
docker run --rm \
  -v hirsel-data:/data \
  -v $(pwd):/backup \
  alpine tar czf /backup/hirsel-backup.tar.gz -C /data .
```

### Restore

```bash
docker run --rm \
  -v hirsel-data:/data \
  -v $(pwd):/backup \
  alpine tar xzf /backup/hirsel-backup.tar.gz -C /data
```

## Operations

```bash
# View logs
docker compose logs -f

# Restart
docker compose restart

# Rebuild after updating the repo
git pull
docker compose up -d --build

# Stop
docker compose down
```

## Important runtime note

Workers always run on the backend host.

For this Docker deployment, that means workers run **inside this container** unless you explicitly configure container runners or choose a non-containerized host deployment.

So this image is a good default for simple self-hosting, but it does **not** try to ship every possible project toolchain. If your workers need custom language/runtime environments, prefer:

- host/binary deployment, or
- explicit worker runner container images configured in Hirsel.
