from __future__ import annotations

import json
import sqlite3
import uuid
from contextlib import closing
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

DB_SCHEMA_VERSION = 1


@dataclass(frozen=True)
class UploadRecord:
    id: str
    created_at: str
    tester_id: str | None
    surface: str
    evidence_grade: str
    fidelity: str
    total_tokens: int
    git_tokens: int
    git_token_share: float
    payload_sha256: str
    payload: dict[str, Any]
    schema_version: str = "unknown"
    study_id: str | None = None
    protocol_version: str | None = None
    session_id_hash: str | None = None
    time_bucket_utc: str | None = None
    repo_hash: str | None = None

    def summary(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "created_at": self.created_at,
            "schema_version": self.schema_version,
            "tester_id": self.tester_id,
            "study_id": self.study_id,
            "protocol_version": self.protocol_version,
            "session_id_hash": self.session_id_hash,
            "time_bucket_utc": self.time_bucket_utc,
            "surface": self.surface,
            "evidence_grade": self.evidence_grade,
            "fidelity": self.fidelity,
            "total_tokens": self.total_tokens,
            "git_tokens": self.git_tokens,
            "git_token_share": self.git_token_share,
            "payload_sha256": self.payload_sha256,
        }

    def to_dict(self) -> dict[str, Any]:
        data = self.summary()
        data["payload"] = self.payload
        return data

    def export_dict(self) -> dict[str, Any]:
        metrics = self.payload.get("metrics") if isinstance(self.payload, dict) else {}
        if not isinstance(metrics, dict):
            metrics = {}
        summary = metrics.get("summary") if isinstance(metrics.get("summary"), dict) else {}
        session_git_share = (
            metrics.get("session_git_share")
            if isinstance(metrics.get("session_git_share"), dict)
            else {}
        )
        token_sources = (
            metrics.get("token_sources") if isinstance(metrics.get("token_sources"), list) else []
        )
        git_workflow = (
            metrics.get("git_workflow") if isinstance(metrics.get("git_workflow"), dict) else {}
        )
        git_action_subtypes = (
            git_workflow.get("action_subtypes")
            if isinstance(git_workflow.get("action_subtypes"), list)
            else []
        )
        return {
            "id": self.id,
            "received_at": self.created_at,
            "schema_version": self.schema_version,
            "study_id": self.study_id,
            "protocol_version": self.protocol_version,
            "tester_id": self.tester_id,
            "session_id_hash": self.session_id_hash,
            "time_bucket_utc": self.time_bucket_utc,
            "repo_hash": self.repo_hash,
            "surface": self.surface,
            "evidence_grade": self.evidence_grade,
            "fidelity": self.fidelity,
            "total_tokens": self.total_tokens,
            "input_tokens": _int_from(summary, "input_tokens"),
            "output_tokens": _int_from(summary, "output_tokens"),
            "cache_read_tokens": _int_from(summary, "cache_read_tokens"),
            "cache_write_tokens": _int_from(summary, "cache_write_tokens"),
            "bytes": _int_from(summary, "bytes"),
            "git_tokens": self.git_tokens,
            "non_git_tokens": _int_from(session_git_share, "non_git_tokens"),
            "git_token_share": self.git_token_share,
            "payload_sha256": self.payload_sha256,
            "token_sources_json": _compact_json(token_sources),
            "git_action_subtypes_json": _compact_json(git_action_subtypes),
        }


