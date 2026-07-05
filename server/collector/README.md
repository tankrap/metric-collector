# vc-tokmeter Collector

Minimal HTTP collector for opt-in vc-tokmeter metric uploads.

This service stores opt-in `vc-tokmeter.upload.v1` aggregate payloads in SQLite.
It is intentionally small and dependency-free so it can run in a Docker image
with a mounted volume. It does not accept raw event logs, prompts, tool
transcripts, source code, commands, paths, branch names, repository names,
credentials, or API keys.

## Run Locally

```sh
cd /Users/justin/metrics/server/collector
export COLLECTOR_UPLOAD_TOKEN="$(openssl rand -base64 32)"
export COLLECTOR_ADMIN_TOKEN="$(openssl rand -base64 32)"
python3 -m collector_server --host 127.0.0.1 --port 8080 --db data/collector.sqlite3
```

Health check:

```sh
curl http://127.0.0.1:8080/health
```

## API

### `GET /health`

Returns:

```json
{"status":"ok"}
```

### `POST /v1/uploads`

Accepts the versioned upload payload documented in
[`../../docs/upload-schema.md`](../../docs/upload-schema.md):

```json
{
  "schema_version": "vc-tokmeter.upload.v1",
  "artifact_type": "vc-tokmeter.upload",
  "created_at": "2026-07-05T15:30:00Z",
  "client": {
    "tokmeter_version": "0.1.0",
    "surface": "codex-tui",
    "platform": {
      "os": "macos",
      "arch": "arm64"
    },
  },
  "consent": {
    "upload_opt_in": true,
    "consent_version": "2026-07-05",
    "tester_alias": "tester-a"
  },
  "study": {"study_id": "metric-taker-t10", "protocol_version": "2026-07-05"},
  "session": {
    "session_id_hash": "0123456789abcdef",
    "time_bucket_utc": "2026-07-05T15:00:00Z",
    "duration_seconds": 1800
  },
  "metrics": "... aggregate report-share metrics only ...",
  "redaction": {
    "source_artifact": "report-share",
    "digest_hex_chars": 16,
    "private_data_policy": "aggregate-only-no-raw-content"
  }
}
```

Uploads require a configured upload token. Send either
`Authorization: Bearer <token>` or `X-Upload-Token: <token>`. The response is
upload metadata with a generated `id` and server `created_at` timestamp.

Use `COLLECTOR_UPLOAD_TOKENS` for a comma-separated token list, or
`COLLECTOR_UPLOAD_TOKEN_FILE` to read newline/comma-separated tokens from a
Docker secret file. The same forms exist for admin auth:
`COLLECTOR_ADMIN_TOKENS` and `COLLECTOR_ADMIN_TOKEN_FILE`.

### `GET /v1/uploads`

Admin-only. Returns upload summaries. Supports `limit` and `offset`.

### `GET /v1/uploads/{id}`

Admin-only. Returns upload metadata plus the stored sanitized payload.

### `GET /v1/uploads/export`

Admin-only. Returns all stored uploads as analysis rows. Export rows include
stored metadata and aggregate metric fields, but they do not include the full
stored payload.

Use `GET /v1/uploads/export?format=ndjson` for newline-delimited JSON.
Use `GET /v1/uploads/export?format=csv` for CSV.

### Dashboard API

Dashboard endpoints are admin-only aggregate views. Set
`COLLECTOR_ADMIN_TOKEN`, then send either
`Authorization: Bearer <token>`, `X-Admin-Token: <token>`, or HTTP Basic auth
with the admin token as the password.

Supported windows are `all`, `7d`, and `30d`.

```sh
curl -H "Authorization: Bearer $COLLECTOR_ADMIN_TOKEN" \
  "http://127.0.0.1:8080/v1/dashboard/summary?window=30d"
```

Endpoints:

* `GET /v1/dashboard/summary`
* `GET /v1/dashboard/time-series`
* `GET /v1/dashboard/surfaces`
* `GET /v1/dashboard/fidelity`
* `GET /v1/dashboard/git-actions?limit=10`

Responses are schema-versioned and aggregate-only. They intentionally exclude
upload IDs, tester IDs, payload hashes, stored payloads, prompts, transcripts,
source code, commands, tool output, paths, branch names, repository names,
credentials, and API keys.

### Web Dashboard

`GET /dashboard` serves a small admin-only HTML dashboard. It uses the same
aggregate dashboard data as the API and supports `?window=all`, `?window=7d`,
and `?window=30d`.

Open `http://127.0.0.1:8080/dashboard` in a browser and use any username with
`COLLECTOR_ADMIN_TOKEN` as the HTTP Basic auth password. The page shows summary
cards, daily volume, surface and fidelity distributions, top git actions, latest
aggregate upload metadata, and links to the admin JSON/NDJSON export endpoints.
It does not render raw upload payloads, upload IDs, tester IDs, session hashes,
payload hashes, prompts, transcripts, repository data, or credentials.

## Validation Boundary

Uploads are limited to 1 MiB by default. The service also rate limits upload
attempts by client IP with `COLLECTOR_RATE_LIMIT_REQUESTS` per
`COLLECTOR_RATE_LIMIT_WINDOW_SECONDS`; defaults are 60 requests per 60 seconds.
The service rejects non-v1 schemas, unknown fields, malformed aggregates,
common raw fields such as `event_log`, `prompt`, `messages`, `command`,
`stdout`, `stderr`, `diff`, `path`, `repo`, and `branch`, and sensitive fields
such as `api_key`, `authorization`, `secret`, and `credential`.

The collector derives dashboard fields from `client.surface`,
`metrics.evidence_grade`, `metrics.token_fidelity`, `metrics.summary`, and
`metrics.session_git_share`. This keeps the first server slice focused on the
study metrics needed for aggregate analysis and dashboard work.

## Storage

SQLite is stored at `data/collector.sqlite3` by default. For deployment, mount
the parent directory as a Docker volume and set `COLLECTOR_DB` to the mounted
database path. On startup, the collector creates the database, records applied
migrations in `schema_migrations`, and sets SQLite `user_version` to the current
collector storage schema version.

## Tests

```sh
cd /Users/justin/metrics/server/collector
python3 -m unittest discover -s tests
```

## End-to-End Smoke

From the repository root, run a no-network local smoke test:

```sh
python3 scripts/collector-e2e-smoke.py
```

The script starts an in-process collector, posts two redacted v1 fixture
uploads, verifies upload/admin auth, list/read/export, NDJSON export, dashboard
aggregate endpoints, and forbidden payload rejection.

To target a running compose or deployed stack:

```sh
COLLECTOR_BASE_URL=http://127.0.0.1:8088 \
COLLECTOR_UPLOAD_TOKEN=... \
COLLECTOR_ADMIN_TOKEN=... \
python3 scripts/collector-e2e-smoke.py
python3 scripts/collector-dashboard-smoke.py --base-url http://127.0.0.1:8088
```

The dashboard smoke checks that `/dashboard` serves nonblank HTML. That route is
served by the deployment Caddy layer; the Python collector itself only exposes
the JSON dashboard API.
