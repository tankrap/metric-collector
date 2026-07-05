from __future__ import annotations

import argparse
import base64
import csv
import hmac
import io
import json
import math
import os
import threading
import time
from collections import defaultdict, deque
from dataclasses import dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Iterable, Sequence
from urllib.parse import parse_qs, urlparse

from .dashboard import (
    dashboard_page_model,
    dashboard_fidelity_distribution,
    dashboard_summary,
    dashboard_surface_distribution,
    dashboard_time_series,
    dashboard_top_git_actions,
    render_dashboard_html,
)
from .storage import UploadStore
from .validation import (
    DEFAULT_MAX_BODY_BYTES,
    PayloadValidationError,
    validate_request_body,
    validate_upload_payload,
)


@dataclass(frozen=True)
class CollectorConfig:
    db_path: str
    max_body_bytes: int = DEFAULT_MAX_BODY_BYTES
    upload_token: str | None = None
    upload_tokens: tuple[str, ...] = ()
    # Dashboard endpoints are aggregate-only and require an admin token. The
    # deployment layer should set COLLECTOR_ADMIN_TOKEN and keep it private.
    admin_token: str | None = None
    admin_tokens: tuple[str, ...] = ()
    rate_limit_requests: int = 60
    rate_limit_window_seconds: int = 60

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            "upload_tokens",
            _normalize_tokens(self.upload_token, self.upload_tokens),
        )
        object.__setattr__(
            self,
            "admin_tokens",
            _normalize_tokens(self.admin_token, self.admin_tokens),
        )
        object.__setattr__(self, "rate_limit_requests", max(1, self.rate_limit_requests))
        object.__setattr__(
            self,
            "rate_limit_window_seconds",
            max(1, self.rate_limit_window_seconds),
        )


class RateLimiter:
    def __init__(self, *, limit: int, window_seconds: int):
        self.limit = max(1, limit)
        self.window_seconds = max(1, window_seconds)
        self._events: dict[str, deque[float]] = defaultdict(deque)
        self._lock = threading.Lock()

    def allow(self, key: str, *, now: float | None = None) -> tuple[bool, int]:
        clock = now if now is not None else time.monotonic()
        cutoff = clock - self.window_seconds
        with self._lock:
            events = self._events[key]
            while events and events[0] <= cutoff:
                events.popleft()
            if len(events) >= self.limit:
                retry_after = max(1, math.ceil(events[0] + self.window_seconds - clock))
                return False, retry_after
            events.append(clock)
            return True, 0


