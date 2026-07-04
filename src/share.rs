use std::collections::BTreeMap;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub const DEFAULT_DIGEST_HEX_CHARS: usize = 12;

pub type ShareMap = BTreeMap<String, ShareValue>;

#[derive(Clone, Debug, PartialEq)]
pub enum ShareValue {
    Map(ShareMap),
    List(Vec<ShareValue>),
    String(String),
    U64(u64),
    I64(i64),
    F64(f64),
    Bool(bool),
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShareSanitizer {
    salt: String,
    digest_hex_chars: usize,
}

impl ShareSanitizer {
    pub fn new(salt: impl Into<String>) -> Self {
        Self {
            salt: salt.into(),
            digest_hex_chars: DEFAULT_DIGEST_HEX_CHARS,
        }
    }

    pub fn with_digest_hex_chars(mut self, digest_hex_chars: usize) -> Self {
        self.digest_hex_chars = digest_hex_chars.max(1);
        self
    }

    pub fn sanitize(&self, value: &ShareValue) -> ShareValue {
        sanitize_value(value, self)
    }
}

pub fn sanitize_report(value: &ShareValue, salt: &str) -> ShareValue {
    ShareSanitizer::new(salt).sanitize(value)
}

/// Returns a salted, deterministic identifier for a sensitive value.
///
/// This is a stdlib-only FNV-1a fallback so share mode has stable local
/// redaction before a cryptographic digest dependency is introduced. It is not
/// collision-resistant and must not be treated as a secret-preserving hash.
pub fn salted_stable_hash(value: &str, salt: &str) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    feed_hash(&mut hash, salt.as_bytes());
    feed_hash(&mut hash, b"\0");
    feed_hash(&mut hash, value.as_bytes());
    format!("{hash:016x}")
}

pub fn hash_path(path: &str, salt: &str) -> String {
    salted_stable_hash(path, &format!("path:{salt}"))
}

pub fn hash_repo_name(repo: &str, salt: &str) -> String {
    salted_stable_hash(repo, &format!("repo:{salt}"))
}

pub fn truncate_digest(digest: &str, hex_chars: usize) -> String {
    let hex_chars = hex_chars.max(1);

    if digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return take_chars(digest, hex_chars);
    }

    for separator in [':', '-'] {
        if let Some((algorithm, suffix)) = digest.split_once(separator) {
            if !algorithm.is_empty()
                && !suffix.is_empty()
                && suffix.chars().all(|ch| ch.is_ascii_hexdigit())
            {
                return format!("{algorithm}{separator}{}", take_chars(suffix, hex_chars));
            }
        }
    }

    take_chars(digest, hex_chars)
}

fn sanitize_value(value: &ShareValue, sanitizer: &ShareSanitizer) -> ShareValue {
    match value {
        ShareValue::Map(map) => sanitize_map(map, sanitizer),
        ShareValue::List(values) => ShareValue::List(
            values
                .iter()
                .map(|value| sanitize_value(value, sanitizer))
                .collect(),
        ),
        ShareValue::String(value) => ShareValue::String(salted_stable_hash(
            value,
            &format!("text:{}", sanitizer.salt),
        )),
        ShareValue::U64(_)
        | ShareValue::I64(_)
        | ShareValue::F64(_)
        | ShareValue::Bool(_)
        | ShareValue::Null => value.clone(),
    }
}

fn sanitize_map(map: &ShareMap, sanitizer: &ShareSanitizer) -> ShareValue {
    let mut sanitized = BTreeMap::new();

    for (key, value) in map {
        let normalized_key = normalize_field_name(key);
        if is_private_text_field(&normalized_key) {
            insert_sensitive_summary(&mut sanitized, &normalized_key, value);
            continue;
        }

        if normalized_key == "path" {
            sanitized.insert(
                "path_hash".to_string(),
                hash_sensitive_value(value, &format!("path:{}", sanitizer.salt)),
            );
            continue;
        }

        if normalized_key == "repo" || normalized_key == "repository" {
            sanitized.insert(
                format!("{normalized_key}_hash"),
                hash_sensitive_value(value, &format!("repo:{}", sanitizer.salt)),
            );
            continue;
        }

        let output_key = output_key_for(key, &normalized_key, value, sanitizer);
        let output_value = sanitize_field_value(&normalized_key, value, sanitizer);
        sanitized.insert(output_key, output_value);
    }

    ShareValue::Map(sanitized)
}

