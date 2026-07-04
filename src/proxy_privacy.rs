use std::fmt;

pub const REDACTED: &str = "[REDACTED]";

pub const SENSITIVE_HEADERS: &[&str] = &["authorization", "api-key", "x-api-key", "cookie"];

pub const DEFAULT_FORBIDDEN_LOG_MARKERS: &[&str] = &[
    "TOKMETER_FIXTURE_SECRET",
    "TOKMETER_FIXTURE_PROMPT",
    "TOKMETER_FIXTURE_CONTENT",
    "SECRET_RAW_TOOL_OUTPUT",
    "PROMPT_SHOULD_NOT_PERSIST",
    "CONTENT_SHOULD_NOT_PERSIST",
    "CREDENTIAL_SHOULD_NOT_PERSIST",
    "PRIVATE_PROMPT_FIXTURE",
    "PRIVATE_CONTENT_FIXTURE",
    "PRIVATE_CREDENTIAL_FIXTURE",
];

const CREDENTIAL_KEYS: &[&str] = &[
    "authorization",
    "api-key",
    "api_key",
    "x-api-key",
    "x_api_key",
    "cookie",
    "openai_api_key",
    "anthropic_api_key",
    "linear_api_key",
    "provider_api_key",
    "access_token",
    "bearer_token",
];

const CONTENT_KEYS: &[&str] = &[
    "prompt",
    "prompts",
    "content",
    "messages",
    "input",
    "instructions",
    "completion",
    "response",
    "tool_output",
    "request_body",
    "response_body",
];

const TOKEN_SCHEMES: &[&str] = &["bearer", "basic"];

const SECRET_PREFIXES: &[&str] = &[
    "sk-",
    "sk-ant-",
    "sk-proj-",
    "sk-svcacct-",
    "ghp_",
    "github_pat_",
    "glpat-",
    "xoxb-",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyRegressionFailure {
    pub marker: String,
    pub byte_offset: usize,
    pub excerpt: String,
}

impl fmt::Display for PrivacyRegressionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "privacy regression marker {:?} at byte {} near {:?}",
            self.marker, self.byte_offset, self.excerpt
        )
    }
}

pub fn is_sensitive_header(header_name: &str) -> bool {
    SENSITIVE_HEADERS
        .iter()
        .any(|sensitive| header_name.trim().eq_ignore_ascii_case(sensitive))
}

pub fn redact_header_value(header_name: &str, header_value: &str) -> String {
    if is_sensitive_header(header_name) {
        REDACTED.to_string()
    } else {
        header_value.to_string()
    }
}

pub fn redact_sensitive_headers<K, V>(headers: &[(K, V)]) -> Vec<(String, String)>
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    headers
        .iter()
        .map(|(name, value)| {
            let name = name.as_ref();
            (name.to_string(), redact_header_value(name, value.as_ref()))
        })
        .collect()
}

pub fn redact_sensitive_header_line(line: &str) -> String {
    let Some(colon) = line.find(':') else {
        return line.to_string();
    };

    if is_sensitive_header(&line[..colon]) {
        format!("{}: {REDACTED}", line[..colon].trim_end())
    } else {
        line.to_string()
    }
}

pub fn redact_provider_credentials(message: &str) -> String {
    let redacted = redact_keyed_values(message, CREDENTIAL_KEYS, false);
    let redacted = redact_token_schemes(&redacted);
    redact_secret_prefixes(&redacted)
}

pub fn redact_provider_error(message: &str) -> String {
    let redacted = redact_provider_credentials(message);
    redact_keyed_values(&redacted, CONTENT_KEYS, true)
}

pub fn redact_proxy_error(message: &str) -> String {
    redact_provider_error(message)
}

pub fn redact_error_string(message: &str) -> String {
    redact_provider_error(message)
}

