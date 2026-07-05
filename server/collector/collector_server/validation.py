from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from typing import Any


DEFAULT_MAX_BODY_BYTES = 1_048_576
MAX_NESTING_DEPTH = 12
MAX_STRING_BYTES = 4096
MAX_LIST_LENGTH = 1000
SCHEMA_VERSION = "vc-tokmeter.upload.v1"
ARTIFACT_TYPE = "vc-tokmeter.upload"
CONSENT_VERSION = "2026-07-05"

SENSITIVE_KEY_RE = re.compile(
    r"(api[_-]?key|authorization|credential|secret|password|access[_-]?token)",
    re.IGNORECASE,
)

RAW_FIELD_NAMES = {
    "args",
    "branch",
    "code",
    "command",
    "completion",
    "completions",
    "content",
    "diff",
    "event_log",
    "messages",
    "path",
    "prompt",
    "prompts",
    "repo",
    "repository",
    "source_code",
    "stderr",
    "stdout",
    "text",
    "tool_input",
    "tool_response",
    "transcript",
}

SURFACES = {
    "codex-cli",
    "codex-tui",
    "codex-exec",
    "claude-code",
    "claude-desktop",
    "mcp-desktop",
    "other",
}
OSES = {"macos", "linux", "windows", "unknown"}
ARCHES = {"arm64", "x86_64", "aarch64", "unknown"}
EVIDENCE_GRADES = {"O", "P"}
FIDELITIES = {"exact", "estimated", "mixed", "unknown"}
WARNINGS = {
    "observational_only",
    "partial_capture",
    "mixed_fidelity",
    "unknown_token_source",
}
TOKEN_SOURCES = {
    "codex exec exact usage",
    "proxy exact usage",
    "proxy estimate",
    "mcp tool",
    "mcp tool request",
    "mcp tool response",
    "hook",
    "hook request",
    "hook response",
    "other",
}
ACTION_SUBTYPES = {
    "git.status",
    "git.diff",
    "git.log",
    "git.show",
    "git.branch",
    "git.commit",
    "git.other",
    "vc.status",
    "vc.diff",
    "vc.log",
    "vc.other",
}
DIRECTIONS = {"request", "response", "summary", "unknown"}
OPERATION_CLASSES = {
    "version_control",
    "file_interaction",
    "generated_context",
    "other",
}
SALT_SCOPES = {"client-local", "session-local", "tester-local"}

UPLOAD_ID_RE = re.compile(r"^[A-Za-z0-9_.:-]{8,128}$")
VERSION_RE = re.compile(r"^[0-9A-Za-z_.:+-]{1,64}$")
SURFACE_DETAIL_RE = re.compile(r"^[A-Za-z0-9_.:+-]+$")
STUDY_ID_RE = re.compile(r"^[A-Za-z0-9_.:-]{1,80}$")
PROTOCOL_RE = re.compile(r"^[A-Za-z0-9_.:-]{1,40}$")
HASH_RE = re.compile(r"^[a-f0-9]{12,64}$")
TIME_BUCKET_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:00:00Z$")
ALIAS_RE = re.compile(r"^[A-Za-z0-9_.@:+-]+$")


class PayloadValidationError(ValueError):
    pass


@dataclass(frozen=True)
class ValidatedUpload:
    payload: dict[str, Any]
    payload_sha256: str
    tester_id: str | None
    surface: str
    evidence_grade: str
    fidelity: str
    total_tokens: int
    git_tokens: int
    git_token_share: float


def validate_request_body(body: bytes, *, max_body_bytes: int) -> dict[str, Any]:
    if not body:
        raise PayloadValidationError("request body is required")
    if len(body) > max_body_bytes:
        raise PayloadValidationError("request body exceeds configured size limit")
    try:
        value = json.loads(body.decode("utf-8"))
    except UnicodeDecodeError as exc:
        raise PayloadValidationError("request body must be utf-8 JSON") from exc
    except json.JSONDecodeError as exc:
        raise PayloadValidationError("request body must be valid JSON") from exc
    if not isinstance(value, dict):
        raise PayloadValidationError("request body must be a JSON object")
    return value


