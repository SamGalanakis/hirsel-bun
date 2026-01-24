# Hirsel Server Deployment

Deploy hirsel as a headless orchestrator server.

## Network Architecture

Hirsel uses [Tailscale](https://tailscale.com) for secure networking between the orchestrator and workers:

```
Your machine ──┐
               │ Tailscale network (WireGuard encrypted)
Orchestrator ──┼── 100.x.x.x:8080
               │
Fly workers ───┘
```

Workers connect to the orchestrator via its Tailscale IP. No domains, certificates, or port forwarding needed.

## Setup

### 1. Install Tailscale on the orchestrator VPS

```bash
# On your VPS (Ubuntu/Debian)
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up

# Note the Tailscale IP (100.x.x.x)
tailscale ip -4
```

### 2. Deploy the orchestrator

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
# From your machine (also on Tailscale)
curl http://100.x.x.x:8080/health
```

## Client Configuration

On your local machine, configure a remote profile in `~/.hirsel/config.toml`:

```toml
default_profile = "remote"

[profiles.remote]
mode = "remote"
url = "http://100.x.x.x:8080"  # Orchestrator's Tailscale IP
api_key = "your-api-key"

# Access strategy - how workers reach the orchestrator
[profiles.remote.access]
type = "tailscale"
oauth_client_id = "your-oauth-client-id"
oauth_client_secret = "your-oauth-client-secret"
tag = "tag:hirsel-worker"  # Optional: tag for worker devices
```

### Tailscale OAuth Setup

Hirsel uses Tailscale OAuth to generate ephemeral auth keys for workers on-demand. This avoids the 90-day expiration limit of static auth keys.

1. Go to the [Tailscale admin console](https://login.tailscale.com/admin/settings/oauth)
2. Create a new OAuth client with these scopes:
   - `devices:core:write` (to create auth keys)
3. Copy the Client ID and Client Secret to your config

Workers receive short-lived (5 minute) ephemeral auth keys at spawn time. The devices auto-remove from your tailnet when they go offline.

### Access Strategies

**Tailscale** (recommended) - Workers auto-join your tailnet:
```toml
[profiles.remote.access]
type = "tailscale"
oauth_client_id = "..."
oauth_client_secret = "..."
tag = "tag:hirsel-worker"  # Optional
```

**Direct** - Assumes network already configured (VPC, same LAN):
```toml
[profiles.remote.access]
type = "direct"
```

## Worker Hosts

With the Tailscale access strategy, worker hosts automatically join your tailnet when spawned. Each worker receives a fresh ephemeral auth key - no manual setup needed.

For the Direct strategy, ensure worker hosts can reach the orchestrator IP before starting runs.

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

## Alternative: No Tailscale

If you can't use Tailscale, alternatives include:

- **Cloudflare Tunnel**: Free, but adds latency and has upload limits
- **VPC/Private network**: If orchestrator and workers are in same cloud
- **Direct IP + firewall rules**: Less secure, requires careful firewall config

The orchestrator just needs to be reachable by workers at some URL.