pub fn scan_persisted_log_string(log: &str) -> Vec<PrivacyRegressionFailure> {
    let mut failures = scan_persisted_log_string_for_markers(log, DEFAULT_FORBIDDEN_LOG_MARKERS);
    failures.extend(scan_unredacted_keyed_values(
        log,
        CREDENTIAL_KEYS,
        "credential",
    ));
    failures.extend(scan_unredacted_keyed_values(log, CONTENT_KEYS, "content"));
    failures.extend(scan_unredacted_token_schemes(log));
    failures.extend(scan_unredacted_secret_prefixes(log));
    failures.sort_by_key(|failure| failure.byte_offset);
    failures
}

pub fn scan_persisted_log_string_for_markers(
    log: &str,
    forbidden_markers: &[&str],
) -> Vec<PrivacyRegressionFailure> {
    let mut failures = Vec::new();

    for marker in forbidden_markers {
        if marker.is_empty() {
            continue;
        }

        let mut search_start = 0;
        while let Some(relative_offset) = log[search_start..].find(marker) {
            let byte_offset = search_start + relative_offset;
            failures.push(PrivacyRegressionFailure {
                marker: (*marker).to_string(),
                byte_offset,
                excerpt: excerpt_around(log, byte_offset, marker.len()),
            });
            search_start = byte_offset + marker.len();
        }
    }

    failures.sort_by_key(|failure| failure.byte_offset);
    failures
}

pub fn privacy_regression_failures(logs: &[&str]) -> Vec<PrivacyRegressionFailure> {
    let mut failures = Vec::new();

    for log in logs {
        failures.extend(scan_persisted_log_string(log));
    }

    failures
}

fn redact_keyed_values(message: &str, keys: &[&str], require_sensitive_value: bool) -> String {
    let mut redacted = message.to_string();

    for key in keys {
        redacted = redact_keyed_value(&redacted, key, require_sensitive_value);
    }

    redacted
}

fn redact_keyed_value(message: &str, key: &str, require_sensitive_value: bool) -> String {
    let mut output = String::with_capacity(message.len());
    let mut cursor = 0;

    while let Some(key_start) = find_keyed_value(message, key, cursor) {
        let Some(value) = parse_value_span(message, key_start, key) else {
            output.push_str(&message[cursor..key_start + key.len()]);
            cursor = key_start + key.len();
            continue;
        };

        if require_sensitive_value && value_is_redacted(message, value.value_start, value.value_end)
        {
            output.push_str(&message[cursor..value.after_value]);
            cursor = value.after_value;
            continue;
        }

        output.push_str(&message[cursor..value.value_start]);
        output.push_str(REDACTED);
        output.push_str(&message[value.value_end..value.after_value]);
        cursor = value.after_value;
    }

    output.push_str(&message[cursor..]);
    output
}

fn redact_token_schemes(message: &str) -> String {
    let mut output = String::with_capacity(message.len());
    let mut cursor = 0;

    while let Some((scheme_start, scheme)) = find_next_token_scheme(message, cursor) {
        let after_scheme = scheme_start + scheme.len();
        let token_start = skip_ascii_whitespace(message, after_scheme);

        if token_start == after_scheme || token_start >= message.len() {
            output.push_str(&message[cursor..after_scheme]);
            cursor = after_scheme;
            continue;
        }

        let token_end = scan_bare_value_end(message, token_start);
        if token_end == token_start || value_is_redacted(message, token_start, token_end) {
            output.push_str(&message[cursor..token_end]);
            cursor = token_end;
            continue;
        }

        output.push_str(&message[cursor..token_start]);
        output.push_str(REDACTED);
        cursor = token_end;
    }

    output.push_str(&message[cursor..]);
    output
}

