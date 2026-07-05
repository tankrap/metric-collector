#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import json
import os
import sys
import tempfile
import threading
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "server/collector/tests/fixtures/upload-payload-v1.valid.json"
FORBIDDEN_FIXTURE = (
    ROOT / "server/collector/tests/fixtures/upload-payload-v1.forbidden-field.json"
)
FORBIDDEN_KEYS = {
    "prompt",
    "prompts",
    "messages",
    "command",
    "stdout",
    "stderr",
    "diff",
    "path",
    "repo",
    "repository",
    "branch",
    "api_key",
    "authorization",
    "secret",
    "credential",
}


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Seed redacted uploads and verify collector upload/admin/export/dashboard APIs."
    )
    parser.add_argument("--base-url", default=os.environ.get("COLLECTOR_BASE_URL"))
    parser.add_argument(
        "--upload-token",
        default=os.environ.get("COLLECTOR_UPLOAD_TOKEN", "smoke-upload-token"),
    )
    parser.add_argument(
        "--admin-token",
        default=os.environ.get("COLLECTOR_ADMIN_TOKEN", "smoke-admin-token"),
    )
    parser.add_argument("--timeout", type=float, default=5.0)
    args = parser.parse_args()

    local = None
    base_url = args.base_url
    if not base_url:
        local = start_local_collector(args.upload_token, args.admin_token)
        base_url = local["base_url"]

    assert base_url is not None
    client = Client(base_url.rstrip("/"), timeout=args.timeout)
    try:
        run_smoke(client, args.upload_token, args.admin_token)
    finally:
        if local:
            local["server"].shutdown()
            local["server"].server_close()
            local["thread"].join(timeout=5)
            local["tmp"].cleanup()

    print(f"collector e2e smoke passed: {base_url}")
    return 0


def start_local_collector(upload_token: str, admin_token: str) -> dict[str, Any]:
    sys.path.insert(0, str(ROOT / "server/collector"))
    from collector_server.app import CollectorConfig, make_server

    tmp = tempfile.TemporaryDirectory()
    config = CollectorConfig(
        db_path=str(Path(tmp.name) / "collector.sqlite3"),
        upload_token=upload_token,
        admin_token=admin_token,
    )
    server = make_server("127.0.0.1", 0, config)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host, port = server.server_address
    return {
        "base_url": f"http://{host}:{port}",
        "server": server,
        "thread": thread,
        "tmp": tmp,
    }


def run_smoke(client: "Client", upload_token: str, admin_token: str) -> None:
    status, body, _ = client.request("GET", "/health")
    check(status == 200 and body["status"] == "ok", "health endpoint failed")

    status, body, _ = client.request("POST", "/v1/uploads", json_body=sample_payload(1))
    check(status == 401, f"upload without token should be 401, got {status}: {body}")

    created = []
    for index, surface, total_tokens, git_tokens in [
        (1, "codex-tui", 1200, 240),
        (2, "claude-code", 3000, 900),
    ]:
        status, body, _ = client.request(
            "POST",
            "/v1/uploads",
            json_body=sample_payload(index, surface, total_tokens, git_tokens),
            headers={"authorization": f"Bearer {upload_token}"},
        )
        check(status == 201, f"upload {index} failed: {status} {body}")
        created.append(body)

    status, body, _ = client.request(
        "POST",
        "/v1/uploads",
        json_body=json.loads(FORBIDDEN_FIXTURE.read_text()),
        headers={"x-upload-token": upload_token},
    )
    check(status == 400, f"forbidden fixture should be rejected, got {status}: {body}")

    status, body, _ = client.request("GET", "/v1/uploads")
    check(status == 401, f"admin list without token should be 401, got {status}: {body}")

    admin = {"authorization": f"Bearer {admin_token}"}
    status, body, _ = client.request("GET", "/v1/uploads?limit=10", headers=admin)
    check(status == 200 and body["count"] >= 2, f"admin list failed: {status} {body}")

    first_id = created[0]["id"]
    status, body, _ = client.request("GET", f"/v1/uploads/{first_id}", headers=admin)
    check(status == 200, f"upload read failed: {status} {body}")
    check(
        body["payload"]["consent"]["tester_alias"] == "smoke-tester-1",
        "stored payload alias did not match seeded upload",
    )

    status, body, _ = client.request("GET", "/v1/uploads/export", headers=admin)
    check(status == 200 and body["count"] >= 2, f"json export failed: {status} {body}")

    status, text, content_type = client.request(
        "GET",
        "/v1/uploads/export?format=ndjson",
        headers={"x-admin-token": admin_token},
    )
    check(status == 200, f"ndjson export failed: {status} {text}")
    check("application/x-ndjson" in content_type, "ndjson export content-type mismatch")
    check(first_id in text, "ndjson export did not include seeded upload")

    dashboard_paths = [
        "/v1/dashboard/summary",
        "/v1/dashboard/time-series",
        "/v1/dashboard/surfaces",
        "/v1/dashboard/fidelity",
        "/v1/dashboard/git-actions?limit=5",
    ]
    for path in dashboard_paths:
        status, body, _ = client.request("GET", path, headers=admin)
        check(status == 200, f"dashboard endpoint failed {path}: {status} {body}")
        check(body["schema_version"] == 1, f"dashboard schema missing for {path}")
        assert_no_forbidden_keys(body)

    status, summary, _ = client.request("GET", "/v1/dashboard/summary", headers=admin)
    check(summary["upload_count"] >= 2, "dashboard summary did not include seeded uploads")
    check(summary["total_tokens"] >= 4200, "dashboard summary token total too low")
    check(summary["git_tokens"] >= 1140, "dashboard summary git token total too low")

    status, actions, _ = client.request(
        "GET",
        "/v1/dashboard/git-actions?limit=5",
        headers=admin,
    )
    action_names = {row["action_subtype"] for row in actions["rows"]}
    check("git.diff" in action_names, "dashboard git-actions missing git.diff aggregate")