def validate_upload_payload(value: dict[str, Any]) -> ValidatedUpload:
    reject_raw_or_sensitive_fields(value)
    _validate_envelope(value)

    consent = _dict_at(value, "consent")
    client = _dict_at(value, "client")
    metrics = _dict_at(value, "metrics")
    summary = _dict_at(metrics, "summary")
    session_git_share = _dict_at(metrics, "session_git_share")

    total_tokens = _non_negative_int(summary, "total_tokens")
    git_tokens = _non_negative_int(session_git_share, "git_tokens")
    if git_tokens > total_tokens:
        raise PayloadValidationError("metrics.session_git_share.git_tokens exceeds total tokens")

    git_token_share = _number(session_git_share, "git_token_share")
    if git_token_share < 0 or git_token_share > 1:
        raise PayloadValidationError("metrics.session_git_share.git_token_share must be 0..1")

    payload = _canonical_payload(value)

    return ValidatedUpload(
        payload=payload,
        payload_sha256=_sha256_json(payload),
        tester_id=consent.get("tester_alias"),
        surface=_string(client, "surface"),
        evidence_grade=_string(metrics, "evidence_grade"),
        fidelity=_string(metrics, "token_fidelity"),
        total_tokens=total_tokens,
        git_tokens=git_tokens,
        git_token_share=float(git_token_share),
    )


def reject_raw_or_sensitive_fields(value: Any, *, depth: int = 0) -> None:
    if depth > MAX_NESTING_DEPTH:
        raise PayloadValidationError("payload nesting is too deep")

    if isinstance(value, dict):
        for key, child in value.items():
            if not isinstance(key, str):
                raise PayloadValidationError("payload object keys must be strings")
            normalized = key.strip().lower()
            if SENSITIVE_KEY_RE.search(normalized):
                raise PayloadValidationError(f"sensitive field is not allowed: {key}")
            if normalized in RAW_FIELD_NAMES:
                raise PayloadValidationError(f"raw field is not allowed: {key}")
            reject_raw_or_sensitive_fields(child, depth=depth + 1)
        return

    if isinstance(value, list):
        if len(value) > MAX_LIST_LENGTH:
            raise PayloadValidationError("payload list is too large")
        for child in value:
            reject_raw_or_sensitive_fields(child, depth=depth + 1)
        return

    if isinstance(value, str) and len(value.encode("utf-8")) > MAX_STRING_BYTES:
        raise PayloadValidationError("payload string exceeds field size limit")


def _validate_envelope(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "payload",
        required={
            "schema_version",
            "artifact_type",
            "created_at",
            "client",
            "consent",
            "study",
            "session",
            "metrics",
            "redaction",
        },
        optional={"upload_id"},
    )
    _const(value, "schema_version", SCHEMA_VERSION)
    _const(value, "artifact_type", ARTIFACT_TYPE)
    _string(value, "created_at")
    if "upload_id" in value:
        _match(_string(value, "upload_id"), UPLOAD_ID_RE, "upload_id")

    _validate_client(_dict_at(value, "client"))
    _validate_consent(_dict_at(value, "consent"))
    _validate_study(_dict_at(value, "study"))
    _validate_session(_dict_at(value, "session"))
    _validate_metrics(_dict_at(value, "metrics"))
    _validate_redaction(_dict_at(value, "redaction"))


def _validate_client(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "client",
        required={"tokmeter_version", "surface", "platform"},
        optional={"surface_detail"},
    )
    _match(_string(value, "tokmeter_version"), VERSION_RE, "client.tokmeter_version")
    _enum(_string(value, "surface"), SURFACES, "client.surface")
    if "surface_detail" in value:
        detail = _string(value, "surface_detail")
        if len(detail) > 64:
            raise PayloadValidationError("client.surface_detail exceeds max length")
        _match(detail, SURFACE_DETAIL_RE, "client.surface_detail")

    platform = _dict_at(value, "platform")
    _assert_keys(platform, "client.platform", required={"os", "arch"})
    _enum(_string(platform, "os"), OSES, "client.platform.os")
    _enum(_string(platform, "arch"), ARCHES, "client.platform.arch")


def _validate_consent(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "consent",
        required={"upload_opt_in", "consent_version"},
        optional={"tester_alias", "contact_ok"},
    )
    if value.get("upload_opt_in") is not True:
        raise PayloadValidationError("consent.upload_opt_in must be true")
    _const(value, "consent_version", CONSENT_VERSION)
    if "tester_alias" in value:
        alias = _string(value, "tester_alias")
        if not 1 <= len(alias) <= 80:
            raise PayloadValidationError("consent.tester_alias must be 1..80 characters")
        _match(alias, ALIAS_RE, "consent.tester_alias")
    if "contact_ok" in value and not isinstance(value["contact_ok"], bool):
        raise PayloadValidationError("consent.contact_ok must be boolean")


