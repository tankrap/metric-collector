use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use crate::core::{
    ADHOC_TASK_ID, AttributedTokenEvent, CaptureMode, EventLog, EventLogError, OperationClass,
    TokenCounts,
};

const DEFAULT_ADAPTER: &str = "import.codex.exec";
const DEFAULT_PROFILE_ID: &str = "adhoc";
const DEFAULT_RUN_ID: &str = "imported-codex-exec";
const CODEX_EXEC_TOOL: &str = "codex.exec";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredUsageDefaults {
    pub timestamp_ms: u64,
    pub run_id: String,
    pub task_id: String,
    pub profile_id: String,
    pub adapter: String,
}

impl Default for StructuredUsageDefaults {
    fn default() -> Self {
        Self {
            timestamp_ms: 1,
            run_id: DEFAULT_RUN_ID.to_owned(),
            task_id: ADHOC_TASK_ID.to_owned(),
            profile_id: DEFAULT_PROFILE_ID.to_owned(),
            adapter: DEFAULT_ADAPTER.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructuredUsageImport {
    pub events: Vec<AttributedTokenEvent>,
    pub diagnostics: Vec<StructuredImportDiagnostic>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructuredImportAppendSummary {
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub diagnostics: Vec<StructuredImportDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredImportDiagnostic {
    pub line: usize,
    pub kind: StructuredImportDiagnosticKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuredImportDiagnosticKind {
    MalformedJsonlRecord,
    UnsupportedRecord,
    MissingUsage,
    ZeroUsage,
    InvalidEvent,
}

impl fmt::Display for StructuredImportDiagnosticKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MalformedJsonlRecord => "malformed_jsonl_record",
            Self::UnsupportedRecord => "unsupported_record",
            Self::MissingUsage => "missing_usage",
            Self::ZeroUsage => "zero_usage",
            Self::InvalidEvent => "invalid_event",
        })
    }
}

pub fn import_codex_exec_jsonl(
    input: &str,
    defaults: &StructuredUsageDefaults,
) -> StructuredUsageImport {
    let mut imported = StructuredUsageImport::default();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        let value = match serde_json::from_str::<Value>(trimmed) {
            Ok(value @ Value::Object(_)) => value,
            Ok(_) => {
                imported.diagnostics.push(diagnostic(
                    line_number,
                    StructuredImportDiagnosticKind::MalformedJsonlRecord,
                ));
                continue;
            }
            Err(_) => {
                imported.diagnostics.push(diagnostic(
                    line_number,
                    StructuredImportDiagnosticKind::MalformedJsonlRecord,
                ));
                continue;
            }
        };

        match codex_exec_usage_event(&value, trimmed, defaults) {
            Ok(Some(event)) => imported.events.push(event),
            Ok(None) => imported.diagnostics.push(diagnostic(
                line_number,
                StructuredImportDiagnosticKind::UnsupportedRecord,
            )),
            Err(kind) => imported.diagnostics.push(diagnostic(line_number, kind)),
        }
    }

    imported
}

pub fn append_codex_exec_jsonl_to_event_log(
    input: &str,
    event_log_path: impl AsRef<Path>,
    defaults: &StructuredUsageDefaults,
) -> io::Result<StructuredImportAppendSummary> {
    let imported = import_codex_exec_jsonl(input, defaults);
    let event_log_path = event_log_path.as_ref();
    let mut seen_digests = existing_digests(event_log_path)?;
    let mut summary = StructuredImportAppendSummary {
        diagnostics: imported.diagnostics,
        ..StructuredImportAppendSummary::default()
    };

    if let Some(parent) = event_log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    for event in imported.events {
        if !seen_digests.insert(event.content_digest.clone()) {
            summary.skipped_duplicates += 1;
            continue;
        }

        EventLog::append_event_to_file(event_log_path, &event)
            .map_err(event_log_error_to_io_error)?;
        summary.imported += 1;
    }

    Ok(summary)
}

fn codex_exec_usage_event(
    value: &Value,
    raw_line: &str,
    defaults: &StructuredUsageDefaults,
) -> Result<Option<AttributedTokenEvent>, StructuredImportDiagnosticKind> {
    if !is_codex_turn_completed(value) {
        return Ok(None);
    }

    let usage = usage_value(value).ok_or(StructuredImportDiagnosticKind::MissingUsage)?;
    let tokens = usage_token_counts(usage).ok_or(StructuredImportDiagnosticKind::ZeroUsage)?;
    let timestamp_ms = u64_at_paths(
        value,
        &[
            &["timestamp_ms"],
            &["time_ms"],
            &["created_at_ms"],
            &["created_ms"],
        ],
    )
    .unwrap_or(defaults.timestamp_ms);
    let run_id = string_at_paths(
        value,
        &[
            &["session_id"],
            &["conversation_id"],
            &["run_id"],
            &["session", "id"],
        ],
    )
    .unwrap_or_else(|| defaults.run_id.clone());
    let task_id = string_at_paths(value, &[&["task_id"], &["tokmeter_task_id"]])
        .unwrap_or_else(|| defaults.task_id.clone());
    let profile_id = string_at_paths(value, &[&["profile_id"], &["profile"]])
        .unwrap_or_else(|| defaults.profile_id.clone());
    let digest = usage_digest(value, usage, raw_line);

    let event = AttributedTokenEvent {
        timestamp_ms,
        mode: CaptureMode::Passive,
        run_id,
        task_id,
        profile_id,
        adapter: defaults.adapter.clone(),
        operation_class: OperationClass::SessionMeta,
        tool: CODEX_EXEC_TOOL.to_owned(),
        tokens,
        byte_count: raw_line.len().try_into().unwrap_or(u64::MAX),
        content_digest: digest,
        repeat_of: None,
        action_subtype: None,
        direction: Some("summary".to_owned()),
    };

    event
        .validate()
        .map_err(|_| StructuredImportDiagnosticKind::InvalidEvent)?;
    Ok(Some(event))
}

fn is_codex_turn_completed(value: &Value) -> bool {
    let event_type = string_at_paths(
        value,
        &[
            &["type"],
            &["event"],
            &["event", "type"],
            &["msg", "type"],
            &["message", "type"],
        ],
    );

    matches!(
        event_type.as_deref(),
        Some("turn.completed" | "turn.completed.usage")
    )
}

fn usage_value(value: &Value) -> Option<&Value> {
    value
        .get("usage")
        .or_else(|| value.pointer("/turn/usage"))
        .or_else(|| value.pointer("/event/usage"))
        .or_else(|| value.pointer("/msg/usage"))
        .or_else(|| value.pointer("/message/usage"))
}

fn usage_token_counts(usage: &Value) -> Option<TokenCounts> {
    let total_tokens = u64_at_paths(
        usage,
        &[&["total_tokens"], &["total"], &["tokens", "total_tokens"]],
    );
    let mut input_tokens = u64_at_paths(
        usage,
        &[
            &["input_tokens"],
            &["prompt_tokens"],
            &["tokens", "input_tokens"],
        ],
    )
    .unwrap_or(0);
    let output_tokens = u64_at_paths(
        usage,
        &[
            &["output_tokens"],
            &["completion_tokens"],
            &["tokens", "output_tokens"],
        ],
    )
    .unwrap_or(0);
    let cache_read_tokens = u64_at_paths(
        usage,
        &[
            &["cache_read_tokens"],
            &["cache_read_input_tokens"],
            &["cached_input_tokens"],
            &["input_tokens_details", "cached_tokens"],
        ],
    )
    .unwrap_or(0);
    let cache_write_tokens = u64_at_paths(
        usage,
        &[
            &["cache_write_tokens"],
            &["cache_creation_input_tokens"],
            &["input_tokens_details", "cache_creation_tokens"],
        ],
    )
    .unwrap_or(0);

    let known_sum = input_tokens
        .saturating_add(output_tokens)
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_write_tokens);