class UploadStore:
    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._init_schema()

    def create(
        self,
        *,
        payload: dict[str, Any],
        payload_sha256: str,
        tester_id: str | None,
        surface: str,
        evidence_grade: str,
        fidelity: str,
        total_tokens: int,
        git_tokens: int,
        git_token_share: float,
    ) -> UploadRecord:
        upload_id = uuid.uuid4().hex
        created_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        payload_json = json.dumps(payload, sort_keys=True, separators=(",", ":"))
        metadata = _payload_metadata(payload)

        with closing(self._connect()) as db:
            with db:
                db.execute(
                    """
                    INSERT INTO uploads (
                        id, created_at, schema_version, tester_id, study_id, protocol_version,
                        session_id_hash, time_bucket_utc, repo_hash, surface, evidence_grade,
                        fidelity, total_tokens, git_tokens, git_token_share, payload_sha256,
                        payload_json
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        upload_id,
                        created_at,
                        metadata["schema_version"],
                        tester_id,
                        metadata["study_id"],
                        metadata["protocol_version"],
                        metadata["session_id_hash"],
                        metadata["time_bucket_utc"],
                        metadata["repo_hash"],
                        surface,
                        evidence_grade,
                        fidelity,
                        total_tokens,
                        git_tokens,
                        git_token_share,
                        payload_sha256,
                        payload_json,
                    ),
                )

        record = self.get(upload_id)
        if record is None:
            raise RuntimeError("created upload could not be read back")
        return record

    def get(self, upload_id: str) -> UploadRecord | None:
        with closing(self._connect()) as db:
            row = db.execute(
                "SELECT * FROM uploads WHERE id = ?",
                (upload_id,),
            ).fetchone()
        return self._record_from_row(row) if row else None

    def list(self, *, limit: int = 100, offset: int = 0) -> list[UploadRecord]:
        limit = max(1, min(limit, 500))
        offset = max(0, offset)
        with closing(self._connect()) as db:
            rows = db.execute(
                """
                SELECT * FROM uploads
                ORDER BY created_at DESC, id DESC
                LIMIT ? OFFSET ?
                """,
                (limit, offset),
            ).fetchall()
        return [self._record_from_row(row) for row in rows]

    def count(self) -> int:
        with closing(self._connect()) as db:
            row = db.execute("SELECT COUNT(*) AS count FROM uploads").fetchone()
        return int(row["count"])

    def export(self) -> list[UploadRecord]:
        with closing(self._connect()) as db:
            rows = db.execute(
                "SELECT * FROM uploads ORDER BY created_at ASC, id ASC"
            ).fetchall()
        return [self._record_from_row(row) for row in rows]

    def _connect(self) -> sqlite3.Connection:
        db = sqlite3.connect(self.path)
        db.row_factory = sqlite3.Row
        return db

    def _init_schema(self) -> None:
        with closing(self._connect()) as db:
            with db:
                db.execute("PRAGMA journal_mode=WAL")
                db.execute(
                    """
                    CREATE TABLE IF NOT EXISTS schema_migrations (
                        version INTEGER PRIMARY KEY,
                        applied_at TEXT NOT NULL
                    )
                    """
                )
                applied = {
                    int(row["version"])
                    for row in db.execute("SELECT version FROM schema_migrations")
                }
                if DB_SCHEMA_VERSION not in applied:
                    self._migrate_v1(db)
                    db.execute(
                        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?, ?)",
                        (
                            DB_SCHEMA_VERSION,
                            datetime.now(timezone.utc).isoformat(timespec="seconds"),
                        ),
                    )
                db.execute(f"PRAGMA user_version = {DB_SCHEMA_VERSION}")

    def migration_versions(self) -> list[int]:
        with closing(self._connect()) as db:
            rows = db.execute(
                "SELECT version FROM schema_migrations ORDER BY version"
            ).fetchall()
        return [int(row["version"]) for row in rows]

    def _migrate_v1(self, db: sqlite3.Connection) -> None:
        db.execute(
            """
            CREATE TABLE IF NOT EXISTS uploads (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                schema_version TEXT NOT NULL DEFAULT 'unknown',
                tester_id TEXT,
                study_id TEXT,
                protocol_version TEXT,
                session_id_hash TEXT,
                time_bucket_utc TEXT,
                repo_hash TEXT,
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
        columns = {
            row["name"] for row in db.execute("PRAGMA table_info(uploads)").fetchall()
        }
        for column, definition in {
            "schema_version": "TEXT NOT NULL DEFAULT 'unknown'",
            "study_id": "TEXT",
            "protocol_version": "TEXT",
            "session_id_hash": "TEXT",
            "time_bucket_utc": "TEXT",
            "repo_hash": "TEXT",
        }.items():
            if column not in columns:
                db.execute(f"ALTER TABLE uploads ADD COLUMN {column} {definition}")
        db.execute("CREATE INDEX IF NOT EXISTS uploads_created_at_idx ON uploads(created_at)")
        db.execute("CREATE INDEX IF NOT EXISTS uploads_surface_idx ON uploads(surface)")
        db.execute("CREATE INDEX IF NOT EXISTS uploads_fidelity_idx ON uploads(fidelity)")
        db.execute("CREATE INDEX IF NOT EXISTS uploads_study_idx ON uploads(study_id)")
        db.execute(
            "CREATE INDEX IF NOT EXISTS uploads_session_idx ON uploads(session_id_hash)"
        )

    @staticmethod
    def _record_from_row(row: sqlite3.Row) -> UploadRecord:
        return UploadRecord(
            id=row["id"],
            created_at=row["created_at"],
            schema_version=row["schema_version"],
            tester_id=row["tester_id"],
            study_id=row["study_id"],
            protocol_version=row["protocol_version"],
            session_id_hash=row["session_id_hash"],
            time_bucket_utc=row["time_bucket_utc"],
            repo_hash=row["repo_hash"],
            surface=row["surface"],
            evidence_grade=row["evidence_grade"],
            fidelity=row["fidelity"],
            total_tokens=int(row["total_tokens"]),
            git_tokens=int(row["git_tokens"]),
            git_token_share=float(row["git_token_share"]),
            payload_sha256=row["payload_sha256"],
            payload=json.loads(row["payload_json"]),
        )


def _payload_metadata(payload: dict[str, Any]) -> dict[str, str | None]:
    study = payload.get("study") if isinstance(payload.get("study"), dict) else {}
    session = payload.get("session") if isinstance(payload.get("session"), dict) else {}
    return {
        "schema_version": _optional_string(payload.get("schema_version")) or "unknown",
        "study_id": _optional_string(study.get("study_id")),
        "protocol_version": _optional_string(study.get("protocol_version")),
        "session_id_hash": _optional_string(session.get("session_id_hash")),
        "time_bucket_utc": _optional_string(session.get("time_bucket_utc")),
        "repo_hash": _optional_string(session.get("repo_hash")),
    }


def _optional_string(value: Any) -> str | None:
    return value if isinstance(value, str) else None


def _int_from(value: dict[str, Any], key: str) -> int:
    child = value.get(key)
    if isinstance(child, bool) or not isinstance(child, int):
        return 0
    return child


def _compact_json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))