def _validate_study(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "study",
        required={"study_id", "protocol_version"},
        optional={"cohort"},
    )
    _match(_string(value, "study_id"), STUDY_ID_RE, "study.study_id")
    _match(_string(value, "protocol_version"), PROTOCOL_RE, "study.protocol_version")
    if "cohort" in value:
        _match(_string(value, "cohort"), STUDY_ID_RE, "study.cohort")


def _validate_session(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "session",
        required={"session_id_hash", "time_bucket_utc", "duration_seconds"},
        optional={"repo_hash", "timezone_offset_minutes"},
    )
    _match(_string(value, "session_id_hash"), HASH_RE, "session.session_id_hash")
    if "repo_hash" in value:
        _match(_string(value, "repo_hash"), HASH_RE, "session.repo_hash")
    _match(_string(value, "time_bucket_utc"), TIME_BUCKET_RE, "session.time_bucket_utc")
    _non_negative_int(value, "duration_seconds")
    if "timezone_offset_minutes" in value:
        offset = _int(value, "timezone_offset_minutes")
        if offset < -840 or offset > 840:
            raise PayloadValidationError("session.timezone_offset_minutes must be -840..840")


def _validate_metrics(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "metrics",
        required={
            "evidence_grade",
            "token_fidelity",
            "summary",
            "session_git_share",
            "token_sources",
            "git_workflow",
        },
        optional={"warnings"},
    )
    _enum(_string(value, "evidence_grade"), EVIDENCE_GRADES, "metrics.evidence_grade")
    _enum(_string(value, "token_fidelity"), FIDELITIES, "metrics.token_fidelity")
    _validate_summary(_dict_at(value, "summary"), "metrics.summary")
    _validate_session_git_share(_dict_at(value, "session_git_share"))
    _validate_token_sources(_list_at(value, "token_sources"))
    _validate_git_workflow(_dict_at(value, "git_workflow"))
    if "warnings" in value:
        for warning in _list_at(value, "warnings"):
            _enum(_as_string(warning, "metrics.warnings[]"), WARNINGS, "metrics.warnings[]")


def _validate_summary(value: dict[str, Any], path: str) -> None:
    fields = {
        "runs",
        "tasks",
        "events",
        "total_tokens",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "bytes",
    }
    _assert_keys(value, path, required=fields)
    for field in fields:
        _non_negative_int(value, field)


def _validate_session_git_share(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "metrics.session_git_share",
        required={
            "total_tokens",
            "git_tokens",
            "non_git_tokens",
            "git_token_share",
            "fidelity",
        },
    )
    for field in ("total_tokens", "git_tokens", "non_git_tokens"):
        _non_negative_int(value, field)
    _ratio(value, "git_token_share", "metrics.session_git_share.git_token_share")
    _enum(_string(value, "fidelity"), FIDELITIES, "metrics.session_git_share.fidelity")
    total = _non_negative_int(value, "total_tokens")
    git = _non_negative_int(value, "git_tokens")
    non_git = _non_negative_int(value, "non_git_tokens")
    if git + non_git != total:
        raise PayloadValidationError("metrics.session_git_share totals do not add up")


def _validate_token_sources(rows: list[Any]) -> None:
    for index, raw in enumerate(rows):
        if not isinstance(raw, dict):
            raise PayloadValidationError("metrics.token_sources[] must be objects")
        path = f"metrics.token_sources[{index}]"
        _assert_keys(
            raw,
            path,
            required={"source", "events", "total_tokens", "token_share"},
            optional={
                "input_tokens",
                "output_tokens",
                "cache_read_tokens",
                "cache_write_tokens",
                "bytes",
            },
        )
        _enum(_string(raw, "source"), TOKEN_SOURCES, f"{path}.source")
        _non_negative_int(raw, "events")
        _non_negative_int(raw, "total_tokens")
        for field in (
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "bytes",
        ):
            if field in raw:
                _non_negative_int(raw, field)
        _ratio(raw, "token_share", f"{path}.token_share")


