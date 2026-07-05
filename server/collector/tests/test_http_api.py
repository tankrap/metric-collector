import copy
import http.client
import json
import tempfile
import threading
import unittest
from pathlib import Path

from collector_server.app import CollectorConfig, RateLimiter, make_server, _tokens_from_sources


FIXTURE_DIR = Path(__file__).parent / "fixtures"


def valid_payload():
    with (FIXTURE_DIR / "upload-payload-v1.valid.json").open() as handle:
        payload = json.load(handle)
    payload["client"]["surface"] = "claude-code"
    payload["consent"]["tester_alias"] = "tester-http"
    payload["metrics"]["token_fidelity"] = "estimated"
    payload["metrics"]["session_git_share"]["fidelity"] = "estimated"
    payload["metrics"]["summary"]["total_tokens"] = 3000
    payload["metrics"]["summary"]["input_tokens"] = 2100
    payload["metrics"]["summary"]["output_tokens"] = 900
    payload["metrics"]["session_git_share"]["total_tokens"] = 3000
    payload["metrics"]["session_git_share"]["git_tokens"] = 900
    payload["metrics"]["session_git_share"]["non_git_tokens"] = 2100
    payload["metrics"]["session_git_share"]["git_token_share"] = 0.3
    return payload


class CollectorApiTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        db_path = Path(self.tmp.name) / "collector.sqlite3"
        config = CollectorConfig(
            db_path=str(db_path),
            upload_token="upload-test-token",
            admin_token="admin-test-token",
            rate_limit_requests=100,
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
        if content_type == "application/json":
            parsed = json.loads(data.decode("utf-8"))
        else:
            parsed = data.decode("utf-8")
        return response.status, content_type, parsed

    def upload_headers(self):
        return {"authorization": "Bearer upload-test-token"}

    def admin_headers(self):
        return {"authorization": "Bearer admin-test-token"}

    def test_health_create_get_list_and_export(self):
        status, _, body = self.request("GET", "/health")
        self.assertEqual(status, 200)
        self.assertEqual(body["status"], "ok")

        status, _, created = self.request(
            "POST",
            "/v1/uploads",
            valid_payload(),
            headers=self.upload_headers(),
        )
        self.assertEqual(status, 201)
        self.assertEqual(created["surface"], "claude-code")
        self.assertEqual(created["total_tokens"], 3000)

        status, _, fetched = self.request(
            "GET",
            f"/v1/uploads/{created['id']}",
            headers=self.admin_headers(),
        )
        self.assertEqual(status, 200)
        self.assertEqual(fetched["payload"]["consent"]["tester_alias"], "tester-http")

        status, _, listed = self.request("GET", "/v1/uploads", headers=self.admin_headers())
        self.assertEqual(status, 200)
        self.assertEqual(listed["count"], 1)
        self.assertEqual(listed["uploads"][0]["id"], created["id"])

        status, _, exported = self.request(
            "GET",
            "/v1/uploads/export",
            headers=self.admin_headers(),
        )
        self.assertEqual(status, 200)
        self.assertEqual(exported["count"], 1)
        self.assertEqual(exported["uploads"][0]["id"], created["id"])
        self.assertEqual(exported["uploads"][0]["study_id"], "metric-taker-t10")
        self.assertNotIn("payload", exported["uploads"][0])
        self.assertIn("git_action_subtypes_json", exported["uploads"][0])

    def test_export_ndjson(self):
        self.request("POST", "/v1/uploads", valid_payload(), headers=self.upload_headers())

        status, content_type, body = self.request(
            "GET",
            "/v1/uploads/export?format=ndjson",
            headers={"x-admin-token": "admin-test-token"},
        )

        self.assertEqual(status, 200)
        self.assertEqual(content_type, "application/x-ndjson")
        self.assertIn('"surface": "claude-code"', body)
        self.assertNotIn('"payload"', body)

    def test_export_csv(self):
        self.request("POST", "/v1/uploads", valid_payload(), headers=self.upload_headers())

        status, content_type, body = self.request(
            "GET",
            "/v1/uploads/export?format=csv",
            headers=self.admin_headers(),
        )

        self.assertEqual(status, 200)
        self.assertEqual(content_type, "text/csv")
        self.assertIn("id,received_at,schema_version", body)
        self.assertIn("claude-code", body)
        self.assertNotIn("payload_json", body)
        self.assertNotIn("prompt", body)

    def test_rejects_invalid_payload(self):
        payload = copy.deepcopy(valid_payload())
        payload["prompt"] = "raw prompt"

        status, _, body = self.request(
            "POST",
            "/v1/uploads",
            payload,
            headers=self.upload_headers(),
        )

        self.assertEqual(status, 400)
        self.assertEqual(body["error"]["code"], "invalid_payload")

    def test_upload_and_admin_endpoints_require_tokens(self):
        status, _, body = self.request("POST", "/v1/uploads", valid_payload())
        self.assertEqual(status, 401)
        self.assertEqual(body["error"]["code"], "unauthorized")

        status, _, body = self.request("GET", "/v1/uploads")
        self.assertEqual(status, 401)
        self.assertEqual(body["error"]["code"], "unauthorized")

    def test_rejects_upload_when_auth_is_not_configured(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

        db_path = Path(self.tmp.name) / "no-auth.sqlite3"
        config = CollectorConfig(db_path=str(db_path), admin_token="admin-test-token")
        self.server = make_server("127.0.0.1", 0, config)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.host, self.port = self.server.server_address

        status, _, body = self.request("POST", "/v1/uploads", valid_payload())

        self.assertEqual(status, 503)
        self.assertEqual(body["error"]["code"], "upload_auth_not_configured")

    def test_rejects_large_payload_before_validation(self):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=5)
        conn.request(
            "POST",
            "/v1/uploads",
            body=b"",
            headers={
                "content-type": "application/json",
                "content-length": str(self.server.config.max_body_bytes + 1),
                **self.upload_headers(),
            },
        )
        response = conn.getresponse()
        body = json.loads(response.read().decode("utf-8"))
        conn.close()

        self.assertEqual(response.status, 413)
        self.assertEqual(body["error"]["code"], "payload_too_large")

    def test_rate_limits_repeated_upload_attempts(self):
        self.server.rate_limiter = RateLimiter(limit=1, window_seconds=60)

        first_status, _, _ = self.request(
            "POST",
            "/v1/uploads",
            valid_payload(),
            headers=self.upload_headers(),
        )
        second_status, _, body = self.request(
            "POST",
            "/v1/uploads",
            valid_payload(),
            headers=self.upload_headers(),
        )

        self.assertEqual(first_status, 201)
        self.assertEqual(second_status, 429)
        self.assertEqual(body["error"]["code"], "rate_limited")

    def test_accepts_multiple_tokens_and_token_file_sources(self):
        with tempfile.NamedTemporaryFile("w", encoding="utf-8") as handle:
            handle.write("file-upload-token\nfile-admin-token\n")
            handle.flush()

            tokens = _tokens_from_sources(
                "upload-test-token,invite-token",
                None,
                handle.name,
            )

        self.assertEqual(
            tokens,
            ("upload-test-token", "invite-token", "file-upload-token", "file-admin-token"),
        )


if __name__ == "__main__":
    unittest.main()
