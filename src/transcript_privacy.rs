use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::core::{EventLog, EventLogError};
use crate::share::{ShareMap, ShareSanitizer, ShareValue};
use crate::transcript_import::{
    ImportDiagnosticKind, ImportedEvent, ImportedTranscript, import_transcript,
};

pub const RAW_PROMPT_MARKER: &str = "RAW_PROMPT_DO_NOT_SHARE_4f3a1d";
pub const RAW_SOURCE_MARKER: &str = "RAW_SOURCE_DO_NOT_SHARE_8b91c2";
pub const RAW_PATH_MARKER: &str = "/Users/example/private/metric-taker/secrets.rs";
pub const RAW_SECRET_MARKER: &str = "sk-metric-taker-regression-secret";

pub const FORBIDDEN_RAW_MARKERS: [&str; 4] = [
    RAW_PROMPT_MARKER,
    RAW_SOURCE_MARKER,
    RAW_PATH_MARKER,
    RAW_SECRET_MARKER,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyLeak {
    pub label: String,
    pub marker: String,
}

impl PrivacyLeak {
    pub fn new(label: impl Into<String>, marker: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            marker: marker.into(),
        }
    }
}

impl fmt::Display for PrivacyLeak {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} leaked forbidden raw marker {:?}",
            self.label, self.marker
        )
    }
}

impl Error for PrivacyLeak {}

pub fn scan_forbidden_raw_markers(
    label: &str,
    text: &str,
    markers: &[&str],
) -> Result<(), PrivacyLeak> {
    for marker in markers {
        if !marker.is_empty() && text.contains(marker) {
            return Err(PrivacyLeak::new(label, *marker));
        }
    }

    Ok(())
}

pub fn privacy_regression_transcript_fixture() -> String {
    [
        format!(
            "{{\"type\":\"tool_call\",\"timestamp_ms\":1725000123456,\
             \"run_id\":\"privacy-run\",\"task_id\":\"privacy-task\",\
             \"profile_id\":\"default\",\"adapter\":\"claude-code\",\
             \"tool\":\"Bash\",\"command\":\"cat {RAW_PATH_MARKER} && \
             printf '{RAW_PROMPT_MARKER}' && echo {RAW_SECRET_MARKER}\",\
             \"usage\":{{\"input_tokens\":31,\"output_tokens\":7,\
             \"cache_read_tokens\":5,\"cache_write_tokens\":2}},\
             \"model\":\"claude-regression\",\
             \"settings\":{{\"temperature\":0}}}}"
        ),
        format!(
            "{{\"type\":\"tool_use\",\"timestamp_ms\":1725000123555,\
             \"run_id\":\"privacy-run\",\"task_id\":\"privacy-task\",\
             \"profile_id\":\"default\",\"adapter\":\"claude-code\",\
             \"tool\":\"Read\",\"path\":\"{RAW_PATH_MARKER}\",\
             \"content\":\"fn source_marker() {{ /* {RAW_SOURCE_MARKER} */ }}\",\
             \"prompt\":\"{RAW_PROMPT_MARKER}\"}}"
        ),
        format!(
            "{{\"type\":\"unsupported\",\"prompt\":\"{RAW_PROMPT_MARKER}\",\
             \"source\":\"{RAW_SOURCE_MARKER}\",\"raw_path\":\"{RAW_PATH_MARKER}\",\
             \"secret\":\"{RAW_SECRET_MARKER}\"}}"
        ),
        format!(
            "{{\"type\":\"tool_call\",\"tool\":\"Bash\",\
             \"command\":\"echo {RAW_SECRET_MARKER}\""
        ),
    ]
    .join("\n")
}

pub fn import_privacy_fixture() -> ImportedTranscript {
    import_transcript(&privacy_regression_transcript_fixture())
}