fn redact_secret_prefixes(message: &str) -> String {
    let mut output = String::with_capacity(message.len());
    let mut cursor = 0;

    while let Some((prefix_start, prefix)) = find_next_secret_prefix(message, cursor) {
        let secret_end = scan_secret_token_end(message, prefix_start);

        if secret_end - prefix_start <= prefix.len() + 6 {
            output.push_str(&message[cursor..prefix_start + prefix.len()]);
            cursor = prefix_start + prefix.len();
            continue;
        }

        output.push_str(&message[cursor..prefix_start]);
        output.push_str(REDACTED);
        cursor = secret_end;
    }

    output.push_str(&message[cursor..]);
    output
}

fn scan_unredacted_keyed_values(
    log: &str,
    keys: &[&str],
    marker_prefix: &str,
) -> Vec<PrivacyRegressionFailure> {
    let mut failures = Vec::new();

    for key in keys {
        let mut cursor = 0;
        while let Some(key_start) = find_keyed_value(log, key, cursor) {
            let Some(value) = parse_value_span(log, key_start, key) else {
                cursor = key_start + key.len();
                continue;
            };

            if !value_is_redacted(log, value.value_start, value.value_end)
                && value.value_start < value.value_end
            {
                failures.push(PrivacyRegressionFailure {
                    marker: format!("{marker_prefix}:{key}"),
                    byte_offset: key_start,
                    excerpt: excerpt_around(log, key_start, key.len()),
                });
            }

            cursor = value.after_value;
        }
    }

    failures
}

fn scan_unredacted_token_schemes(log: &str) -> Vec<PrivacyRegressionFailure> {
    let mut failures = Vec::new();
    let mut cursor = 0;

    while let Some((scheme_start, scheme)) = find_next_token_scheme(log, cursor) {
        let after_scheme = scheme_start + scheme.len();
        let token_start = skip_ascii_whitespace(log, after_scheme);
        let token_end = scan_bare_value_end(log, token_start);

        if token_start > after_scheme
            && token_start < token_end
            && !value_is_redacted(log, token_start, token_end)
        {
            failures.push(PrivacyRegressionFailure {
                marker: format!("credential:{scheme}"),
                byte_offset: scheme_start,
                excerpt: excerpt_around(log, scheme_start, scheme.len()),
            });
        }

        cursor = token_end.max(after_scheme);
    }

    failures
}

fn scan_unredacted_secret_prefixes(log: &str) -> Vec<PrivacyRegressionFailure> {
    let mut failures = Vec::new();
    let mut cursor = 0;

    while let Some((prefix_start, prefix)) = find_next_secret_prefix(log, cursor) {
        let secret_end = scan_secret_token_end(log, prefix_start);

        if secret_end - prefix_start > prefix.len() + 6 {
            failures.push(PrivacyRegressionFailure {
                marker: format!("credential:{prefix}"),
                byte_offset: prefix_start,
                excerpt: excerpt_around(log, prefix_start, prefix.len()),
            });
        }

        cursor = secret_end.max(prefix_start + prefix.len());
    }

    failures
}

#[derive(Debug, Clone, Copy)]
struct ValueSpan {
    value_start: usize,
    value_end: usize,
    after_value: usize,
}

fn find_keyed_value(text: &str, key: &str, start: usize) -> Option<usize> {
    let mut cursor = start;

    while let Some(relative) = find_ascii_case_insensitive(&text[cursor..], key) {
        let key_start = cursor + relative;
        let key_end = key_start + key.len();

        if has_key_boundary(text, key_start, key_end) {
            return Some(key_start);
        }

        cursor = key_end;
    }

    None
}

