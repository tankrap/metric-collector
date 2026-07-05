use serde_json::Value;

const VALID_UPLOAD_FIXTURE: &str = include_str!("fixtures/upload-payload-v1.valid.json");
const FORBIDDEN_UPLOAD_FIXTURE: &str = include_str!("fixtures/upload-payload-v1.forbidden.json");

const FORBIDDEN_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "authorization",
    "branch",
    "branch_name",
    "command",
    "content",
    "cookie",
    "event_log",
    "events_jsonl",
    "file_content",
    "file_path",
    "messages",
    "path",
    "prompt",
    "provider_request",
    "provider_response",
    "raw",
    "raw_events",
    "raw_tool_output",
    "repository",
    "repository_name",
    "response",
    "source",
    "source_code",
    "tool_input",
    "tool_output",
    "transcript",
];

const FORBIDDEN_VALUE_MARKERS: &[&str] = &[
    "PROMPT_SHOULD_NOT_UPLOAD",
    "TOOL_OUTPUT_SHOULD_NOT_UPLOAD",
    "source_code_should_not_upload",
    "sk-upload-regression-secret",
    "/Users/example/private",
    "secret-customer-branch",
];

#[test]
fn upload_payload_fixture_excludes_forbidden_private_fields() {
    let fixture: Value = serde_json::from_str(VALID_UPLOAD_FIXTURE).unwrap();
    let failures = scan_upload_payload(&fixture);
    assert!(
        failures.is_empty(),
        "valid upload fixture leaked forbidden data: {failures:?}"
    );
}

#[test]
fn upload_payload_privacy_scanner_catches_forbidden_fixture() {
    let fixture: Value = serde_json::from_str(FORBIDDEN_UPLOAD_FIXTURE).unwrap();
    let failures = scan_upload_payload(&fixture);

    for expected in [
        "forbidden key prompt",
        "forbidden key path",
        "forbidden key branch_name",
        "forbidden key raw_events",
        "forbidden key tool_output",
        "forbidden key source",
        "forbidden key authorization",
        "forbidden value marker PROMPT_SHOULD_NOT_UPLOAD",
        "forbidden value marker TOOL_OUTPUT_SHOULD_NOT_UPLOAD",
        "forbidden value marker source_code_should_not_upload",
        "forbidden value marker sk-upload-regression-secret",
        "forbidden value marker /Users/example/private",
        "forbidden value marker secret-customer-branch",
    ] {
        assert!(
            failures.iter().any(|failure| failure.contains(expected)),
            "expected scanner failure containing {expected:?}, got {failures:?}"
        );
    }
}

fn scan_upload_payload(value: &Value) -> Vec<String> {
    let mut failures = Vec::new();
    scan_value(value, "$", &mut failures);
    failures
}

fn scan_value(value: &Value, path: &str, failures: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let normalized = normalize_key(key);
                if FORBIDDEN_KEYS.contains(&normalized.as_str())
                    && !is_allowed_aggregate_key(path, &normalized)
                {
                    failures.push(format!("forbidden key {normalized} at {path}.{key}"));
                }
                scan_value(value, &format!("{path}.{key}"), failures);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                scan_value(value, &format!("{path}[{index}]"), failures);
            }
        }
        Value::String(text) => {
            for marker in FORBIDDEN_VALUE_MARKERS {
                if text.contains(marker) {
                    failures.push(format!("forbidden value marker {marker} at {path}"));
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn is_allowed_aggregate_key(path: &str, key: &str) -> bool {
    key == "source" && path.starts_with("$.metrics.token_sources[")
}

fn normalize_key(key: &str) -> String {
    key.trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}
