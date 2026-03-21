# Deployment Guide

Production deployment guide for PoliticalDebAIser.

## Docker Deployment

### Build and Run

```bash
# Development (default)
docker compose up --build -d

# Production (stricter settings, JSON logging, higher resource limits)
docker compose --profile production up --build -d
```

This starts two services:
- **app** (or **app-production** with the production profile) — PoliticalDebAIser on port 3000
- **ollama** — Local LLM inference on port 11434 (bound to 127.0.0.1 only)

### Resource Limits

Docker Compose enforces resource constraints:

| Service | Memory Limit | CPU Limit |
|---------|-------------|-----------|
| app (dev) | 512M | 1.0 |
| app (production) | 1G | 2.0 |
| ollama | 4G | 4.0 |

### Custom Configuration

Create a `.env` file or pass environment variables:

```bash
OLLAMA_MODEL=llama3.2
RUST_LOG=info
CORS_ORIGIN=https://yourdomain.com
LOG_FORMAT=json
```

### Using Cloud LLM Providers

To use cloud providers instead of (or alongside) local Ollama:

```bash
LLM_PROVIDERS=groq,gemini
GROQ_API_KEY=your-groq-key
GEMINI_API_KEY=your-gemini-key
```

With cloud providers, you can remove the `ollama` service from docker-compose or keep it as a fallback.

### Docker Compose Override

Create a `docker-compose.override.yml` for production customizations:

```yaml
services:
  app:
    environment:
      LLM_PROVIDERS: groq,gemini,ollama
      GROQ_API_KEY: ${GROQ_API_KEY}
      GEMINI_API_KEY: ${GEMINI_API_KEY}
      CONFIG_AUTH_TOKEN: ${CONFIG_AUTH_TOKEN}
      LOG_FORMAT: json
      CORS_ORIGIN: https://yourdomain.com
    restart: always
```

---

## Fly.io Deployment

### Prerequisites

- [flyctl](https://fly.io/docs/hands-on/install-flyctl/) installed and authenticated

### Steps

1. **Create the app:**

```bash
fly launch --no-deploy
```

2. **Set secrets:**

```bash
fly secrets set \
  LLM_PROVIDERS=groq,gemini \
  GROQ_API_KEY=your-groq-key \
  GEMINI_API_KEY=your-gemini-key \
  CONFIG_AUTH_TOKEN=your-auth-token \
  LOG_FORMAT=json
```

3. **Configure `fly.toml`:**

```toml
app = "political-debaiser"
primary_region = "syd"

[build]
  dockerfile = "Dockerfile"

[env]
  RUST_LOG = "info"
  LOG_FORMAT = "json"

[http_service]
  internal_port = 3000
  force_https = true
  auto_stop_machines = true
  auto_start_machines = true

  [[http_service.checks]]
    grace_period = "10s"
    interval = "15s"
    method = "GET"
    path = "/health"
    timeout = "5s"

[[vm]]
  size = "shared-cpu-1x"
  memory = "512mb"
```

4. **Deploy:**

```bash
fly deploy
```

> **Note:** Fly.io does not run Ollama locally. Use cloud LLM providers (`LLM_PROVIDERS=groq,gemini`) or connect to an external Ollama instance via `OLLAMA_URL`.

---

## Railway Deployment

### Steps

1. **Connect your repository** in the [Railway dashboard](https://railway.app/).

2. **Set environment variables** in the Railway service settings:

| Variable | Value |
|----------|-------|
| `LLM_PROVIDERS` | `groq,gemini` |
| `GROQ_API_KEY` | your key |
| `GEMINI_API_KEY` | your key |
| `CONFIG_AUTH_TOKEN` | your auth token |
| `LOG_FORMAT` | `json` |
| `RUST_LOG` | `info` |
| `CORS_ORIGIN` | your Railway URL |

3. **Configure health check** in service settings:
   - Path: `/health`
   - Interval: 15s
   - Timeout: 5s

4. **Deploy** — Railway auto-builds from the Dockerfile on push.

> **Note:** Like Fly.io, Railway does not provide Ollama. Use cloud LLM providers.

---

## Environment Configuration

### Required for Cloud Providers

| Variable | Description |
|----------|-------------|
| `LLM_PROVIDERS` | Comma-separated list: `groq`, `gemini`, `huggingface`, `ollama` |
| `GROQ_API_KEY` | Required if `groq` is in providers list |
| `GEMINI_API_KEY` | Required if `gemini` is in providers list |
| `HF_API_KEY` | Required if `huggingface` is in providers list |

### Required for Ollama

| Variable | Default | Description |
|----------|---------|-------------|
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint |
| `OLLAMA_MODEL` | `llama3.2` | Model to use for inference |

### Security

| Variable | Description |
|----------|-------------|
| `CONFIG_AUTH_TOKEN` | Bearer token for protected endpoints (`/config`, `/history` DELETE). Set this in production. |
| `CORS_ORIGIN` | Restrict CORS to your domain (default: `http://localhost:3000`) |

### Logging

| Variable | Default | Description |
|----------|---------|-------------|
| `LOG_FORMAT` | `text` | Set to `json` for structured logging (recommended for production) |
| `RUST_LOG` | `info` | Log level filter |

---

## Health Check Setup

The `/health` endpoint returns:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "providers": ["groq", "gemini"],
  "history_count": 42
}
```

Configure your load balancer or orchestrator to poll `/health` every 10-15 seconds with a 5-second timeout.

---

## Monitoring

### /health Endpoint

Use `/health` for uptime monitoring and service status. It reports:
- Application version
- Uptime in seconds
- Configured LLM providers
- Number of stored analyses

### /metrics Endpoint

Use `/metrics` for operational monitoring:

```json
{
  "total_requests": 15234,
  "total_analyses": 892,
  "cache_count": 47,
  "uptime_seconds": 86400
}
```

Integrate with your monitoring stack by polling `/metrics` and forwarding to Prometheus, Datadog, or similar.

### Structured Logging

Set `LOG_FORMAT=json` in production for machine-parseable logs compatible with log aggregation services (CloudWatch, Loki, Datadog Logs, etc.).

### Recommended Alerts

| Metric | Condition | Action |
|--------|-----------|--------|
| `/health` status | Not `ok` | Page on-call |
| `/health` response time | > 5s | Investigate |
| `/metrics` total_requests | Sudden drop | Check upstream |
| Error rate in logs | > 5% | Investigate LLM provider status |