class CollectorHandler(BaseHTTPRequestHandler):
    server_version = "vc-tokmeter-collector/0.1"

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        path = _normalize_path(parsed.path)

        if path in {"/health", "/v1/health"}:
            self._write_json(HTTPStatus.OK, {"status": "ok"})
            return

        if path == "/dashboard":
            self._handle_dashboard_page(parsed.query)
            return

        if path == "/v1/uploads":
            self._handle_upload_list(parsed.query)
            return

        if path == "/v1/uploads/export":
            self._handle_upload_export(parsed.query)
            return

        if path.startswith("/v1/dashboard/"):
            self._handle_dashboard(path.removeprefix("/v1/dashboard/"), parsed.query)
            return

        if path.startswith("/v1/uploads/"):
            self._handle_upload_get(path.removeprefix("/v1/uploads/"))
            return

        self._write_error(HTTPStatus.NOT_FOUND, "not_found", "endpoint not found")

    def do_POST(self) -> None:
        parsed = urlparse(self.path)
        path = _normalize_path(parsed.path)

        if path == "/v1/uploads":
            self._handle_upload_create()
            return

        self._write_error(HTTPStatus.NOT_FOUND, "not_found", "endpoint not found")

    def log_message(self, format: str, *args: Any) -> None:
        return

    @property
    def store(self) -> UploadStore:
        return self.server.store  # type: ignore[attr-defined]

    @property
    def config(self) -> CollectorConfig:
        return self.server.config  # type: ignore[attr-defined]

    def _handle_upload_create(self) -> None:
        allowed, retry_after = self.server.rate_limiter.allow(  # type: ignore[attr-defined]
            self._rate_limit_key()
        )
        if not allowed:
            self._write_error(
                HTTPStatus.TOO_MANY_REQUESTS,
                "rate_limited",
                "too many upload attempts; retry later",
                headers={"retry-after": str(retry_after)},
            )
            return

        if not self.config.upload_tokens:
            self._write_error(
                HTTPStatus.SERVICE_UNAVAILABLE,
                "upload_auth_not_configured",
                "upload authorization is not configured",
            )
            return

        if not self._is_token_request(
            self.config.upload_tokens,
            header_name="x-upload-token",
        ):
            self._write_error(
                HTTPStatus.UNAUTHORIZED,
                "unauthorized",
                "upload endpoint requires upload authorization",
            )
            return

        content_type = self.headers.get("content-type", "")
        if "application/json" not in content_type.lower():
            self._write_error(
                HTTPStatus.UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "content-type must be application/json",
            )
            return

        try:
            content_length = int(self.headers.get("content-length", "0"))
        except ValueError:
            self._write_error(HTTPStatus.BAD_REQUEST, "invalid_request", "bad content-length")
            return

        if content_length > self.config.max_body_bytes:
            self._write_error(
                HTTPStatus.REQUEST_ENTITY_TOO_LARGE,
                "payload_too_large",
                "request body exceeds configured size limit",
            )
            return

        try:
            body = self.rfile.read(content_length)
            raw_payload = validate_request_body(
                body,
                max_body_bytes=self.config.max_body_bytes,
            )
            upload = validate_upload_payload(raw_payload)
            record = self.store.create(
                payload=upload.payload,
                payload_sha256=upload.payload_sha256,
                tester_id=upload.tester_id,
                surface=upload.surface,
                evidence_grade=upload.evidence_grade,
                fidelity=upload.fidelity,
                total_tokens=upload.total_tokens,
                git_tokens=upload.git_tokens,
                git_token_share=upload.git_token_share,
            )
        except PayloadValidationError as exc:
            self._write_error(HTTPStatus.BAD_REQUEST, "invalid_payload", str(exc))
            return

        self._write_json(HTTPStatus.CREATED, record.summary())

    def _handle_upload_get(self, upload_id: str) -> None:
        if not self._is_admin_request():
            self._write_error(
                HTTPStatus.UNAUTHORIZED,
                "unauthorized",
                "upload read endpoints require admin authorization",
            )
            return
        if not upload_id or "/" in upload_id:
            self._write_error(HTTPStatus.NOT_FOUND, "not_found", "upload not found")
            return
        record = self.store.get(upload_id)
        if record is None:
            self._write_error(HTTPStatus.NOT_FOUND, "not_found", "upload not found")
            return
        self._write_json(HTTPStatus.OK, record.to_dict())

    def _handle_upload_list(self, query: str) -> None:
        if not self._is_admin_request():
            self._write_error(
                HTTPStatus.UNAUTHORIZED,
                "unauthorized",
                "upload list endpoint requires admin authorization",
            )
            return
        params = parse_qs(query)
        limit = _int_param(params, "limit", default=100)
        offset = _int_param(params, "offset", default=0)
        records = self.store.list(limit=limit, offset=offset)
        self._write_json(
            HTTPStatus.OK,
            {
                "count": self.store.count(),
                "uploads": [record.summary() for record in records],
            },
        )

    def _handle_upload_export(self, query: str) -> None:
        if not self._is_admin_request():
            self._write_error(
                HTTPStatus.UNAUTHORIZED,
                "unauthorized",
                "upload export endpoint requires admin authorization",
            )
            return
        params = parse_qs(query)
        records = self.store.export()
        rows = [record.export_dict() for record in records]
        export_format = params.get("format", ["json"])[0]
        if export_format == "ndjson":
            lines = [json.dumps(row, sort_keys=True) for row in rows]
            body = ("\n".join(lines) + ("\n" if lines else "")).encode("utf-8")
            self.send_response(HTTPStatus.OK)
            self.send_header("content-type", "application/x-ndjson")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if export_format == "csv":
            output = io.StringIO()
            fieldnames = list(rows[0].keys()) if rows else _export_fieldnames()
            writer = csv.DictWriter(output, fieldnames=fieldnames, lineterminator="\n")
            writer.writeheader()
            writer.writerows(rows)
            body = output.getvalue().encode("utf-8")
            self.send_response(HTTPStatus.OK)
            self.send_header("content-type", "text/csv")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if export_format != "json":
            self._write_error(
                HTTPStatus.BAD_REQUEST,
                "invalid_request",
                "format must be json, ndjson, or csv",
            )
            return
        self._write_json(
            HTTPStatus.OK,
            {"count": len(rows), "uploads": rows},
        )

    def _handle_dashboard(self, endpoint: str, query: str) -> None:
        if not self._is_admin_request():
            self._write_admin_unauthorized("dashboard endpoints require admin authorization")
            return

        params = parse_qs(query)
        window = params.get("window", ["all"])[0]
        records = self.store.export()

        if endpoint == "summary":
            self._write_json(
                HTTPStatus.OK,
                dashboard_summary(records, window=window),
            )
            return

        if endpoint == "time-series":
            self._write_json(
                HTTPStatus.OK,
                dashboard_time_series(records, window=window),
            )
            return

        if endpoint == "surfaces":
            self._write_json(
                HTTPStatus.OK,
                dashboard_surface_distribution(records, window=window),
            )
            return

        if endpoint == "fidelity":
            self._write_json(
                HTTPStatus.OK,
                dashboard_fidelity_distribution(records, window=window),
            )
            return

        if endpoint == "git-actions":
            limit = _int_param(params, "limit", default=10)
            self._write_json(
                HTTPStatus.OK,
                dashboard_top_git_actions(records, window=window, limit=limit),
            )
            return

        self._write_error(HTTPStatus.NOT_FOUND, "not_found", "endpoint not found")

    def _handle_dashboard_page(self, query: str) -> None:
        if not self._is_admin_request():
            self._write_admin_unauthorized(
                "dashboard page requires admin authorization",
                html=True,
            )
            return

        params = parse_qs(query)
        window = params.get("window", ["all"])[0]
        model = dashboard_page_model(self.store.export(), window=window)
        self._write_html(HTTPStatus.OK, render_dashboard_html(model))

    def _is_admin_request(self) -> bool:
        if not self.config.admin_tokens:
            return False
        return self._is_token_request(self.config.admin_tokens, header_name="x-admin-token")

    def _is_token_request(self, expected_tokens: Sequence[str], *, header_name: str) -> bool:
        provided_values = []
        header_value = self.headers.get(header_name, "")
        if header_value:
            provided_values.append(header_value.strip())
        authorization = self.headers.get("authorization", "")
        if authorization.lower().startswith("bearer "):
            provided_values.append(authorization[7:].strip())
        elif authorization.lower().startswith("basic "):
            provided_values.append(_basic_auth_token(authorization[6:].strip()))

        return any(
            hmac.compare_digest(provided, expected)
            for provided in provided_values
            for expected in expected_tokens
        )

    def _rate_limit_key(self) -> str:
        forwarded = self.headers.get("x-forwarded-for", "")
        if forwarded:
            return forwarded.split(",", 1)[0].strip()
        return self.client_address[0]

    def _write_admin_unauthorized(self, message: str, *, html: bool = False) -> None:
        if html:
            body = (
                "<!doctype html><html><head><title>Unauthorized</title></head>"
                "<body><h1>Unauthorized</h1></body></html>"
            ).encode("utf-8")
            self.send_response(HTTPStatus.UNAUTHORIZED)
            self.send_header("content-type", "text/html; charset=utf-8")
            self.send_header("www-authenticate", 'Basic realm="Metric Taker Dashboard"')
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        self._write_error(HTTPStatus.UNAUTHORIZED, "unauthorized", message)

    def _write_error(
        self,
        status: HTTPStatus,
        code: str,
        message: str,
        *,
        headers: dict[str, str] | None = None,
    ) -> None:
        self._write_json(
            status,
            {"error": {"code": code, "message": message}},
            headers=headers,
        )

    def _write_json(
        self,
        status: HTTPStatus,
        value: dict[str, Any],
        *,
        headers: dict[str, str] | None = None,
    ) -> None:
        body = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        for name, header_value in (headers or {}).items():
            self.send_header(name, header_value)
        self.end_headers()
        self.wfile.write(body)

    def _write_html(self, status: HTTPStatus, value: str) -> None:
        body = value.encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "text/html; charset=utf-8")
        self.send_header(
            "content-security-policy",
            "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'",
        )
        self.send_header("x-content-type-options", "nosniff")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class CollectorHTTPServer(ThreadingHTTPServer):
    def __init__(self, server_address: tuple[str, int], config: CollectorConfig):
        super().__init__(server_address, CollectorHandler)
        self.config = config
        self.store = UploadStore(config.db_path)
        self.rate_limiter = RateLimiter(
            limit=config.rate_limit_requests,
            window_seconds=config.rate_limit_window_seconds,
        )