fn parse_value_span(text: &str, key_start: usize, key: &str) -> Option<ValueSpan> {
    let bytes = text.as_bytes();
    let mut index = key_start + key.len();

    index = skip_ascii_whitespace(text, index);

    if bytes.get(index) == Some(&b'"') && key_start > 0 && bytes.get(key_start - 1) == Some(&b'"') {
        index += 1;
        index = skip_ascii_whitespace(text, index);
    }

    if !matches!(bytes.get(index), Some(b':') | Some(b'=')) {
        return None;
    }

    index += 1;
    index = skip_ascii_whitespace(text, index);

    if bytes.get(index) == Some(&b'"') {
        let value_start = index + 1;
        let value_end = scan_quoted_value_end(text, value_start)?;
        Some(ValueSpan {
            value_start,
            value_end,
            after_value: value_end + 1,
        })
    } else {
        let value_start = index;
        let value_end = scan_bare_value_end(text, value_start);
        Some(ValueSpan {
            value_start,
            value_end,
            after_value: value_end,
        })
    }
}

fn scan_quoted_value_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = start;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Some(index),
            b'\\' => {
                index = index.checked_add(2)?;
            }
            _ => index += 1,
        }
    }

    None
}

fn scan_bare_value_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut index = start;

    while index < bytes.len() {
        match bytes[index] {
            b'\n' | b'\r' | b',' | b'&' | b'}' | b']' => break,
            _ => index += 1,
        }
    }

    trim_ascii_whitespace_end(text, start, index)
}

fn scan_secret_token_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut index = start;

    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'-' | b'_' | b'.'))
    {
        index += 1;
    }

    index
}

fn skip_ascii_whitespace(text: &str, mut index: usize) -> usize {
    let bytes = text.as_bytes();

    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }

    index
}

fn trim_ascii_whitespace_end(text: &str, start: usize, mut end: usize) -> usize {
    let bytes = text.as_bytes();

    while end > start && matches!(bytes[end - 1], b' ' | b'\t') {
        end -= 1;
    }

    end
}

fn value_is_redacted(text: &str, start: usize, end: usize) -> bool {
    let value = text[start..end].trim();

    value == REDACTED
        || value.eq_ignore_ascii_case(&format!("bearer {REDACTED}"))
        || value.eq_ignore_ascii_case(&format!("basic {REDACTED}"))
}

fn has_key_boundary(text: &str, start: usize, end: usize) -> bool {
    let bytes = text.as_bytes();
    let before = start.checked_sub(1).and_then(|index| bytes.get(index));
    let after = bytes.get(end);

    !before.is_some_and(|byte| is_key_continuation(*byte))
        && !after.is_some_and(|byte| is_key_continuation(*byte))
}

