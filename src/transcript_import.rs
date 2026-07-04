use crate::core::{AttributedTokenEvent, OperationClass, TokenCounts, ValidationError};

const DEFAULT_TIMESTAMP_MS: u64 = 1;
const DEFAULT_RUN_ID: &str = "imported-run";
const DEFAULT_TASK_ID: &str = "imported-task";
const DEFAULT_PROFILE_ID: &str = "imported-profile";
const DEFAULT_ADAPTER: &str = "transcript-import";
const UNKNOWN_TOOL: &str = "unknown-tool";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTranscript {
    pub events: Vec<ImportedEvent>,
    pub diagnostics: Vec<ImportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedEvent {
    pub timestamp_ms: u64,
    pub run_id: String,
    pub task_id: String,
    pub profile_id: String,
    pub adapter: String,
    pub tool: String,
    pub op_class: String,
    pub tokens: ImportedTokenCounts,
    pub byte_count: u64,
    pub digest: String,
    pub estimated_tokens: bool,
    pub fidelity: ImportFidelity,
}

impl ImportedEvent {
    pub fn to_core_event(&self) -> AttributedTokenEvent {
        AttributedTokenEvent {
            timestamp_ms: self.timestamp_ms,
            run_id: self.run_id.clone(),
            task_id: self.task_id.clone(),
            profile_id: self.profile_id.clone(),
            adapter: self.adapter.clone(),
            operation_class: OperationClass::from_str_or_other(&self.op_class),
            tool: self.tool.clone(),
            tokens: self.tokens.to_core_counts(),
            byte_count: self.byte_count,
            content_digest: self.digest.clone(),
            repeat_of: None,
        }
    }

    pub fn validate_core_compatible(&self) -> Result<(), ValidationError> {
        self.to_core_event().validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTokenCounts {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl ImportedTokenCounts {
    pub const fn to_core_counts(&self) -> TokenCounts {
        TokenCounts::new(
            self.input_tokens,
            self.output_tokens,
            self.cache_read_tokens,
            self.cache_write_tokens,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFidelity {
    pub label: String,
    pub missing: MissingImportMetadata,
}

impl ImportFidelity {
    pub fn missing_labels(&self) -> Vec<&'static str> {
        let mut labels = Vec::new();

        if self.missing.usage {
            labels.push("missing_usage");
        }
        if self.missing.cache {
            labels.push("missing_cache");
        }
        if self.missing.model {
            labels.push("missing_model");
        }
        if self.missing.settings {
            labels.push("missing_settings");
        }
        if self.missing.timestamp {
            labels.push("missing_timestamp");
        }
        if self.missing.run_id {
            labels.push("missing_run_id");
        }
        if self.missing.task_id {
            labels.push("missing_task_id");
        }
        if self.missing.profile_id {
            labels.push("missing_profile_id");
        }

        labels
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MissingImportMetadata {
    pub usage: bool,
    pub cache: bool,
    pub model: bool,
    pub settings: bool,
    pub timestamp: bool,
    pub run_id: bool,
    pub task_id: bool,
    pub profile_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDiagnostic {
    pub line: usize,
    pub kind: ImportDiagnosticKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDiagnosticKind {
    MalformedJsonlRecord,
    UnsupportedRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDefaults {
    pub timestamp_ms: u64,
    pub run_id: String,
    pub task_id: String,
    pub profile_id: String,
    pub adapter: String,
}

impl Default for ImportDefaults {
    fn default() -> Self {
        Self {
            timestamp_ms: DEFAULT_TIMESTAMP_MS,
            run_id: DEFAULT_RUN_ID.to_owned(),
            task_id: DEFAULT_TASK_ID.to_owned(),
            profile_id: DEFAULT_PROFILE_ID.to_owned(),
            adapter: DEFAULT_ADAPTER.to_owned(),
        }
    }
}

pub fn import_transcript(input: &str) -> ImportedTranscript {
    import_transcript_with_defaults(input, &ImportDefaults::default())
}

pub fn import_transcript_with_defaults(
    input: &str,
    defaults: &ImportDefaults,
) -> ImportedTranscript {
    let mut imported = ImportedTranscript {
        events: Vec::new(),
        diagnostics: Vec::new(),
    };

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if !looks_like_json_object(trimmed) {
            imported.diagnostics.push(ImportDiagnostic {
                line: line_number,
                kind: ImportDiagnosticKind::MalformedJsonlRecord,
            });
            continue;
        }

        match import_line(trimmed, defaults) {
            Some(event) => imported.events.push(event),
            None => imported.diagnostics.push(ImportDiagnostic {
                line: line_number,
                kind: ImportDiagnosticKind::UnsupportedRecord,
            }),
        }
    }

    imported
}

fn import_line(line: &str, defaults: &ImportDefaults) -> Option<ImportedEvent> {
    if !is_importable_event(line) {
        return None;
    }

    let timestamp = extract_u64_any(line, &["timestamp_ms", "time_ms"]);
    let run_id = extract_string_any(line, &["run_id", "run"]);
    let task_id = extract_string_any(line, &["task_id", "task"]);
    let profile_id = extract_string_any(line, &["profile_id", "profile"]);
    let adapter = extract_string_any(line, &["adapter", "source"]);
    let raw_tool = extract_string_any(line, &["tool", "tool_name", "name", "recipient_name"]);
    let command = extract_string_any(line, &["command", "cmd", "input", "arguments", "path"]);
    let tool = normalize_tool(raw_tool.as_deref());
    let payload = command.as_deref().unwrap_or(line);
    let byte_count = extract_u64_any(line, &["byte_count", "bytes"])
        .unwrap_or_else(|| payload.len().try_into().unwrap_or(u64::MAX));
    let missing = MissingImportMetadata {
        usage: !has_any_key(
            line,
            &[
                "usage",
                "token_usage",
                "input_tokens",
                "prompt_tokens",
                "output_tokens",
                "completion_tokens",
            ],
        ),
        cache: !has_any_key(
            line,
            &[
                "cache_read_tokens",
                "cache_write_tokens",
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
            ],
        ),
        model: !has_key(line, "model"),
        settings: !has_any_key(line, &["settings", "temperature", "reasoning_effort"]),
        timestamp: timestamp.is_none(),
        run_id: run_id.is_none(),
        task_id: task_id.is_none(),
        profile_id: profile_id.is_none(),
    };
    let estimated_tokens = missing.usage;
    let tokens = imported_token_counts(line, byte_count, estimated_tokens);

    Some(ImportedEvent {
        timestamp_ms: timestamp.unwrap_or(defaults.timestamp_ms),
        run_id: run_id.unwrap_or_else(|| defaults.run_id.clone()),
        task_id: task_id.unwrap_or_else(|| defaults.task_id.clone()),
        profile_id: profile_id.unwrap_or_else(|| defaults.profile_id.clone()),
        adapter: adapter.unwrap_or_else(|| defaults.adapter.clone()),
        tool: tool.to_owned(),
        op_class: classify_imported_event(tool, command.as_deref()).to_owned(),
        tokens,
        byte_count,
        digest: digest_payload(payload),
        estimated_tokens,
        fidelity: ImportFidelity {
            label: fidelity_label(estimated_tokens, &missing).to_owned(),
            missing,
        },
    })
}

fn imported_token_counts(
    line: &str,
    byte_count: u64,
    estimated_tokens: bool,
) -> ImportedTokenCounts {
    if estimated_tokens {
        return ImportedTokenCounts {
            input_tokens: estimate_tokens(byte_count),
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
    }

    ImportedTokenCounts {
        input_tokens: extract_u64_any(line, &["input_tokens", "prompt_tokens"]).unwrap_or(0),
        output_tokens: extract_u64_any(line, &["output_tokens", "completion_tokens"]).unwrap_or(0),
        cache_read_tokens: extract_u64_any(line, &["cache_read_tokens", "cache_read_input_tokens"])
            .unwrap_or(0),
        cache_write_tokens: extract_u64_any(
            line,
            &["cache_write_tokens", "cache_creation_input_tokens"],
        )
        .unwrap_or(0),
    }
}

fn fidelity_label(estimated_tokens: bool, missing: &MissingImportMetadata) -> &'static str {
    if estimated_tokens {
        "estimated_missing_usage"
    } else if missing.cache {
        "exact_missing_cache"
    } else if missing.model || missing.settings {
        "exact_missing_metadata"
    } else {
        "full"
    }
}

fn estimate_tokens(byte_count: u64) -> u64 {
    byte_count
        .saturating_add(3)
        .checked_div(4)
        .unwrap_or(0)
        .max(1)
}

fn classify_imported_event(tool: &str, command: Option<&str>) -> &'static str {
    let lower_tool = tool.to_ascii_lowercase();

    match lower_tool.as_str() {
        "read" | "notebookread" => return "file.read",
        "grep" | "glob" | "search" => return "file.search",
        "ls" | "list" => return "file.list",
        "edit" | "multiedit" | "write" | "notebookedit" | "apply_patch" => return "edit.echo",
        _ => {}
    }

    if matches!(lower_tool.as_str(), "bash" | "shell" | "exec_command") {
        if let Some(command) = command {
            return classify_command(command);
        }
    }

    command.map(classify_command).unwrap_or("other")
}

fn classify_command(command: &str) -> &'static str {
    for segment in command_segments(command) {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        if tokens.is_empty() {
            continue;
        }

        if tokens.first().is_some_and(|token| *token == "git") {
            match git_subcommand(&tokens[1..]).as_deref() {
                Some("status") => return "vc.status",
                Some("diff") => return "vc.diff",
                Some("log") => return "vc.log",
                Some("show") => return "vc.show",
                Some("branch" | "checkout" | "switch") => return "vc.branch_ops",
                Some("push" | "pull" | "fetch") => return "vc.push_pull",
                _ => {}
            }
        }

        match tokens.as_slice() {
            ["cargo", "test", ..]
            | ["go", "test", ..]
            | ["pytest", ..]
            | ["python", "-m", "pytest", ..]
            | ["python3", "-m", "pytest", ..]
            | ["npm", "test", ..]
            | ["pnpm", "test", ..]
            | ["yarn", "test", ..] => return "test.output",
            ["cargo", "build", ..]
            | ["cargo", "check", ..]
            | ["cargo", "clippy", ..]
            | ["go", "build", ..]
            | ["npm", "build", ..]
            | ["pnpm", "build", ..]
            | ["yarn", "build", ..]
            | ["tsc", ..]
            | ["rustc", ..] => return "build.output",
            ["npm", "run", name, ..] | ["pnpm", "run", name, ..] | ["yarn", "run", name, ..] => {
                if name.starts_with("test") {
                    return "test.output";
                }
                if *name == "build" || name.starts_with("build:") {
                    return "build.output";
                }
            }
            [first, ..] if is_file_read_command(first) => return "file.read",
            [first, ..] if is_file_search_command(first) => return "file.search",
            [first, ..] if is_file_list_command(first) => return "file.list",
            ["apply_patch", ..] | ["tee", ..] => return "edit.echo",
            ["echo" | "printf" | "cat", ..] if segment.contains('>') || segment.contains("<<") => {
                return "edit.echo";
            }
            _ => {}
        }
    }

    "other"
}

fn is_file_read_command(command: &str) -> bool {
    matches!(
        command,
        "cat" | "sed" | "head" | "tail" | "nl" | "less" | "more" | "bat" | "awk" | "wc"
    )
}

fn is_file_search_command(command: &str) -> bool {
    matches!(command, "rg" | "grep" | "ag" | "ack" | "find" | "fd")
}

fn is_file_list_command(command: &str) -> bool {
    matches!(command, "ls" | "tree" | "du")
}

fn command_segments(command: &str) -> impl Iterator<Item = &str> {
    command
        .split(['\n', ';', '|'])
        .flat_map(|part| part.split("&&"))
        .flat_map(|part| part.split("||"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

fn shell_tokens(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for ch in segment.chars() {
        if escaped {
            current.push(ch.to_ascii_lowercase());
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch.to_ascii_lowercase()),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn command_tokens(tokens: &[String]) -> Vec<&str> {
    let mut index = 0;

    while tokens
        .get(index)
        .is_some_and(|token| is_env_assignment(token))
    {
        index += 1;
    }

    while let Some(token) = tokens.get(index) {
        match token.as_str() {
            "sudo" | "command" | "time" => index += 1,
            "env" => {
                index += 1;
                while tokens
                    .get(index)
                    .is_some_and(|token| is_env_assignment(token))
                {
                    index += 1;
                }
            }
            _ => break,
        }
    }

    tokens[index..].iter().map(String::as_str).collect()
}

fn git_subcommand(tokens: &[&str]) -> Option<String> {
    let mut index = 0;

    while let Some(token) = tokens.get(index) {
        match *token {
            "-c" | "-C" | "--git-dir" | "--work-tree" | "--namespace" => index += 2,
            "--no-pager" | "--bare" => index += 1,
            token if token.starts_with('-') => index += 1,
            token => return Some(token.to_owned()),
        }
    }

    None
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };

    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn normalize_tool(tool: Option<&str>) -> &str {
    let Some(tool) = tool else {
        return UNKNOWN_TOOL;
    };

    tool.rsplit('.').next().unwrap_or(tool).trim()
}

fn is_importable_event(line: &str) -> bool {
    has_any_key(
        line,
        &[
            "tool",
            "tool_name",
            "recipient_name",
            "command",
            "cmd",
            "path",
        ],
    ) || line.contains("\"tool_call\"")
        || line.contains("\"tool_calls\"")
        || line.contains("\"toolUse\"")
        || line.contains("\"tool_use\"")
        || line.contains("\"function_call\"")
}

fn extract_string_any(line: &str, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| extract_json_string(line, key))
}

fn extract_u64_any(line: &str, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| extract_json_u64(line, key))
}

fn has_any_key(line: &str, keys: &[&str]) -> bool {
    keys.iter().any(|key| has_key(line, key))
}

fn has_key(line: &str, key: &str) -> bool {
    value_start(line, key).is_some()
}

fn extract_json_string(line: &str, key: &str) -> Option<String> {
    let start = skip_ascii_whitespace(line, value_start(line, key)?);
    parse_json_string(line, start)
}

fn extract_json_u64(line: &str, key: &str) -> Option<u64> {
    let start = skip_ascii_whitespace(line, value_start(line, key)?);
    let bytes = line.as_bytes();
    let mut end = start;

    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }

    (end > start)
        .then(|| line[start..end].parse().ok())
        .flatten()
}

fn value_start(line: &str, key: &str) -> Option<usize> {
    let marker = format!("\"{key}\"");
    let mut search_from = 0;

    while let Some(offset) = line[search_from..].find(&marker) {
        let after_marker = search_from + offset + marker.len();
        let colon = skip_ascii_whitespace(line, after_marker);
        if line.as_bytes().get(colon) == Some(&b':') {
            return Some(colon + 1);
        }
        search_from = after_marker;
    }

    None
}

fn skip_ascii_whitespace(line: &str, mut index: usize) -> usize {
    let bytes = line.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn parse_json_string(line: &str, start: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return None;
    }

    let mut value = String::new();
    let mut index = start + 1;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Some(value),
            b'\\' => {
                index += 1;
                match bytes.get(index).copied()? {
                    b'"' => value.push('"'),
                    b'\\' => value.push('\\'),
                    b'/' => value.push('/'),
                    b'b' => value.push('\u{0008}'),
                    b'f' => value.push('\u{000C}'),
                    b'n' => value.push('\n'),
                    b'r' => value.push('\r'),
                    b't' => value.push('\t'),
                    b'u' => {
                        value.push('?');
                        index = index.saturating_add(4);
                    }
                    other => value.push(other as char),
                }
            }
            other => value.push(other as char),
        }
        index += 1;
    }

    None
}

fn digest_payload(payload: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;

    for byte in payload.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("fnv1a64:{hash:016x}")
}

fn looks_like_json_object(line: &str) -> bool {
    line.starts_with('{') && line.ends_with('}') && braces_are_balanced(line)
}

fn braces_are_balanced(line: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for byte in line.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    depth == 0 && !in_string
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_valid_transcript_event_to_core_compatible_fields() {
        let input = concat!(
            "{\"timestamp_ms\":1725000123456,",
            "\"run_id\":\"run-1\",",
            "\"task_id\":\"task-1\",",
            "\"profile_id\":\"default\",",
            "\"adapter\":\"codex-cli\",",
            "\"type\":\"tool_call\",",
            "\"tool\":\"Bash\",",
            "\"command\":\"git diff -- src/lib.rs\",",
            "\"usage\":{\"input_tokens\":40,\"output_tokens\":8,",
            "\"cache_read_tokens\":3,\"cache_write_tokens\":1},",
            "\"model\":\"gpt-5\",",
            "\"settings\":{\"reasoning_effort\":\"low\"}}\n",
        );

        let imported = import_transcript(input);

        assert!(imported.diagnostics.is_empty());
        assert_eq!(imported.events.len(), 1);

        let event = &imported.events[0];
        assert_eq!(event.timestamp_ms, 1_725_000_123_456);
        assert_eq!(event.run_id, "run-1");
        assert_eq!(event.task_id, "task-1");
        assert_eq!(event.profile_id, "default");
        assert_eq!(event.adapter, "codex-cli");
        assert_eq!(event.tool, "Bash");
        assert_eq!(event.op_class, "vc.diff");
        assert_eq!(event.byte_count, "git diff -- src/lib.rs".len() as u64);
        assert_eq!(event.tokens.input_tokens, 40);
        assert_eq!(event.tokens.output_tokens, 8);
        assert_eq!(event.tokens.cache_read_tokens, 3);
        assert_eq!(event.tokens.cache_write_tokens, 1);
        assert!(!event.estimated_tokens);
        assert_eq!(event.fidelity.label, "full");
        assert!(event.fidelity.missing_labels().is_empty());
        event.validate_core_compatible().unwrap();
    }

    #[test]
    fn labels_missing_usage_cache_and_model_metadata() {
        let input = concat!(
            "{\"timestamp_ms\":1725000123456,",
            "\"run_id\":\"run-2\",",
            "\"task_id\":\"task-2\",",
            "\"profile_id\":\"default\",",
            "\"type\":\"tool_call\",",
            "\"tool\":\"Bash\",",
            "\"command\":\"rg TODO src\"}\n",
        );

        let imported = import_transcript(input);
        let event = &imported.events[0];

        assert_eq!(event.op_class, "file.search");
        assert!(event.estimated_tokens);
        assert_eq!(event.fidelity.label, "estimated_missing_usage");
        assert_eq!(
            event.fidelity.missing_labels(),
            vec![
                "missing_usage",
                "missing_cache",
                "missing_model",
                "missing_settings"
            ]
        );
        assert!(event.tokens.input_tokens > 0);
        assert_eq!(event.tokens.output_tokens, 0);
        assert_eq!(event.tokens.cache_read_tokens, 0);
        assert_eq!(event.tokens.cache_write_tokens, 0);
        event.validate_core_compatible().unwrap();
    }

    #[test]
    fn does_not_persist_raw_transcript_content() {
        let input = concat!(
            "{\"timestamp_ms\":1725000123456,",
            "\"run_id\":\"run-3\",",
            "\"task_id\":\"task-3\",",
            "\"profile_id\":\"default\",",
            "\"type\":\"tool_call\",",
            "\"tool\":\"Bash\",",
            "\"command\":\"printf SECRET_RAW_TRANSCRIPT > /tmp/out\"}\n",
        );

        let imported = import_transcript(input);
        let debug = format!("{imported:?}");
        let event = &imported.events[0];

        assert_eq!(event.tool, "Bash");
        assert_eq!(event.op_class, "edit.echo");
        assert_eq!(
            event.byte_count,
            "printf SECRET_RAW_TRANSCRIPT > /tmp/out".len() as u64
        );
        assert!(event.digest.starts_with("fnv1a64:"));
        assert!(!event.digest.contains("SECRET_RAW_TRANSCRIPT"));
        assert!(!debug.contains("SECRET_RAW_TRANSCRIPT"));
    }
}
