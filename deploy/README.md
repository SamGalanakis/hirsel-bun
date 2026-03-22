# Hirsel Server Deployment

Deploy hirsel as a headless backend server.

## Network Architecture

Hirsel expects the backend to be reachable at a stable URL from client devices:

```
Desktop / phone app ──┐
                      │ your network / VPN / reverse proxy
Backend host ─────────┘  backend.example.internal:8080
```

Clients connect to the backend via whatever URL you provide. Hirsel does not manage the network layer itself.

## Setup

### 1. Put the backend host on your network

```bash
# Make sure client devices can reach the backend host.
# This can be via LAN, VPN, reverse proxy, or any other setup you manage.
```

### 2. Deploy the backend

```bash
git clone https://github.com/anthropics/hirsel.git
cd hirsel/deploy

cp .env.example .env
# Edit .env and set HIRSEL_API_KEY (generate with: openssl rand -hex 32)

docker compose up -d --build
```

First build takes a few minutes (compiling Rust).

### 3. Verify

```bash
# From a client machine
curl http://backend.example.internal:8080/health
```

## Client Configuration

On each client device, configure the backend target in `~/.hirsel/config.toml`:

```toml
[backend]
url = "http://backend.example.internal:8080"
api_key = "your-api-key"
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `HIRSEL_API_KEY` | API key for authentication (required) |

## Data Management

Run data is persisted in the `hirsel-data` Docker volume.

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

# Update
git pull
docker compose up -d --build

# Stop
docker compose down
```

The backend just needs to be reachable by clients at some URL.
