# Collector Deployment

This stack runs the opt-in tokmeter collector behind Caddy. It is intended for
redacted `vc-tokmeter.upload.v1` payloads only.

## Files

- `docker-compose.yml`: collector app, persistent SQLite volume, and Caddy.
- `Caddyfile`: TLS reverse proxy with compression, security headers,
  auth-header log redaction, and a 1 MiB body limit. It also serves
  `/dashboard`, a small admin-token-backed aggregate dashboard page.
- `.env.example`: required deployment settings.
- [`../../docs/deployment-runbook.md`](../../docs/deployment-runbook.md):
  operator runbook and smoke checks.

## Local Bring-Up

```sh
cd deploy/collector
cp .env.example .env
openssl rand -base64 32
openssl rand -base64 32
```

Put the generated values into `COLLECTOR_UPLOAD_TOKEN` and
`COLLECTOR_ADMIN_TOKEN`.

For local testing before DNS is configured:

```sh
COLLECTOR_DOMAIN=:80 COLLECTOR_HTTP_PORT=8088 docker compose up
```

Then check:

```sh
curl -i http://localhost:8088/health
```

Run the bounded API and dashboard smoke checks from the repository root:

```sh
COLLECTOR_BASE_URL=http://127.0.0.1:8088 \
COLLECTOR_UPLOAD_TOKEN="$(grep '^COLLECTOR_UPLOAD_TOKEN=' deploy/collector/.env | cut -d= -f2-)" \
COLLECTOR_ADMIN_TOKEN="$(grep '^COLLECTOR_ADMIN_TOKEN=' deploy/collector/.env | cut -d= -f2-)" \
python3 scripts/collector-e2e-smoke.py
python3 scripts/collector-dashboard-smoke.py --base-url http://127.0.0.1:8088
```

Open `http://127.0.0.1:8088/dashboard` and enter the admin token to view the
seeded aggregate dashboard.

## VPS Bring-Up

1. Create a DNS `A` or `AAAA` record for `COLLECTOR_DOMAIN`.
2. Copy `.env.example` to `.env`.
3. Set strong `COLLECTOR_UPLOAD_TOKEN` and `COLLECTOR_ADMIN_TOKEN` values.
4. Start the stack:

```sh
docker compose up -d
```

Caddy will request and renew TLS certificates automatically for a real domain.

After the stack is healthy, run:

```sh
COLLECTOR_BASE_URL=https://$COLLECTOR_DOMAIN \
COLLECTOR_UPLOAD_TOKEN=... \
COLLECTOR_ADMIN_TOKEN=... \
python3 ../../scripts/collector-e2e-smoke.py
python3 ../../scripts/collector-dashboard-smoke.py --base-url "https://$COLLECTOR_DOMAIN"
```

## Storage

Uploaded artifacts are stored through the collector service in the
`collector-data` Docker volume. The collector initializes SQLite on startup and
records applied migrations in `schema_migrations`. Back up this volume before
server migrations:

```sh
docker run --rm \
  -v collector_collector-data:/data:ro \
  -v "$PWD":/backup \
  alpine tar -czf /backup/collector-data.tgz -C /data .
```

## Security Notes

- Keep upload and admin tokens out of shell history where possible.
- Uploads without a valid upload token are rejected by the app.
- Rotate tokens before inviting a new study cohort.
- `COLLECTOR_UPLOAD_TOKENS` and `COLLECTOR_ADMIN_TOKENS` may hold
  comma-separated token sets for cohort rotation. The app can also read
  newline/comma-separated tokens from Docker secret files via
  `COLLECTOR_UPLOAD_TOKEN_FILE` and `COLLECTOR_ADMIN_TOKEN_FILE`.
- The Caddy request body limit is defense in depth. The collector app must also
  enforce `COLLECTOR_MAX_BODY_BYTES`.
- The collector rate limits upload attempts by client IP with
  `COLLECTOR_RATE_LIMIT_REQUESTS` per `COLLECTOR_RATE_LIMIT_WINDOW_SECONDS`;
  defaults are 60 requests per 60 seconds.
- Caddy access logs should not include upload payload bodies, but operational
  logs may include client IPs and request paths. The provided Caddyfile removes
  `Authorization`, `X-Upload-Token`, `X-Admin-Token`, and `Cookie` headers from
  access logs.
- The `/dashboard` page is public HTML, but every API call it makes requires the
  admin token. Do not share the admin token with testers.