fn sanitize_field_value(
    normalized_key: &str,
    value: &ShareValue,
    sanitizer: &ShareSanitizer,
) -> ShareValue {
    match value {
        ShareValue::Map(map) => sanitize_map(map, sanitizer),
        ShareValue::List(values) => ShareValue::List(
            values
                .iter()
                .map(|value| sanitize_field_value(normalized_key, value, sanitizer))
                .collect(),
        ),
        ShareValue::String(text) if is_digest_key(normalized_key) => {
            if is_digest_like(text) {
                ShareValue::String(truncate_digest(text, sanitizer.digest_hex_chars))
            } else {
                ShareValue::String(format!(
                    "digest_hash:{}",
                    salted_stable_hash(text, &format!("digest:{}", sanitizer.salt))
                ))
            }
        }
        ShareValue::String(text) if is_hash_key(normalized_key) => ShareValue::String(
            salted_stable_hash(text, &format!("hash:{}", sanitizer.salt)),
        ),
        ShareValue::String(text) if is_category_key(normalized_key) => {
            ShareValue::String(sanitize_category(text, sanitizer))
        }
        ShareValue::String(text) => ShareValue::String(salted_stable_hash(
            text,
            &format!("text:{}", sanitizer.salt),
        )),
        ShareValue::U64(_)
        | ShareValue::I64(_)
        | ShareValue::F64(_)
        | ShareValue::Bool(_)
        | ShareValue::Null => value.clone(),
    }
}

fn insert_sensitive_summary(out: &mut ShareMap, key: &str, value: &ShareValue) {
    let summary = SensitiveSummary::from_value(value);
    out.insert(
        format!("{key}_byte_count"),
        ShareValue::U64(summary.byte_count),
    );
    out.insert(
        format!("{key}_value_count"),
        ShareValue::U64(summary.value_count),
    );
}

fn hash_sensitive_value(value: &ShareValue, salt: &str) -> ShareValue {
    match value {
        ShareValue::String(text) => ShareValue::String(salted_stable_hash(text, salt)),
        ShareValue::List(values) => ShareValue::List(
            values
                .iter()
                .map(|value| hash_sensitive_value(value, salt))
                .collect(),
        ),
        ShareValue::Map(map) => {
            let mut hashed = BTreeMap::new();
            for (key, value) in map {
                hashed.insert(
                    format!("entry_hash_{}", salted_stable_hash(key, salt)),
                    hash_sensitive_value(value, salt),
                );
            }
            ShareValue::Map(hashed)
        }
        ShareValue::U64(value) => ShareValue::String(salted_stable_hash(&value.to_string(), salt)),
        ShareValue::I64(value) => ShareValue::String(salted_stable_hash(&value.to_string(), salt)),
        ShareValue::F64(value) => ShareValue::String(salted_stable_hash(&value.to_string(), salt)),
        ShareValue::Bool(value) => ShareValue::String(salted_stable_hash(&value.to_string(), salt)),
        ShareValue::Null => ShareValue::String(salted_stable_hash("null", salt)),
    }
}

fn safe_output_key(key: &str, sanitizer: &ShareSanitizer) -> String {
    let normalized_key = normalize_field_name(key);
    if looks_like_path(key) || !is_plain_field_name(&normalized_key) {
        return format!(
            "field_hash_{}",
            salted_stable_hash(key, &format!("field:{}", sanitizer.salt))
        );
    }
    normalized_key
}