    if known_sum == 0 {
        return total_tokens.filter(|total| *total > 0).map(|total| {
            TokenCounts::new(total, output_tokens, cache_read_tokens, cache_write_tokens)
        });
    }

    if let Some(total_tokens) = total_tokens {
        if known_sum > total_tokens {
            input_tokens = input_tokens.saturating_sub(known_sum.saturating_sub(total_tokens));
        } else if known_sum < total_tokens {
            input_tokens = input_tokens.saturating_add(total_tokens.saturating_sub(known_sum));
        }
    }

    Some(TokenCounts::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    ))
}

fn usage_digest(value: &Value, usage: &Value, raw_line: &str) -> String {
    let stable_id = string_at_paths(
        value,
        &[
            &["turn_id"],
            &["response_id"],
            &["id"],
            &["event_id"],
            &["msg", "id"],
            &["message", "id"],
        ],
    );
    let session_id = string_at_paths(value, &[&["session_id"], &["conversation_id"]]);
    let digest_input = match (session_id, stable_id) {
        (Some(session_id), Some(stable_id)) => {
            format!("codex-exec|{session_id}|{stable_id}|{usage}")
        }
        (Some(session_id), None) => format!("codex-exec|{session_id}|{usage}"),
        (None, Some(stable_id)) => format!("codex-exec|{stable_id}|{usage}"),
        (None, None) => raw_line.to_owned(),
    };

    digest_payload(&digest_input)
}