def make_server(host: str, port: int, config: CollectorConfig) -> CollectorHTTPServer:
    return CollectorHTTPServer((host, port), config)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the vc-tokmeter collector API")
    parser.add_argument("--host", default=os.environ.get("COLLECTOR_HOST", "127.0.0.1"))
    parser.add_argument(
        "--port",
        type=int,
        default=int(os.environ.get("COLLECTOR_PORT", "8080")),
    )
    parser.add_argument(
        "--db",
        default=os.environ.get("COLLECTOR_DB", "data/collector.sqlite3"),
    )
    parser.add_argument(
        "--max-body-bytes",
        type=int,
        default=int(os.environ.get("COLLECTOR_MAX_BODY_BYTES", DEFAULT_MAX_BODY_BYTES)),
    )
    parser.add_argument(
        "--admin-token-file",
        default=os.environ.get("COLLECTOR_ADMIN_TOKEN_FILE"),
    )
    parser.add_argument(
        "--upload-token-file",
        default=os.environ.get("COLLECTOR_UPLOAD_TOKEN_FILE"),
    )
    parser.add_argument(
        "--rate-limit-requests",
        type=int,
        default=int(os.environ.get("COLLECTOR_RATE_LIMIT_REQUESTS", "60")),
    )
    parser.add_argument(
        "--rate-limit-window-seconds",
        type=int,
        default=int(os.environ.get("COLLECTOR_RATE_LIMIT_WINDOW_SECONDS", "60")),
    )
    args = parser.parse_args()

    config = CollectorConfig(
        db_path=args.db,
        max_body_bytes=args.max_body_bytes,
        upload_tokens=_tokens_from_sources(
            os.environ.get("COLLECTOR_UPLOAD_TOKEN"),
            os.environ.get("COLLECTOR_UPLOAD_TOKENS"),
            args.upload_token_file,
        ),
        admin_tokens=_tokens_from_sources(
            os.environ.get("COLLECTOR_ADMIN_TOKEN"),
            os.environ.get("COLLECTOR_ADMIN_TOKENS"),
            args.admin_token_file,
        ),
        rate_limit_requests=args.rate_limit_requests,
        rate_limit_window_seconds=args.rate_limit_window_seconds,
    )
    server = make_server(args.host, args.port, config)
    print(f"collector listening on http://{args.host}:{args.port}")
    print(f"collector db: {args.db}")
    server.serve_forever()