fn output_key_for(
    key: &str,
    normalized_key: &str,
    value: &ShareValue,
    sanitizer: &ShareSanitizer,
) -> String {
    let output_key = safe_output_key(key, sanitizer);
    if matches!(value, ShareValue::String(_))
        && !is_digest_key(normalized_key)
        && !is_hash_key(normalized_key)
        && !is_category_key(normalized_key)
        && !output_key.ends_with("_hash")
    {
        format!("{output_key}_hash")
    } else {
        output_key
    }
}

fn normalize_field_name(key: &str) -> String {
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

fn is_private_text_field(key: &str) -> bool {
    matches!(key, "prompt" | "content" | "source" | "tool_output" | "raw")
}

fn is_digest_key(key: &str) -> bool {
    key == "digest" || key.ends_with("_digest") || key.ends_with("_sha")
}

fn is_hash_key(key: &str) -> bool {
    key == "hash" || key.ends_with("_hash")
}

fn is_category_key(key: &str) -> bool {
    matches!(
        key,
        "category" | "class" | "event_class" | "kind" | "model" | "profile" | "status" | "type"
    ) || key.ends_with("_category")
        || key.ends_with("_class")
        || key.ends_with("_kind")
        || key.ends_with("_status")
        || key.ends_with("_type")
}

fn sanitize_category(value: &str, sanitizer: &ShareSanitizer) -> String {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
        && !looks_like_path(value)
    {
        value.to_ascii_lowercase()
    } else {
        format!(
            "category_hash:{}",
            salted_stable_hash(value, &format!("category:{}", sanitizer.salt))
        )
    }
}

fn is_plain_field_name(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 80
        && key
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/')
        || value.contains('\\')
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
}

fn is_digest_like(value: &str) -> bool {
    if value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return true;
    }

    [':', '-'].iter().any(|separator| {
        value
            .split_once(*separator)
            .map(|(algorithm, suffix)| {
                !algorithm.is_empty()
                    && algorithm
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                    && !suffix.is_empty()
                    && suffix.chars().all(|ch| ch.is_ascii_hexdigit())
            })
            .unwrap_or(false)
    })
}

fn take_chars(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}

