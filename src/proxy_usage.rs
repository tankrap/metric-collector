#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProviderUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

impl ProviderUsage {
    pub fn from_response_json(response_json: &str) -> Self {
        extract_provider_usage(response_json)
    }

    pub const fn has_any_usage(&self) -> bool {
        self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_write_tokens.is_some()
    }
}

pub fn extract_provider_usage(response_json: &str) -> ProviderUsage {
    ProviderUsage {
        input_tokens: extract_u64_any(response_json, &["input_tokens", "prompt_tokens"]),
        output_tokens: extract_u64_any(response_json, &["output_tokens", "completion_tokens"]),
        cache_read_tokens: extract_u64_any(
            response_json,
            &["cache_read_tokens", "cache_read_input_tokens"],
        ),
        cache_write_tokens: extract_u64_any(
            response_json,
            &["cache_write_tokens", "cache_creation_input_tokens"],
        ),
    }
}

fn extract_u64_any(response_json: &str, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| extract_u64_for_json_key(response_json, key))
}

fn extract_u64_for_json_key(response_json: &str, key: &str) -> Option<u64> {
    let bytes = response_json.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }

        let Some(after_string) = skip_json_string(response_json, index) else {
            return None;
        };

        let after_whitespace = skip_json_whitespace(bytes, after_string);
        if after_whitespace >= bytes.len() || bytes[after_whitespace] != b':' {
            index = after_string;
            continue;
        }

        if json_string_matches(response_json, index, after_string, key) {
            let value_start = skip_json_whitespace(bytes, after_whitespace + 1);
            if let Some(value) = read_u64(bytes, value_start) {
                return Some(value);
            }
        }

        index = after_string;
    }

    None
}

fn skip_json_string(response_json: &str, start: usize) -> Option<usize> {
    let bytes = response_json.as_bytes();
    let mut index = start.checked_add(1)?;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Some(index + 1),
            b'\\' => {
                let escape_index = index + 1;
                if escape_index >= bytes.len() {
                    return None;
                }

                match bytes[escape_index] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => index += 2,
                    b'u' => {
                        if escape_index + 4 >= bytes.len() {
                            return None;
                        }
                        index += 6;
                    }
                    _ => return None,
                }
            }
            byte if byte < 0x20 => return None,
            _ => index += 1,
        }
    }

    None
}

fn json_string_matches(response_json: &str, start: usize, after_string: usize, key: &str) -> bool {
    if after_string <= start + 1 {
        return key.is_empty();
    }

    let content = &response_json.as_bytes()[start + 1..after_string - 1];
    !content.contains(&b'\\') && content == key.as_bytes()
}

fn skip_json_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\n' | b'\r' | b'\t') {
        index += 1;
    }

    index
}

fn read_u64(bytes: &[u8], start: usize) -> Option<u64> {
    if start >= bytes.len() || !bytes[start].is_ascii_digit() {
        return None;
    }

    let mut index = start;
    let mut value = 0_u64;

    while index < bytes.len() && bytes[index].is_ascii_digit() {
        let digit = u64::from(bytes[index] - b'0');
        value = value.checked_mul(10)?.checked_add(digit)?;
        index += 1;
    }

    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_full_cache_fields_exactly() {
        let usage = extract_provider_usage(
            r#"{
                "id": "response-1",
                "usage": {
                    "input_tokens": 120,
                    "output_tokens": 45,
                    "cache_read_tokens": 30,
                    "cache_write_tokens": 7
                }
            }"#,
        );

        assert_eq!(
            usage,
            ProviderUsage {
                input_tokens: Some(120),
                output_tokens: Some(45),
                cache_read_tokens: Some(30),
                cache_write_tokens: Some(7),
            }
        );
        assert!(usage.has_any_usage());
    }

    #[test]
    fn represents_missing_cache_fields_explicitly() {
        let usage = ProviderUsage::from_response_json(
            r#"{
                "usage": {
                    "input_tokens": 18,
                    "output_tokens": 5
                }
            }"#,
        );

        assert_eq!(usage.input_tokens, Some(18));
        assert_eq!(usage.output_tokens, Some(5));
        assert_eq!(usage.cache_read_tokens, None);
        assert_eq!(usage.cache_write_tokens, None);
    }

    #[test]
    fn extracts_common_aliases() {
        let usage = extract_provider_usage(
            r#"{
                "usage": {
                    "prompt_tokens": 250,
                    "completion_tokens": 33,
                    "cache_read_input_tokens": 200,
                    "cache_creation_input_tokens": 11
                }
            }"#,
        );

        assert_eq!(
            usage,
            ProviderUsage {
                input_tokens: Some(250),
                output_tokens: Some(33),
                cache_read_tokens: Some(200),
                cache_write_tokens: Some(11),
            }
        );
    }

    #[test]
    fn usage_records_do_not_leak_prompt_or_response_content() {
        let response_json = r#"{
            "usage": {
                "input_tokens": 9,
                "output_tokens": 3,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0
            },
            "prompt": "private prompt with api-key-like text",
            "choices": [
                {
                    "message": {
                        "content": "private response with \"input_tokens\": 9999 in text"
                    }
                }
            ]
        }"#;

        let usage = extract_provider_usage(response_json);
        let debug = format!("{usage:?}");

        assert_eq!(usage.input_tokens, Some(9));
        assert_eq!(usage.output_tokens, Some(3));
        assert!(!debug.contains("private prompt"));
        assert!(!debug.contains("private response"));
        assert!(!debug.contains("api-key-like"));
        assert!(!debug.contains("9999"));
    }
}