fn existing_digests(path: &Path) -> io::Result<HashSet<String>> {
    match EventLog::read_file(path) {
        Ok(log) => Ok(log
            .events
            .into_iter()
            .map(|event| event.content_digest)
            .collect()),
        Err(EventLogError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Ok(HashSet::new())
        }
        Err(error) => Err(event_log_error_to_io_error(error)),
    }
}

fn event_log_error_to_io_error(error: EventLogError) -> io::Error {
    match error {
        EventLogError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

fn string_at_paths(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| value_at_path(value, path).and_then(Value::as_str))
        .map(str::to_owned)
}

fn u64_at_paths(value: &Value, paths: &[&[&str]]) -> Option<u64> {
    paths
        .iter()
        .find_map(|path| value_at_path(value, path).and_then(Value::as_u64))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn diagnostic(line: usize, kind: StructuredImportDiagnosticKind) -> StructuredImportDiagnostic {
    StructuredImportDiagnostic { line, kind }
}

fn digest_payload(payload: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;

    for byte in payload.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("fnv1a64:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_codex_exec_turn_completed_usage_as_core_event() {
        let input = concat!(
            "{\"type\":\"turn.completed\",",
            "\"session_id\":\"session-1\",",
            "\"turn_id\":\"turn-1\",",
            "\"timestamp_ms\":1725000123456,",
            "\"usage\":{\"input_tokens\":100,\"output_tokens\":25,",
            "\"input_tokens_details\":{\"cached_tokens\":10},",
            "\"total_tokens\":125}}\n"
        );
        let imported = import_codex_exec_jsonl(input, &StructuredUsageDefaults::default());

        assert!(imported.diagnostics.is_empty());
        assert_eq!(imported.events.len(), 1);
        let event = &imported.events[0];
        assert_eq!(event.adapter, "import.codex.exec");
        assert_eq!(event.tool, "codex.exec");
        assert_eq!(event.operation_class, OperationClass::SessionMeta);
        assert_eq!(event.run_id, "session-1");
        assert_eq!(event.tokens.input_tokens, 90);
        assert_eq!(event.tokens.output_tokens, 25);
        assert_eq!(event.tokens.cache_read_tokens, 10);
        assert_eq!(event.tokens.total().unwrap(), 125);
        assert_eq!(event.direction.as_deref(), Some("summary"));
        event.validate().unwrap();
    }

    #[test]
    fn skips_unsupported_json_events_and_malformed_lines() {
        let input = concat!(
            "not json\n",
            "{\"type\":\"agent_message\",\"message\":\"done\"}\n",
            "{\"type\":\"turn.completed\"}\n"
        );
        let imported = import_codex_exec_jsonl(input, &StructuredUsageDefaults::default());

        assert!(imported.events.is_empty());
        assert_eq!(imported.diagnostics.len(), 3);
        assert_eq!(
            imported.diagnostics[0].kind,
            StructuredImportDiagnosticKind::MalformedJsonlRecord
        );
        assert_eq!(
            imported.diagnostics[1].kind,
            StructuredImportDiagnosticKind::UnsupportedRecord
        );
        assert_eq!(
            imported.diagnostics[2].kind,
            StructuredImportDiagnosticKind::MissingUsage
        );
    }

    #[test]
    fn appending_same_codex_usage_twice_does_not_duplicate_event_log_tokens() {
        let temp_dir = std::env::temp_dir().join(format!(
            "vc-tokmeter-structured-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let event_log = temp_dir.join("events.jsonl");
        let input = concat!(
            "{\"type\":\"turn.completed\",",
            "\"session_id\":\"session-dedupe\",",
            "\"turn_id\":\"turn-dedupe\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"total_tokens\":15}}\n"
        );

        let first = append_codex_exec_jsonl_to_event_log(
            input,
            &event_log,
            &StructuredUsageDefaults::default(),
        )
        .unwrap();
        let second = append_codex_exec_jsonl_to_event_log(
            input,
            &event_log,
            &StructuredUsageDefaults::default(),
        )
        .unwrap();
        let log = EventLog::read_file(&event_log).unwrap();

        assert_eq!(first.imported, 1);
        assert_eq!(first.skipped_duplicates, 0);
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_duplicates, 1);
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].tokens.total().unwrap(), 15);
    }
}
