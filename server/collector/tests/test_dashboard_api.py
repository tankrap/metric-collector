import base64
import http.client
import json
import tempfile
import threading
import unittest
from datetime import datetime, timezone
from pathlib import Path

from collector_server.app import CollectorConfig, make_server
from collector_server.dashboard import (
    dashboard_fidelity_distribution,
    dashboard_summary,
    dashboard_surface_distribution,
    dashboard_time_series,
    dashboard_top_git_actions,
)
from collector_server.storage import UploadRecord


FIXTURE_DIR = Path(__file__).parent / "fixtures"


FORBIDDEN_RESPONSE_KEYS = {
    "id",
    "payload",
    "payload_sha256",
    "tester_id",
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


def payload(surface="codex-tui", total_tokens=1000, git_tokens=250, fidelity="mixed"):
    with (FIXTURE_DIR / "upload-payload-v1.valid.json").open() as handle:
        body = json.load(handle)
    body["client"]["surface"] = surface
    body["consent"]["tester_alias"] = "tester-private"
    body["metrics"]["token_fidelity"] = fidelity
    body["metrics"]["summary"]["total_tokens"] = total_tokens
    body["metrics"]["summary"]["input_tokens"] = total_tokens - git_tokens
    body["metrics"]["summary"]["output_tokens"] = git_tokens
    body["metrics"]["session_git_share"]["total_tokens"] = total_tokens
    body["metrics"]["session_git_share"]["git_tokens"] = git_tokens
    body["metrics"]["session_git_share"]["non_git_tokens"] = total_tokens - git_tokens
    body["metrics"]["session_git_share"]["git_token_share"] = git_tokens / total_tokens
    body["metrics"]["session_git_share"]["fidelity"] = fidelity
    body["metrics"]["token_sources"][0]["total_tokens"] = total_tokens
    body["metrics"]["token_sources"][0]["token_share"] = 1.0
    body["metrics"]["git_workflow"]["totals"]["events"] = 3
    body["metrics"]["git_workflow"]["totals"]["total_tokens"] = git_tokens
    body["metrics"]["git_workflow"]["totals"]["input_tokens"] = 80
    body["metrics"]["git_workflow"]["totals"]["output_tokens"] = git_tokens - 80
    body["metrics"]["git_workflow"]["action_subtypes"] = [
        {
            "action_subtype": "git.status",
            "direction": "request",
            "operation_class": "version_control",
            "events": 2,
            "input_tokens": 80,
            "output_tokens": 0,
            "total_tokens": 80,
            "bytes": 512,
            "token_share": 80 / total_tokens,
        },
        {
            "action_subtype": "git.diff",
            "direction": "response",
            "operation_class": "version_control",
            "events": 1,
            "input_tokens": 0,
            "output_tokens": git_tokens - 80,
            "total_tokens": git_tokens - 80,
            "bytes": 512,
            "token_share": (git_tokens - 80) / total_tokens,
        },
    ]
    return body


def record(
    *,
    created_at="2026-07-05T12:00:00+00:00",
    surface="codex-tui",
    total_tokens=1000,
    git_tokens=250,
    fidelity="mixed",
):
    body = payload(
        surface=surface,
        total_tokens=total_tokens,
        git_tokens=git_tokens,
        fidelity=fidelity,
    )
    return UploadRecord(
        id="private-id",
        created_at=created_at,
        tester_id="private-tester",
        surface=surface,
        evidence_grade="O",
        fidelity=fidelity,
        total_tokens=total_tokens,
        git_tokens=git_tokens,
        git_token_share=git_tokens / total_tokens,
        payload_sha256="private-sha",
        payload=body,
    )


def assert_no_forbidden_fields(testcase, value):
    if isinstance(value, dict):
        testcase.assertTrue(FORBIDDEN_RESPONSE_KEYS.isdisjoint(value.keys()), value.keys())
        for child in value.values():
            assert_no_forbidden_fields(testcase, child)
    elif isinstance(value, list):
        for child in value:
            assert_no_forbidden_fields(testcase, child)


class DashboardAggregationTests(unittest.TestCase):
    def test_summary_time_series_distributions_and_actions(self):
        records = [
            record(surface="codex-tui", total_tokens=1000, git_tokens=250, fidelity="mixed"),
            record(surface="claude-code", total_tokens=2000, git_tokens=1000, fidelity="exact"),
        ]

        summary = dashboard_summary(records)
        self.assertEqual(summary["upload_count"], 2)
        self.assertEqual(summary["total_tokens"], 3000)
        self.assertEqual(summary["git_tokens"], 1250)
        self.assertAlmostEqual(summary["git_token_share"], 1250 / 3000)
        self.assertEqual(summary["latest_upload"]["surface"], "codex-tui")

        series = dashboard_time_series(records)
        self.assertEqual(series["bucket"], "day")
        self.assertEqual(series["rows"][0]["bucket"], "2026-07-05")
        self.assertEqual(series["rows"][0]["total_tokens"], 3000)

        surfaces = dashboard_surface_distribution(records)
        self.assertEqual({row["surface"] for row in surfaces["rows"]}, {"codex-tui", "claude-code"})

        fidelity = dashboard_fidelity_distribution(records)
        self.assertEqual({row["fidelity"] for row in fidelity["rows"]}, {"mixed", "exact"})

        actions = dashboard_top_git_actions(records, limit=1)
        self.assertEqual(actions["rows"][0]["action_subtype"], "git.diff")
        self.assertEqual(actions["rows"][0]["total_tokens"], 1090)

        for response in [summary, series, surfaces, fidelity, actions]:
            assert_no_forbidden_fields(self, response)

    def test_window_filtering(self):
        now = datetime(2026, 7, 5, 12, tzinfo=timezone.utc)
        records = [
            record(created_at="2026-07-04T12:00:00+00:00", total_tokens=1000, git_tokens=200),
            record(created_at="2026-06-01T12:00:00+00:00", total_tokens=9000, git_tokens=900),
        ]

        summary = dashboard_summary(records, window="7d", now=now)

        self.assertEqual(summary["upload_count"], 1)
        self.assertEqual(summary["total_tokens"], 1000)


class DashboardHttpTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        db_path = Path(self.tmp.name) / "collector.sqlite3"
        config = CollectorConfig(
            db_path=str(db_path),
            upload_token="upload-test-token",
            admin_token="admin-test-token",
        )
        self.server = make_server("127.0.0.1", 0, config)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.host, self.port = self.server.server_address

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.tmp.cleanup()

    def request(self, method, path, body=None, headers=None):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=5)
        encoded = None
        if body is not None:
            encoded = json.dumps(body).encode("utf-8")
            headers = {"content-type": "application/json", **(headers or {})}
        conn.request(method, path, body=encoded, headers=headers or {})
        response = conn.getresponse()
        data = response.read()
        content_type = response.getheader("content-type") or ""
        conn.close()
        if "json" in content_type:
            parsed = json.loads(data.decode("utf-8"))
        else:
            parsed = data.decode("utf-8")
        return response.status, parsed

    def auth_headers(self):
        return {"authorization": "Bearer admin-test-token"}

    def upload_headers(self):
        return {"authorization": "Bearer upload-test-token"}

    def basic_auth_headers(self):
        token = base64.b64encode(b"admin:admin-test-token").decode("ascii")
        return {"authorization": f"Basic {token}"}

    def test_dashboard_requires_admin_auth(self):
        self.request("POST", "/v1/uploads", payload(), headers=self.upload_headers())

        status, body = self.request("GET", "/v1/dashboard/summary")

        self.assertEqual(status, 401)
        self.assertEqual(body["error"]["code"], "unauthorized")

    def test_web_dashboard_requires_admin_auth(self):
        self.request("POST", "/v1/uploads", payload(), headers=self.upload_headers())

        status, body = self.request("GET", "/dashboard")

        self.assertEqual(status, 401)
        self.assertIn("Unauthorized", body)

    def test_dashboard_endpoints_return_aggregate_only_data(self):
        self.request("POST", "/v1/uploads", payload(), headers=self.upload_headers())
        self.request(
            "POST",
            "/v1/uploads",
            payload(surface="claude-code", total_tokens=2000, git_tokens=500, fidelity="exact"),
            headers=self.upload_headers(),
        )

        for path in [
            "/v1/dashboard/summary",
            "/v1/dashboard/time-series",
            "/v1/dashboard/surfaces",
            "/v1/dashboard/fidelity",
            "/v1/dashboard/git-actions?limit=5",
        ]:
            status, body = self.request("GET", path, headers=self.auth_headers())
            self.assertEqual(status, 200, path)
            self.assertEqual(body["schema_version"], 1)
            assert_no_forbidden_fields(self, body)

        _, summary = self.request("GET", "/v1/dashboard/summary", headers=self.auth_headers())
        self.assertEqual(summary["upload_count"], 2)
        self.assertEqual(summary["total_tokens"], 3000)
        self.assertEqual(summary["git_tokens"], 750)
        self.assertNotIn("tester_id", json.dumps(summary))

        _, actions = self.request(
            "GET",
            "/v1/dashboard/git-actions?limit=1",
            headers={"x-admin-token": "admin-test-token"},
        )
        self.assertEqual(actions["rows"][0]["action_subtype"], "git.diff")

    def test_web_dashboard_renders_aggregate_only_data(self):
        self.request("POST", "/v1/uploads", payload(), headers=self.upload_headers())
        self.request(
            "POST",
            "/v1/uploads",
            payload(surface="claude-code", total_tokens=2000, git_tokens=500, fidelity="exact"),
            headers=self.upload_headers(),
        )

        status, body = self.request(
            "GET",
            "/dashboard?window=30d",
            headers=self.basic_auth_headers(),
        )

        self.assertEqual(status, 200)
        self.assertIn("Metric Taker Dashboard", body)
        self.assertIn("Aggregate study metrics only", body)
        self.assertIn(">3,000<", body)
        self.assertIn(">750<", body)
        self.assertIn("codex-tui", body)
        self.assertIn("claude-code", body)
        self.assertIn("git.diff", body)
        self.assertIn("/v1/uploads/export", body)
        self.assertIn("format=ndjson", body)
        self.assertNotIn("tester-private", body)
        self.assertNotIn("private-id", body)
        self.assertNotIn("private-sha", body)
        self.assertNotIn("session_id_hash", body)
        self.assertNotIn("payload_sha256", body)

    def test_basic_auth_works_for_dashboard_api_links(self):
        self.request("POST", "/v1/uploads", payload(), headers=self.upload_headers())

        status, summary = self.request(
            "GET",
            "/v1/dashboard/summary",
            headers=self.basic_auth_headers(),
        )

        self.assertEqual(status, 200)
        self.assertEqual(summary["upload_count"], 1)


if __name__ == "__main__":
    unittest.main()
