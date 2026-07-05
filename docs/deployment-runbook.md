# Metric Taker Collector Deployment Runbook

This runbook covers a bounded collector deployment for the opt-in
`vc-tokmeter.upload.v1` study flow. It does not require any external network for
local smoke testing.

## Scope

- Collector API behind Caddy.
- Persistent SQLite storage in the `collector-data` Docker volume.
- Admin-only JSON dashboard endpoints.
- Caddy-served `/dashboard` HTML that loads aggregate dashboard APIs after an
  admin token is entered.

The current smoke scripts seed redacted fixture uploads directly through the
collector API. They do not prove the Rust CLI upload command end to end unless a
packaged `vc-tokmeter upload` is separately pointed at the same collector.

## Preflight

From the repository root:

```sh
python3 scripts/collector-e2e-smoke.py
```

This starts an in-process collector on localhost, posts sample redacted uploads,
verifies upload auth, admin list/read/export, NDJSON export, dashboard
aggregates, and forbidden payload rejection.

Validate the compose file before a host deployment:

```sh
cd deploy/collector
cp .env.example .env
docker compose config --quiet
```

## Configure

Set these values in `deploy/collector/.env`:

```sh
COLLECTOR_DOMAIN=metrics.example.com
COLLECTOR_HTTP_PORT=80
COLLECTOR_UPLOAD_TOKEN=replace-with-generated-upload-token
COLLECTOR_ADMIN_TOKEN=replace-with-generated-admin-token
COLLECTOR_MAX_BODY_BYTES=1048576
COLLECTOR_DB=/data/collector.sqlite3
```

Generate token values with:

```sh
openssl rand -base64 32
```

Use separate values for upload and admin tokens.

## Local Compose Smoke

Use localhost and an unprivileged port:

```sh
cd deploy/collector
COLLECTOR_DOMAIN=:80 COLLECTOR_HTTP_PORT=8088 docker compose up -d --build
cd ../..
COLLECTOR_BASE_URL=http://127.0.0.1:8088 \
COLLECTOR_UPLOAD_TOKEN="$(grep '^COLLECTOR_UPLOAD_TOKEN=' deploy/collector/.env | cut -d= -f2-)" \
COLLECTOR_ADMIN_TOKEN="$(grep '^COLLECTOR_ADMIN_TOKEN=' deploy/collector/.env | cut -d= -f2-)" \
python3 scripts/collector-e2e-smoke.py
python3 scripts/collector-dashboard-smoke.py --base-url http://127.0.0.1:8088
```

Open `http://127.0.0.1:8088/dashboard`, enter the admin token, and confirm the
seeded aggregate metrics load.

## VPS Deploy

1. Create a DNS `A` or `AAAA` record for `COLLECTOR_DOMAIN`.
2. Copy `.env.example` to `.env` and set generated tokens.
3. Start the stack:

```sh
cd deploy/collector
docker compose up -d --build
docker compose ps
```

4. Run smoke checks against the public origin:

```sh
COLLECTOR_BASE_URL=https://metrics.example.com \
COLLECTOR_UPLOAD_TOKEN=... \
COLLECTOR_ADMIN_TOKEN=... \
python3 scripts/collector-e2e-smoke.py
python3 scripts/collector-dashboard-smoke.py --base-url https://metrics.example.com
```

## Operations

Check service health:

```sh
curl -fsS https://metrics.example.com/health
```

Export retained uploads for offline analysis:

```sh
curl -fsS \
  -H "Authorization: Bearer $COLLECTOR_ADMIN_TOKEN" \
  "https://metrics.example.com/v1/uploads/export?format=ndjson" \
  > collector-export.ndjson
```

Back up the SQLite volume:

```sh
cd deploy/collector
docker run --rm \
  -v collector_collector-data:/data:ro \
  -v "$PWD":/backup \
  alpine tar -czf /backup/collector-data.tgz -C /data .
```

Rotate tokens by editing `.env` and restarting:

```sh
docker compose up -d
```

After rotating the upload token, update tester instructions before inviting the
next cohort.

## Rollback

Stop the current stack:

```sh
cd deploy/collector
docker compose down
```

Restore a known-good database backup if required:

```sh
docker run --rm \
  -v collector_collector-data:/data \
  -v "$PWD":/backup \
  alpine sh -c 'rm -rf /data/* && tar -xzf /backup/collector-data.tgz -C /data'
```

Then restart the stack and rerun both smoke scripts.

## Known Gaps

- The smoke scripts seed the documented v1 fixture through HTTP; they do not
  execute `vc-tokmeter upload`.
- There is no deletion endpoint yet for tester-requested removal.
- The dashboard HTML is a deployment page served by Caddy and backed by
  aggregate JSON APIs. It is not part of the collector Python application.