def sample_payload(
    index: int,
    surface: str = "codex-tui",
    total_tokens: int = 1200,
    git_tokens: int = 240,
) -> dict[str, Any]:
    payload = copy.deepcopy(json.loads(FIXTURE.read_text()))
    input_tokens = total_tokens - git_tokens
    payload["client"]["surface"] = surface
    payload["consent"]["tester_alias"] = f"smoke-tester-{index}"
    payload["session"]["session_id_hash"] = f"{index:016x}"
    payload["session"]["repo_hash"] = f"{index + 100:016x}"
    payload["metrics"]["summary"]["total_tokens"] = total_tokens
    payload["metrics"]["summary"]["input_tokens"] = input_tokens
    payload["metrics"]["summary"]["output_tokens"] = git_tokens
    payload["metrics"]["session_git_share"]["total_tokens"] = total_tokens
    payload["metrics"]["session_git_share"]["git_tokens"] = git_tokens
    payload["metrics"]["session_git_share"]["non_git_tokens"] = input_tokens
    payload["metrics"]["session_git_share"]["git_token_share"] = git_tokens / total_tokens
    payload["metrics"]["token_sources"][0]["total_tokens"] = total_tokens
    payload["metrics"]["token_sources"][0]["input_tokens"] = input_tokens
    payload["metrics"]["token_sources"][0]["output_tokens"] = git_tokens
    payload["metrics"]["token_sources"][0]["token_share"] = 1.0
    payload["metrics"]["git_workflow"]["totals"]["total_tokens"] = git_tokens
    payload["metrics"]["git_workflow"]["totals"]["input_tokens"] = min(160, git_tokens)
    payload["metrics"]["git_workflow"]["totals"]["output_tokens"] = max(0, git_tokens - 160)
    payload["metrics"]["git_workflow"]["action_subtypes"][0]["total_tokens"] = min(80, git_tokens)
    payload["metrics"]["git_workflow"]["action_subtypes"][0]["input_tokens"] = min(80, git_tokens)
    payload["metrics"]["git_workflow"]["action_subtypes"][0]["output_tokens"] = 0
    payload["metrics"]["git_workflow"]["action_subtypes"][0]["token_share"] = (
        min(80, git_tokens) / total_tokens
    )
    payload["metrics"]["git_workflow"]["action_subtypes"][1]["total_tokens"] = max(
        0, git_tokens - min(80, git_tokens)
    )
    payload["metrics"]["git_workflow"]["action_subtypes"][1]["input_tokens"] = 0
    payload["metrics"]["git_workflow"]["action_subtypes"][1]["output_tokens"] = max(
        0, git_tokens - min(80, git_tokens)
    )
    payload["metrics"]["git_workflow"]["action_subtypes"][1]["token_share"] = (
        max(0, git_tokens - min(80, git_tokens)) / total_tokens
    )
    return payload


class Client:
    def __init__(self, base_url: str, *, timeout: float):
        self.base_url = base_url
        self.timeout = timeout

    def request(
        self,
        method: str,
        path: str,
        *,
        json_body: dict[str, Any] | None = None,
        headers: dict[str, str] | None = None,
    ) -> tuple[int, Any, str]:
        body = None
        request_headers = dict(headers or {})
        if json_body is not None:
            body = json.dumps(json_body).encode("utf-8")
            request_headers["content-type"] = "application/json"
        req = urllib.request.Request(
            f"{self.base_url}{path}",
            data=body,
            headers=request_headers,
            method=method,
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as response:
                return parse_response(response)
        except urllib.error.HTTPError as exc:
            return parse_response(exc)


def parse_response(response: Any) -> tuple[int, Any, str]:
    data = response.read()
    content_type = response.headers.get("content-type", "")
    media_type = content_type.split(";", 1)[0].strip().lower()
    if media_type == "application/json" or media_type.endswith("+json"):
        return response.status, json.loads(data.decode("utf-8")), content_type
    return response.status, data.decode("utf-8"), content_type


def assert_no_forbidden_keys(value: Any) -> None:
    if isinstance(value, dict):
        overlap = FORBIDDEN_KEYS.intersection(value.keys())
        check(not overlap, f"dashboard response exposed forbidden keys: {sorted(overlap)}")
        for child in value.values():
            assert_no_forbidden_keys(child)
    elif isinstance(value, list):
        for child in value:
            assert_no_forbidden_keys(child)


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


if __name__ == "__main__":
    raise SystemExit(main())