fn feed_hash(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SensitiveSummary {
    byte_count: u64,
    value_count: u64,
}

impl SensitiveSummary {
    fn from_value(value: &ShareValue) -> Self {
        match value {
            ShareValue::String(text) => Self {
                byte_count: text.len() as u64,
                value_count: 1,
            },
            ShareValue::List(values) => {
                values.iter().fold(Self::default(), |mut summary, value| {
                    let child = Self::from_value(value);
                    summary.byte_count += child.byte_count;
                    summary.value_count += child.value_count;
                    summary
                })
            }
            ShareValue::Map(map) => map.values().fold(Self::default(), |mut summary, value| {
                let child = Self::from_value(value);
                summary.byte_count += child.byte_count;
                summary.value_count += child.value_count;
                summary
            }),
            ShareValue::U64(_)
            | ShareValue::I64(_)
            | ShareValue::F64(_)
            | ShareValue::Bool(_)
            | ShareValue::Null => Self {
                byte_count: 0,
                value_count: 1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(value: &str) -> ShareValue {
        ShareValue::String(value.to_string())
    }

    fn map(entries: impl IntoIterator<Item = (&'static str, ShareValue)>) -> ShareValue {
        ShareValue::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn salted_hash_is_stable_and_salt_scoped() {
        let first = hash_path("/Users/alice/private/repo/src/main.rs", "salt-a");
        let second = hash_path("/Users/alice/private/repo/src/main.rs", "salt-a");
        let different_salt = hash_path("/Users/alice/private/repo/src/main.rs", "salt-b");
        let repo_hash = hash_repo_name("secret-repository", "salt-a");

        assert_eq!(first, second);
        assert_eq!(first.len(), 16);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, different_salt);
        assert_ne!(first, repo_hash);
    }

    #[test]
    fn digest_truncation_preserves_algorithm_prefix() {
        assert_eq!(
            truncate_digest(
                "sha256:abcdef1234567890fedcba0987654321abcdef1234567890fedcba0987654321",
                12
            ),
            "sha256:abcdef123456"
        );
        assert_eq!(truncate_digest("abcdef1234567890", 8), "abcdef12");
    }

    #[test]
    fn sanitizer_replaces_sensitive_fields_with_hashes_and_counts() {
        let report = map([
            ("path", string("/Users/alice/src/acme-private/src/lib.rs")),
            ("repo", string("acme-private")),
            ("repository", string("github.com/example/acme-private")),
            ("prompt", string("ship the secret customer migration")),
            ("content", string("fn secret_customer_migration() {}")),
            ("source", string("private transcript source")),
            ("tool_output", string("DATABASE_URL=postgres://secret")),
            ("raw", string("{\"private\":\"payload\"}")),
            ("category", string("Generated_File")),
            ("duration_ms", ShareValue::U64(42)),
            ("token_count", ShareValue::U64(9001)),
            (
                "digest",
                string("sha256:abcdef1234567890fedcba0987654321abcdef1234567890"),
            ),
        ]);

        let sanitized = sanitize_report(&report, "share-test-salt");
        let rendered = format!("{sanitized:?}");

        for sensitive in [
            "/Users/alice/src/acme-private/src/lib.rs",
            "acme-private",
            "github.com/example/acme-private",
            "ship the secret customer migration",
            "fn secret_customer_migration() {}",
            "private transcript source",
            "DATABASE_URL=postgres://secret",
            "{\"private\":\"payload\"}",
            "abcdef1234567890fedcba0987654321abcdef1234567890",
        ] {
            assert!(
                !rendered.contains(sensitive),
                "sanitized output leaked {:?}: {}",
                sensitive,
                rendered
            );
        }

        assert!(rendered.contains("path_hash"));
        assert!(rendered.contains("repo_hash"));
        assert!(rendered.contains("repository_hash"));
        assert!(rendered.contains("prompt_byte_count"));
        assert!(rendered.contains("content_byte_count"));
        assert!(rendered.contains("source_byte_count"));
        assert!(rendered.contains("tool_output_byte_count"));
        assert!(rendered.contains("raw_byte_count"));
        assert!(rendered.contains("Generated_File") == false);
        assert!(rendered.contains("generated_file"));
        assert!(rendered.contains("duration_ms"));
        assert!(rendered.contains("token_count"));
        assert!(rendered.contains("sha256:abcdef123456"));
    }

    #[test]
    fn sanitizer_hashes_path_like_map_keys_and_unknown_strings() {
        let report = map([
            (
                "/Users/alice/src/acme-private/README.md",
                map([("line_count", ShareValue::U64(10))]),
            ),
            ("message", string("customer name is Example Corp")),
            (
                "items",
                ShareValue::List(vec![
                    map([
                        ("path", string("src/secret.rs")),
                        ("status", string("Completed")),
                    ]),
                    string("loose sensitive note"),
                ]),
            ),
        ]);

        let sanitized = sanitize_report(&report, "nested-salt");
        let rendered = format!("{sanitized:?}");

        for sensitive in [
            "/Users/alice/src/acme-private/README.md",
            "acme-private",
            "customer name is Example Corp",
            "src/secret.rs",
            "loose sensitive note",
        ] {
            assert!(
                !rendered.contains(sensitive),
                "sanitized output leaked {:?}: {}",
                sensitive,
                rendered
            );
        }

        assert!(rendered.contains("field_hash_"));
        assert!(rendered.contains("line_count"));
        assert!(rendered.contains("message_hash"));
        assert!(rendered.contains("path_hash"));
        assert!(rendered.contains("completed"));
    }
}
