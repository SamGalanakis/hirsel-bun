# Hirsel Server Deployment

Deploy hirsel as a headless server using Docker Compose.

## Prerequisites

- Docker and Docker Compose
- Hirsel CLI binary built for Linux

## Build the binary

From the repo root:

```bash
cd src-tauri
cargo build --release --no-default-features
cp target/release/hirsel ../deploy/
```

## Configure

```bash
cp .env.example .env
# Edit .env and set a secure HIRSEL_API_KEY
```

## Deploy

```bash
docker-compose up -d --build
```

## Usage

```bash
# Check health
curl http://localhost:8080/health

# List runs (with auth)
curl -H "Authorization: Bearer YOUR_API_KEY" http://localhost:8080/api/runs
```

## Client configuration

On your local machine, configure a remote profile in `~/.hirsel/config.toml`:

```toml
[profiles.remote]
mode = "remote"
url = "http://your-server:8080"
api_key = "your-api-key"

default_profile = "remote"
```

Then use the CLI or GUI - credentials are forwarded from your local machine.

## Data

Run data is persisted in the `hirsel-data` Docker volume at `/root/.hirsel`.

Backup:
```bash
docker run --rm -v hirsel-data:/data -v $(pwd):/backup alpine tar czf /backup/hirsel-backup.tar.gz -C /data .
```

Restore:
```bash
docker run --rm -v hirsel-data:/data -v $(pwd):/backup alpine tar xzf /backup/hirsel-backup.tar.gz -C /data
```
