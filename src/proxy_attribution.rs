pub const DIGEST_PLACEHOLDER: &str = "placeholder";
pub const UNKNOWN_TOOL: &str = "unknown-tool";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionEvent {
    pub op_class: String,
    pub tool: String,
    pub byte_count: u64,
    pub digest: String,
    pub token_allocation: Option<TokenAllocation>,
    pub unattributed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAllocation {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolUse {
    id: Option<String>,
    tool: String,
    command: Option<String>,
}

pub fn attribution_events_from_json(
    request_json: &str,
    message_json: &str,
) -> Vec<AttributionEvent> {
    let tool_uses = collect_tool_uses(request_json);
    let mut events = Vec::new();

    for block in tool_result_blocks(message_json) {
        let tool_use_id = extract_string_any(block.text, &["tool_use_id", "tool_call_id", "id"]);
        let direct_tool =
            extract_string_any(block.text, &["tool", "tool_name", "name", "recipient_name"])
                .map(normalize_tool_name);
        let direct_command = extract_command(block.text);
        let matched_tool_use = tool_use_id.as_deref().and_then(|id| {
            tool_uses
                .iter()
                .find(|tool_use| tool_use.id.as_deref() == Some(id))
        });

        let tool = direct_tool
            .or_else(|| matched_tool_use.map(|tool_use| tool_use.tool.clone()))
            .unwrap_or_else(|| UNKNOWN_TOOL.to_owned());
        let command = direct_command
            .or_else(|| matched_tool_use.and_then(|tool_use| tool_use.command.clone()));
        let op_class = classify_event(&tool, command.as_deref());
        let unattributed = tool == UNKNOWN_TOOL || op_class == "other";

        events.push(AttributionEvent {
            op_class: op_class.to_owned(),
            tool,
            byte_count: result_byte_count(block.text),
            digest: DIGEST_PLACEHOLDER.to_owned(),
            token_allocation: extract_token_allocation(block.text),
            unattributed,
        });
    }

    events
}

pub fn attribute_proxy_json(request_json: &str, message_json: &str) -> Vec<AttributionEvent> {
    attribution_events_from_json(request_json, message_json)
}

pub fn attribute_json(request_json: &str, message_json: &str) -> Vec<AttributionEvent> {
    attribution_events_from_json(request_json, message_json)
}

pub fn attribution_events_from_message_json(message_json: &str) -> Vec<AttributionEvent> {
    attribution_events_from_json("", message_json)
}

fn collect_tool_uses(input: &str) -> Vec<ToolUse> {
    json_object_slices(input)
        .into_iter()
        .filter_map(|slice| {
            let object = &input[slice.start..slice.end];

            if !is_tool_use_block(object) {
                return None;
            }

            let tool = extract_string_any(object, &["tool", "tool_name", "name", "recipient_name"])
                .map(normalize_tool_name)?;
            let command = extract_command(object);

            Some(ToolUse {
                id: extract_string_any(object, &["tool_use_id", "tool_call_id", "id", "call_id"]),
                tool,
                command,
            })
        })
        .collect()
}

fn extract_command(object: &str) -> Option<String> {
    extract_string_any(object, &["command", "cmd"])
        .or_else(|| {
            extract_string(object, "input")
                .and_then(|input| extract_string_any(&input, &["command", "cmd"]).or(Some(input)))
        })
        .or_else(|| {
            extract_string(object, "arguments").and_then(|arguments| {
                extract_string_any(&arguments, &["command", "cmd", "input"]).or(Some(arguments))
            })
        })
}

fn tool_result_blocks(input: &str) -> Vec<JsonSlice<'_>> {
    let candidates: Vec<JsonRange> = json_object_slices(input)
        .into_iter()
        .filter(|slice| is_tool_result_block(&input[slice.start..slice.end]))
        .collect();

    candidates
        .iter()
        .filter(|candidate| {
            !candidates
                .iter()
                .any(|other| other.start > candidate.start && other.end < candidate.end)
        })
        .map(|slice| JsonSlice {
            text: &input[slice.start..slice.end],
        })
        .collect()
}

fn is_tool_use_block(object: &str) -> bool {
    has_string_value(object, "type", "tool_call")
        || has_string_value(object, "type", "tool_use")
        || object.contains("\"tool_calls\"")
        || object.contains("\"toolUse\"")
        || object.contains("\"tool_use\"")
}

fn is_tool_result_block(object: &str) -> bool {
    has_string_value(object, "type", "tool_result")
        || has_string_value(object, "type", "tool_output")
        || has_string_value(object, "role", "tool")
        || object.contains("\"tool_result\"")
        || object.contains("\"tool_results\"")
        || object.contains("\"toolResult\"")
        || object.contains("\"tool_output\"")
}

fn classify_event(tool: &str, command: Option<&str>) -> &'static str {
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

fn result_byte_count(object: &str) -> u64 {
    extract_u64_any(object, &["byte_count", "bytes"])
        .or_else(|| {
            extract_string_any(object, &["content", "output", "result", "tool_output"])
                .map(|content| content.len().try_into().unwrap_or(u64::MAX))
        })
        .unwrap_or_else(|| object.len().try_into().unwrap_or(u64::MAX))
}

fn extract_token_allocation(object: &str) -> Option<TokenAllocation> {
    let has_usage = has_any_key(
        object,
        &[
            "input_tokens",
            "prompt_tokens",
            "output_tokens",
            "completion_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ],
    );

    has_usage.then(|| TokenAllocation {
        input_tokens: extract_u64_any(object, &["input_tokens", "prompt_tokens"]).unwrap_or(0),
        output_tokens: extract_u64_any(object, &["output_tokens", "completion_tokens"])
            .unwrap_or(0),
        cache_read_tokens: extract_u64_any(
            object,
            &["cache_read_tokens", "cache_read_input_tokens"],
        )
        .unwrap_or(0),
        cache_write_tokens: extract_u64_any(
            object,
            &["cache_write_tokens", "cache_creation_input_tokens"],
        )
        .unwrap_or(0),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JsonRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JsonSlice<'a> {
    text: &'a str,
}

fn json_object_slices(input: &str) -> Vec<JsonRange> {
    let mut ranges = Vec::new();
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        if in_string {
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push(index),
            '}' => {
                if let Some(start) = stack.pop() {
                    ranges.push(JsonRange {
                        start,
                        end: index + ch.len_utf8(),
                    });
                }
            }
            _ => {}
        }
    }

    ranges
}

fn extract_string_any(input: &str, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| extract_string(input, key))
}

fn extract_u64_any(input: &str, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| extract_u64(input, key))
}

fn extract_string(input: &str, key: &str) -> Option<String> {
    let key_pattern = quoted_key(key);
    let mut cursor = 0;

    while let Some(relative_index) = input[cursor..].find(&key_pattern) {
        let key_start = cursor + relative_index;
        let after_key = key_start + key_pattern.len();
        let value_start = after_colon(input, after_key)?;

        if input[value_start..].starts_with('"') {
            return parse_json_string(input, value_start);
        }

        cursor = after_key;
    }

    None
}

fn extract_u64(input: &str, key: &str) -> Option<u64> {
    let key_pattern = quoted_key(key);
    let mut cursor = 0;

    while let Some(relative_index) = input[cursor..].find(&key_pattern) {
        let key_start = cursor + relative_index;
        let after_key = key_start + key_pattern.len();
        let mut value_start = after_colon(input, after_key)?;

        while input
            .as_bytes()
            .get(value_start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            value_start += 1;
        }

        let value_end = input[value_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count()
            + value_start;

        if value_end > value_start {
            return input[value_start..value_end].parse().ok();
        }

        cursor = after_key;
    }

    None
}

fn after_colon(input: &str, start: usize) -> Option<usize> {
    let colon_offset = input[start..].find(':')?;
    let mut value_start = start + colon_offset + 1;

    while input
        .as_bytes()
        .get(value_start)
        .is_some_and(u8::is_ascii_whitespace)
    {
        value_start += 1;
    }

    Some(value_start)
}

fn has_any_key(input: &str, keys: &[&str]) -> bool {
    keys.iter().any(|key| input.contains(&quoted_key(key)))
}

fn has_string_value(input: &str, key: &str, value: &str) -> bool {
    extract_string(input, key).is_some_and(|actual| actual == value)
}

fn quoted_key(key: &str) -> String {
    format!("\"{key}\"")
}

fn parse_json_string(input: &str, quote_start: usize) -> Option<String> {
    let mut output = String::new();
    let mut chars = input[quote_start + 1..].chars();
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            match ch {
                '"' | '\\' | '/' => output.push(ch),
                'b' => output.push('\u{0008}'),
                'f' => output.push('\u{000c}'),
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                'u' => {
                    let mut value = 0_u32;
                    for _ in 0..4 {
                        let hex = chars.next()?;
                        value = value.checked_mul(16)?.checked_add(hex.to_digit(16)?)?;
                    }
                    output.push(char::from_u32(value).unwrap_or('\u{fffd}'));
                }
                other => output.push(other),
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => return Some(output),
            other => output.push(other),
        }
    }

    None
}

fn normalize_tool_name(tool: String) -> String {
    tool.rsplit('.').next().unwrap_or(&tool).trim().to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_git_diff_tool_result_from_matching_request_call() {
        let request = r#"{
            "messages": [{
                "tool_calls": [{
                    "id": "call_1",
                    "type": "tool_call",
                    "name": "Bash",
                    "arguments": "git diff -- src/proxy.rs"
                }]
            }]
        }"#;
        let message = r#"{
            "content": [{
                "type": "tool_result",
                "tool_use_id": "call_1",
                "content": "diff --git a/src/proxy.rs b/src/proxy.rs\n+new line"
            }]
        }"#;

        let events = attribution_events_from_json(request, message);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].op_class, "vc.diff");
        assert_eq!(events[0].tool, "Bash");
        assert_eq!(events[0].byte_count, 50);
        assert_eq!(events[0].digest, DIGEST_PLACEHOLDER);
        assert_eq!(events[0].token_allocation, None);
        assert!(!events[0].unattributed);
    }

    #[test]
    fn attributes_file_read_tool_result_from_direct_tool_name() {
        let message = r#"{
            "content": [{
                "type": "tool_result",
                "tool": "Read",
                "content": "pub fn main() {}\n"
            }]
        }"#;

        let events = attribution_events_from_json("", message);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].op_class, "file.read");
        assert_eq!(events[0].tool, "Read");
        assert_eq!(events[0].byte_count, 17);
        assert!(!events[0].unattributed);
    }

    #[test]
    fn emits_unattributed_event_for_unknown_tool_result_block() {
        let message = r#"{
            "content": [{
                "type": "tool_result",
                "tool_use_id": "missing",
                "content": "opaque result"
            }]
        }"#;

        let events = attribution_events_from_json("", message);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].op_class, "other");
        assert_eq!(events[0].tool, UNKNOWN_TOOL);
        assert_eq!(events[0].byte_count, 13);
        assert!(events[0].unattributed);
    }

    #[test]
    fn event_does_not_persist_raw_tool_result_content() {
        let secret = "SECRET_RAW_TOOL_OUTPUT";
        let message = format!(
            r#"{{
                "content": [{{
                    "type": "tool_result",
                    "tool": "Read",
                    "content": "{secret}"
                }}]
            }}"#
        );

        let events = attribution_events_from_json("", &message);
        let rendered = format!("{events:?}");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].byte_count, 22);
        assert!(!events[0].digest.contains(secret));
        assert!(!events[0].op_class.contains(secret));
        assert!(!events[0].tool.contains(secret));
        assert!(!rendered.contains(secret));
    }
}