def _validate_git_workflow(value: dict[str, Any]) -> None:
    _assert_keys(value, "metrics.git_workflow", required={"totals", "action_subtypes"})
    _validate_summary(_dict_at(value, "totals"), "metrics.git_workflow.totals")
    for index, raw in enumerate(_list_at(value, "action_subtypes")):
        if not isinstance(raw, dict):
            raise PayloadValidationError("metrics.git_workflow.action_subtypes[] must be objects")
        path = f"metrics.git_workflow.action_subtypes[{index}]"
        _assert_keys(
            raw,
            path,
            required={
                "action_subtype",
                "direction",
                "operation_class",
                "events",
                "total_tokens",
                "bytes",
            },
            optional={
                "input_tokens",
                "output_tokens",
                "cache_read_tokens",
                "cache_write_tokens",
                "token_share",
            },
        )
        _enum(_string(raw, "action_subtype"), ACTION_SUBTYPES, f"{path}.action_subtype")
        _enum(_string(raw, "direction"), DIRECTIONS, f"{path}.direction")
        _enum(_string(raw, "operation_class"), OPERATION_CLASSES, f"{path}.operation_class")
        for field in (
            "events",
            "total_tokens",
            "bytes",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
        ):
            if field in raw:
                _non_negative_int(raw, field)
        if "token_share" in raw:
            _ratio(raw, "token_share", f"{path}.token_share")


def _validate_redaction(value: dict[str, Any]) -> None:
    _assert_keys(
        value,
        "redaction",
        required={"source_artifact", "digest_hex_chars", "private_data_policy"},
        optional={"salt_scope"},
    )
    _const(value, "source_artifact", "report-share")
    digest_chars = _int(value, "digest_hex_chars")
    if digest_chars < 8 or digest_chars > 24:
        raise PayloadValidationError("redaction.digest_hex_chars must be 8..24")
    if "salt_scope" in value:
        _enum(_string(value, "salt_scope"), SALT_SCOPES, "redaction.salt_scope")
    _const(value, "private_data_policy", "aggregate-only-no-raw-content")


def _assert_keys(
    value: dict[str, Any],
    path: str,
    *,
    required: set[str],
    optional: set[str] | None = None,
) -> None:
    optional = optional or set()
    missing = sorted(required - value.keys())
    if missing:
        raise PayloadValidationError(f"{path} missing required field: {missing[0]}")
    allowed = required | optional
    extra = sorted(set(value) - allowed)
    if extra:
        raise PayloadValidationError(f"{path} has unexpected field: {extra[0]}")


def _canonical_payload(value: dict[str, Any]) -> dict[str, Any]:
    return json.loads(json.dumps(value, sort_keys=True, separators=(",", ":")))


def _sha256_json(value: dict[str, Any]) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _dict_at(value: dict[str, Any], key: str) -> dict[str, Any]:
    child = value.get(key)
    if not isinstance(child, dict):
        raise PayloadValidationError(f"{key} is required")
    return child


def _list_at(value: dict[str, Any], key: str) -> list[Any]:
    child = value.get(key)
    if not isinstance(child, list):
        raise PayloadValidationError(f"{key} must be an array")
    if len(child) > MAX_LIST_LENGTH:
        raise PayloadValidationError(f"{key} list is too large")
    return child


def _non_negative_int(value: dict[str, Any], key: str) -> int:
    child = _int(value, key)
    if child < 0:
        raise PayloadValidationError(f"{key} must be a non-negative integer")
    return child


def _int(value: dict[str, Any], key: str) -> int:
    child = value.get(key)
    if not isinstance(child, int) or isinstance(child, bool):
        raise PayloadValidationError(f"{key} must be an integer")
    return child


def _number(value: dict[str, Any], key: str) -> float:
    child = value.get(key)
    if not isinstance(child, (int, float)) or isinstance(child, bool):
        raise PayloadValidationError(f"{key} must be a number")
    return float(child)


def _ratio(value: dict[str, Any], key: str, path: str) -> float:
    number = _number(value, key)
    if number < 0 or number > 1:
        raise PayloadValidationError(f"{path} must be 0..1")
    return number


def _string(value: dict[str, Any], key: str) -> str:
    return _as_string(value.get(key), key)


def _as_string(value: Any, path: str) -> str:
    if not isinstance(value, str) or len(value.encode("utf-8")) > 128:
        raise PayloadValidationError(f"{path} must be a short string")
    return value


def _const(value: dict[str, Any], key: str, expected: str) -> None:
    if value.get(key) != expected:
        raise PayloadValidationError(f"{key} must be {expected}")


def _enum(value: str, allowed: set[str], path: str) -> None:
    if value not in allowed:
        raise PayloadValidationError(f"{path} has unsupported value")


def _match(value: str, pattern: re.Pattern[str], path: str) -> None:
    if not pattern.match(value):
        raise PayloadValidationError(f"{path} has invalid format")
