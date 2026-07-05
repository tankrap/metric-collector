import copy
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from collector_server.storage import UploadStore
from collector_server.validation import PayloadValidationError, validate_upload_payload


FIXTURE_DIR = Path(__file__).parent / "fixtures"


def valid_payload():
    with (FIXTURE_DIR / "upload-payload-v1.valid.json").open() as handle:
        return json.load(handle)


class ValidationTests(unittest.TestCase):
    def test_accepts_sanitized_aggregate_report_wrapper(self):
        upload = validate_upload_payload(valid_payload())

        self.assertEqual(upload.tester_id, "tester-a")
        self.assertEqual(upload.surface, "codex-tui")
        self.assertEqual(upload.total_tokens, 1200)
        self.assertEqual(upload.git_tokens, 240)
        self.assertEqual(upload.fidelity, "mixed")
        self.assertEqual(upload.evidence_grade, "O")
        self.assertEqual(len(upload.payload_sha256), 64)

    def test_rejects_raw_event_fields(self):
        payload = copy.deepcopy(valid_payload())
        payload["event_log"] = [{"prompt": "raw text"}]

        with self.assertRaises(PayloadValidationError):
            validate_upload_payload(payload)

    def test_rejects_forbidden_field_fixture(self):
        with (FIXTURE_DIR / "upload-payload-v1.forbidden-field.json").open() as handle:
            payload = json.load(handle)

        with self.assertRaises(PayloadValidationError):
            validate_upload_payload(payload)

    def test_rejects_sensitive_keys(self):
        payload = copy.deepcopy(valid_payload())
        payload["api_key"] = "sk-test"

        with self.assertRaises(PayloadValidationError):
            validate_upload_payload(payload)

    def test_rejects_git_tokens_greater_than_total(self):
        payload = copy.deepcopy(valid_payload())
        payload["metrics"]["session_git_share"]["git_tokens"] = 2000

        with self.assertRaises(PayloadValidationError):
            validate_upload_payload(payload)

    def test_rejects_legacy_report_wrapper(self):
        payload = {"tester_id": "tester-a", "surface": "codex-tui", "report": {}}

        with self.assertRaises(PayloadValidationError):
            validate_upload_payload(payload)


class StorageTests(unittest.TestCase):
    def test_persists_and_lists_uploads(self):
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "collector.sqlite3"
            store = UploadStore(db_path)
            upload = validate_upload_payload(valid_payload())

            created = store.create(
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

            reopened = UploadStore(db_path)
            self.assertEqual(reopened.count(), 1)
            self.assertEqual(reopened.migration_versions(), [1])
            fetched = reopened.get(created.id)
            self.assertEqual(fetched.total_tokens, 1200)
            self.assertEqual(fetched.schema_version, "vc-tokmeter.upload.v1")
            self.assertEqual(fetched.study_id, "metric-taker-t10")
            self.assertEqual(fetched.session_id_hash, "0123456789abcdef")
            self.assertEqual(reopened.list()[0].id, created.id)
            self.assertEqual(reopened.export()[0].payload["client"]["surface"], "codex-tui")

            exported = reopened.export()[0].export_dict()
            self.assertEqual(exported["received_at"], created.created_at)
            self.assertEqual(exported["study_id"], "metric-taker-t10")
            self.assertNotIn("payload", exported)
            self.assertIn("git.status", exported["git_action_subtypes_json"])

    def test_migrates_legacy_upload_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "collector.sqlite3"
            payload_json = json.dumps(valid_payload(), sort_keys=True, separators=(",", ":"))
            with sqlite3.connect(db_path) as db:
                db.execute(
                    """
                    CREATE TABLE uploads (
                        id TEXT PRIMARY KEY,
                        created_at TEXT NOT NULL,
                        tester_id TEXT,
                        surface TEXT NOT NULL,
                        evidence_grade TEXT NOT NULL,
                        fidelity TEXT NOT NULL,
                        total_tokens INTEGER NOT NULL,
                        git_tokens INTEGER NOT NULL,
                        git_token_share REAL NOT NULL,
                        payload_sha256 TEXT NOT NULL,
                        payload_json TEXT NOT NULL
                    )
                    """
                )
                db.execute(
                    """
                    INSERT INTO uploads (
                        id, created_at, tester_id, surface, evidence_grade, fidelity,
                        total_tokens, git_tokens, git_token_share, payload_sha256, payload_json
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        "legacy-upload",
                        "2026-07-05T15:30:00+00:00",
                        "tester-a",
                        "codex-tui",
                        "O",
                        "mixed",
                        1200,
                        240,
                        0.2,
                        "abc123",
                        payload_json,
                    ),
                )

            store = UploadStore(db_path)

            self.assertEqual(store.migration_versions(), [1])
            migrated = store.get("legacy-upload")
            self.assertEqual(migrated.schema_version, "unknown")
            self.assertEqual(migrated.total_tokens, 1200)


if __name__ == "__main__":
    unittest.main()