pub fn serialize_imported_core_events(
    imported: &ImportedTranscript,
) -> Result<String, EventLogError> {
    let mut log = EventLog::new();
    for event in &imported.events {
        log.push(event.to_core_event())
            .map_err(EventLogError::Validation)?;
    }

    let mut bytes = Vec::new();
    log.write_to(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn serialize_import_diagnostics(imported: &ImportedTranscript) -> String {
    let mut out = String::new();

    for diagnostic in &imported.diagnostics {
        let kind = match diagnostic.kind {
            ImportDiagnosticKind::MalformedJsonlRecord => "malformed_jsonl_record",
            ImportDiagnosticKind::UnsupportedRecord => "unsupported_record",
        };
        out.push_str("line=");
        out.push_str(&diagnostic.line.to_string());
        out.push_str("\tkind=");
        out.push_str(kind);
        out.push('\n');
    }

    out
}

pub fn share_fixture_from_events(events: &[ImportedEvent]) -> ShareValue {
    let event_values = events
        .iter()
        .map(|event| {
            map([
                ("run_id", ShareValue::String(event.run_id.clone())),
                ("task_id", ShareValue::String(event.task_id.clone())),
                ("profile_id", ShareValue::String(event.profile_id.clone())),
                ("adapter", ShareValue::String(event.adapter.clone())),
                ("tool", ShareValue::String(event.tool.clone())),
                ("op_class", ShareValue::String(event.op_class.clone())),
                ("content_digest", ShareValue::String(event.digest.clone())),
                ("byte_count", ShareValue::U64(event.byte_count)),
            ])
        })
        .collect();

    let mut report = BTreeMap::new();
    report.insert("events".to_owned(), ShareValue::List(event_values));
    report.insert(
        "prompt".to_owned(),
        ShareValue::String(RAW_PROMPT_MARKER.to_owned()),
    );
    report.insert(
        "source".to_owned(),
        ShareValue::String(RAW_SOURCE_MARKER.to_owned()),
    );
    report.insert(
        "path".to_owned(),
        ShareValue::String(RAW_PATH_MARKER.to_owned()),
    );
    report.insert(
        "secret".to_owned(),
        ShareValue::String(RAW_SECRET_MARKER.to_owned()),
    );
    report.insert(
        RAW_PATH_MARKER.to_owned(),
        ShareValue::String(RAW_SOURCE_MARKER.to_owned()),
    );

    ShareValue::Map(report)
}

pub fn serialize_sanitized_share_fixture(imported: &ImportedTranscript, salt: &str) -> String {
    let value = share_fixture_from_events(&imported.events);
    let sanitized = ShareSanitizer::new(salt).sanitize(&value);
    serialize_share_value(&sanitized)
}

pub fn serialize_share_value(value: &ShareValue) -> String {
    let mut out = String::new();
    write_share_value(&mut out, value);
    out
}

fn write_share_value(out: &mut String, value: &ShareValue) {
    match value {
        ShareValue::Map(map) => write_share_map(out, map),
        ShareValue::List(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_share_value(out, value);
            }
            out.push(']');
        }
        ShareValue::String(value) => write_json_string(out, value),
        ShareValue::U64(value) => out.push_str(&value.to_string()),
        ShareValue::I64(value) => out.push_str(&value.to_string()),
        ShareValue::F64(value) if value.is_finite() => out.push_str(&value.to_string()),
        ShareValue::F64(_) => out.push_str("null"),
        ShareValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        ShareValue::Null => out.push_str("null"),
    }
}

fn write_share_map(out: &mut String, map: &ShareMap) {
    out.push('{');
    for (index, (key, value)) in map.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_json_string(out, key);
        out.push(':');
        write_share_value(out, value);
    }
    out.push('}');
}

fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                out.push_str("\\u");
                out.push_str(&format!("{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn map(entries: impl IntoIterator<Item = (&'static str, ShareValue)>) -> ShareValue {
    ShareValue::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_forbidden(label: &str, text: &str) {
        if let Err(error) = scan_forbidden_raw_markers(label, text, &FORBIDDEN_RAW_MARKERS) {
            panic!("{error}: {text}");
        }
    }

    #[test]
    fn imported_core_event_serialization_excludes_raw_transcript_markers() {
        let imported = import_privacy_fixture();
        assert_eq!(imported.events.len(), 2);

        let serialized =
            serialize_imported_core_events(&imported).expect("core event serialization succeeds");

        assert_no_forbidden("core events", &serialized);
        assert!(serialized.contains("digest=fnv1a64:"));
        assert!(serialized.contains("op_class=file.read"));
        assert!(serialized.contains("tool=Bash"));
        assert!(serialized.contains("tool=Read"));
    }

    #[test]
    fn sanitized_share_output_excludes_prompt_source_path_and_secret_markers() {
        let imported = import_privacy_fixture();
        let serialized = serialize_sanitized_share_fixture(&imported, "privacy-regression");

        assert_no_forbidden("sanitized share output", &serialized);
        assert!(serialized.contains("prompt_byte_count"));
        assert!(serialized.contains("source_byte_count"));
        assert!(serialized.contains("path_hash"));
        assert!(serialized.contains("secret_hash"));
        assert!(serialized.contains("field_hash_"));
    }

    #[test]
    fn malformed_and_unsupported_transcripts_do_not_echo_raw_lines() {
        let imported = import_privacy_fixture();
        assert_eq!(imported.diagnostics.len(), 2);
        assert_eq!(
            imported.diagnostics[0].kind,
            ImportDiagnosticKind::UnsupportedRecord
        );
        assert_eq!(
            imported.diagnostics[1].kind,
            ImportDiagnosticKind::MalformedJsonlRecord
        );

        let serialized = serialize_import_diagnostics(&imported);
        assert_no_forbidden("import diagnostics", &serialized);
        assert!(serialized.contains("kind=unsupported_record"));
        assert!(serialized.contains("kind=malformed_jsonl_record"));
    }

    #[test]
    fn scanner_reports_the_first_forbidden_marker_with_context() {
        let error = scan_forbidden_raw_markers(
            "regression output",
            &format!("leaked {RAW_SECRET_MARKER}"),
            &FORBIDDEN_RAW_MARKERS,
        )
        .expect_err("marker should be rejected");

        assert_eq!(error.label, "regression output");
        assert_eq!(error.marker, RAW_SECRET_MARKER);
        assert!(error.to_string().contains("regression output"));
    }
}