fn is_key_continuation(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn find_next_token_scheme(text: &str, start: usize) -> Option<(usize, &'static str)> {
    find_next_pattern(text, start, TOKEN_SCHEMES, true)
}

fn find_next_secret_prefix(text: &str, start: usize) -> Option<(usize, &'static str)> {
    find_next_pattern(text, start, SECRET_PREFIXES, false)
}

fn find_next_pattern(
    text: &str,
    start: usize,
    patterns: &'static [&'static str],
    require_boundary: bool,
) -> Option<(usize, &'static str)> {
    let mut best: Option<(usize, &'static str)> = None;

    for pattern in patterns {
        let mut cursor = start;
        while let Some(relative) = find_ascii_case_insensitive(&text[cursor..], pattern) {
            let pattern_start = cursor + relative;
            let pattern_end = pattern_start + pattern.len();

            if !require_boundary || has_key_boundary(text, pattern_start, pattern_end) {
                if best.is_none_or(|(best_start, _)| pattern_start < best_start) {
                    best = Some((pattern_start, *pattern));
                }
                break;
            }

            cursor = pattern_end;
        }
    }

    best
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();

    if needle.len() > haystack.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn excerpt_around(text: &str, offset: usize, marker_len: usize) -> String {
    let mut start = offset.saturating_sub(32);
    let mut end = (offset + marker_len + 32).min(text.len());

    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }

    text[start..end].replace(['\n', '\r'], "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_header_values_case_insensitively() {
        let headers = [
            ("Authorization", "Bearer sk-live-authorization-secret"),
            ("api-key", "provider-api-key-secret"),
            ("X-API-KEY", "provider-x-api-key-secret"),
            ("Cookie", "session=secret; workspace=private"),
            ("content-type", "application/json"),
        ];

        let redacted = redact_sensitive_headers(&headers);

        assert_eq!(
            redacted,
            vec![
                ("Authorization".to_string(), REDACTED.to_string()),
                ("api-key".to_string(), REDACTED.to_string()),
                ("X-API-KEY".to_string(), REDACTED.to_string()),
                ("Cookie".to_string(), REDACTED.to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ]
        );
    }

    #[test]
    fn redacts_provider_credentials_from_error_strings() {
        let error = concat!(
            "provider rejected request: Authorization: Bearer sk-live-credential-123456789\n",
            "x-api-key=provider-x-api-key-secret\n",
            "cookie: session=provider-cookie-secret"
        );

        let redacted = redact_provider_credentials(error);

        assert!(redacted.contains("Authorization: "));
        assert!(redacted.contains(REDACTED));
        assert!(!redacted.contains("sk-live-credential-123456789"));
        assert!(!redacted.contains("provider-x-api-key-secret"));
        assert!(!redacted.contains("provider-cookie-secret"));
    }

    #[test]
    fn redacts_prompt_and_content_from_provider_errors() {
        let error = concat!(
            r#"upstream 400 {"prompt":"PRIVATE_PROMPT_FIXTURE ship customer plan","#,
            r#""messages":"PRIVATE_CONTENT_FIXTURE response body","#,
            r#""authorization":"Bearer sk-provider-json-secret-123456"}"#
        );

        let redacted = redact_provider_error(error);

        assert!(redacted.contains(r#""prompt":"[REDACTED]""#));
        assert!(redacted.contains(r#""messages":"[REDACTED]""#));
        assert!(redacted.contains(r#""authorization":"[REDACTED]""#));
        assert!(!redacted.contains("PRIVATE_PROMPT_FIXTURE"));
        assert!(!redacted.contains("PRIVATE_CONTENT_FIXTURE"));
        assert!(!redacted.contains("sk-provider-json-secret-123456"));
    }

    #[test]
    fn regression_scan_allows_redacted_logs() {
        let log =
            r#"{"usage":{"input_tokens":10},"prompt":"[REDACTED]","authorization":"[REDACTED]"}"#;

        assert_eq!(scan_persisted_log_string(log), Vec::new());
    }

    #[test]
    fn regression_scan_fails_on_fixture_secret_markers() {
        let log = concat!(
            r#"{"event":"proxy","#,
            r#""credential":"TOKMETER_FIXTURE_SECRET","#,
            r#""prompt":"PROMPT_SHOULD_NOT_PERSIST","#,
            r#""content":"CONTENT_SHOULD_NOT_PERSIST"}"#
        );

        let failures = scan_persisted_log_string(log);

        assert!(
            failures
                .iter()
                .any(|failure| failure.marker == "TOKMETER_FIXTURE_SECRET")
        );
        assert!(
            failures
                .iter()
                .any(|failure| failure.marker == "PROMPT_SHOULD_NOT_PERSIST")
        );
        assert!(
            failures
                .iter()
                .any(|failure| failure.marker == "CONTENT_SHOULD_NOT_PERSIST")
        );
    }

    #[test]
    fn custom_marker_scan_reports_offsets_and_excerpts() {
        let log = "safe prefix\nraw fixture PRIVATE_CREDENTIAL_FIXTURE persisted\nsafe suffix";

        let failures = scan_persisted_log_string_for_markers(log, &["PRIVATE_CREDENTIAL_FIXTURE"]);

        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].marker, "PRIVATE_CREDENTIAL_FIXTURE");
        assert_eq!(
            failures[0].byte_offset,
            log.find("PRIVATE_CREDENTIAL_FIXTURE").unwrap()
        );
        assert!(failures[0].excerpt.contains("raw fixture"));
    }
}