def _normalize_path(path: str) -> str:
    return path.rstrip("/") or "/"


def _int_param(params: dict[str, list[str]], key: str, *, default: int) -> int:
    try:
        return int(params.get(key, [str(default)])[0])
    except ValueError:
        return default


def _tokens_from_sources(*sources: str | None) -> tuple[str, ...]:
    tokens: list[str] = []
    for source in sources[:2]:
        tokens.extend(_split_tokens(source))
    token_file = sources[2] if len(sources) > 2 else None
    if token_file:
        with open(token_file, encoding="utf-8") as handle:
            tokens.extend(_split_tokens(handle.read()))
    return _normalize_tokens(None, tokens)


def _normalize_tokens(
    primary: str | None,
    additional: Iterable[str],
) -> tuple[str, ...]:
    tokens: list[str] = []
    tokens.extend(_split_tokens(primary))
    for token in additional:
        tokens.extend(_split_tokens(token))
    unique: list[str] = []
    seen = set()
    for token in tokens:
        if token not in seen:
            unique.append(token)
            seen.add(token)
    return tuple(unique)


def _split_tokens(value: str | None) -> list[str]:
    if not value:
        return []
    return [token.strip() for token in value.replace("\n", ",").split(",") if token.strip()]


def _export_fieldnames() -> list[str]:
    return [
        "id",
        "received_at",
        "schema_version",
        "study_id",
        "protocol_version",
        "tester_id",
        "session_id_hash",
        "time_bucket_utc",
        "repo_hash",
        "surface",
        "evidence_grade",
        "fidelity",
        "total_tokens",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "bytes",
        "git_tokens",
        "non_git_tokens",
        "git_token_share",
        "payload_sha256",
        "token_sources_json",
        "git_action_subtypes_json",
    ]


def _basic_auth_token(value: str) -> str:
    try:
        decoded = base64.b64decode(value, validate=True).decode("utf-8")
    except (ValueError, UnicodeDecodeError):
        return ""
    if ":" not in decoded:
        return ""
    _username, password = decoded.split(":", 1)
    return password
