use crate::classifier::{Classifier, Event};
use crate::core::EventLog;
use crate::core::{
    AttributedTokenEvent, CaptureMode, EventLogError, OperationClass, TokenCounts, ValidationError,
};
use crate::digest::digest_bytes;
use crate::mode::{DEFAULT_PASSIVE_PROFILE_ID, ModeState};

const DEFAULT_ADAPTER: &str = "claude-code-hook";
const UNKNOWN_TOOL: &str = "unknown-tool";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookCaptureMetadata<'a> {
    pub timestamp_ms: u64,
    pub mode: CaptureMode,
    pub run_id: &'a str,
    pub task_id: &'a str,
    pub profile_id: &'a str,
    pub adapter: &'a str,
}

impl<'a> HookCaptureMetadata<'a> {
    pub fn new(timestamp_ms: u64, run_id: &'a str, task_id: &'a str, profile_id: &'a str) -> Self {
        Self {
            timestamp_ms,
            mode: infer_mode(task_id),
            run_id,
            task_id,
            profile_id,
            adapter: DEFAULT_ADAPTER,
        }
    }

    pub fn passive(timestamp_ms: u64, run_id: &'a str) -> Self {
        Self {
            timestamp_ms,
            mode: CaptureMode::Passive,
            run_id,
            task_id: crate::core::ADHOC_TASK_ID,
            profile_id: DEFAULT_PASSIVE_PROFILE_ID,
            adapter: DEFAULT_ADAPTER,
        }
    }

    pub fn from_mode_state(timestamp_ms: u64, run_id: &'a str, state: &'a ModeState) -> Self {
        Self {
            timestamp_ms,
            mode: state.mode,
            run_id,
            task_id: &state.task_id,
            profile_id: &state.profile_id,
            adapter: DEFAULT_ADAPTER,
        }
    }

    pub const fn with_mode(mut self, mode: CaptureMode) -> Self {
        self.mode = mode;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPayloadFields<'a> {
    pub metadata: HookCaptureMetadata<'a>,
    pub tool_name: &'a str,
    pub command: Option<&'a str>,
    pub arguments: Option<&'a str>,
    pub result: Option<&'a str>,
    pub tokens: TokenCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedHookEvents {
    pub argument_digest: Option<String>,
    pub result_digest: Option<String>,
    pub argument_byte_count: u64,
    pub result_byte_count: u64,
    pub events: Vec<AttributedTokenEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedHookPayload {
    pub captured: CapturedHookEvents,
    pub event_log_records: Vec<u8>,
}

pub fn capture_hook_payload(
    payload: &HookPayloadFields<'_>,
) -> Result<CapturedHookEvents, ValidationError> {
    let operation_class = classify_payload(payload);
    let tool = safe_tool_name(payload.tool_name);
    let argument_digest = payload
        .arguments
        .filter(|value| !value.is_empty())
        .map(|value| hook_field_digest("arguments", value));
    let result_digest = payload
        .result
        .filter(|value| !value.is_empty())
        .map(|value| hook_field_digest("result", value));
    let argument_byte_count = payload.arguments.map(byte_count).unwrap_or(0);
    let result_byte_count = payload.result.map(byte_count).unwrap_or(0);
    let mut events = Vec::new();
    let has_argument = argument_digest.is_some();
    let has_result = result_digest.is_some();

    if let Some(digest) = &argument_digest {
        events.push(build_event(
            payload,
            operation_class,
            &tool,
            "arguments",
            tokens_for_arguments(payload.tokens.clone(), has_result),
            argument_byte_count,
            digest.clone(),
        )?);
    }

    if let Some(digest) = &result_digest {
        events.push(build_event(
            payload,
            operation_class,
            &tool,
            "result",
            tokens_for_result(payload.tokens.clone(), has_argument),
            result_byte_count,
            digest.clone(),
        )?);
    }

    if events.is_empty() && payload.tokens.total().unwrap_or(0) > 0 {
        events.push(build_event(
            payload,
            operation_class,
            &tool,
            "usage",
            payload.tokens.clone(),
            0,
            hook_empty_digest(),
        )?);
    }

    Ok(CapturedHookEvents {
        argument_digest,
        result_digest,
        argument_byte_count,
        result_byte_count,
        events,
    })
}

pub fn execute_hook_payload(
    payload: &HookPayloadFields<'_>,
) -> Result<ExecutedHookPayload, EventLogError> {
    let captured = capture_hook_payload(payload).map_err(EventLogError::Validation)?;
    let mut event_log_records = Vec::new();

    for event in &captured.events {
        EventLog::append_event(&mut event_log_records, event)?;
    }

    Ok(ExecutedHookPayload {
        captured,
        event_log_records,
    })
}

pub fn hook_events_from_payload(
    payload: &HookPayloadFields<'_>,
) -> Result<Vec<AttributedTokenEvent>, ValidationError> {
    capture_hook_payload(payload).map(|captured| captured.events)
}

pub fn hook_field_digest(field_name: &str, value: &str) -> String {
    let mut bytes =
        Vec::with_capacity("claude-code-hook".len() + field_name.len() + value.len() + 2);
    bytes.extend_from_slice(b"claude-code-hook");
    bytes.push(0);
    bytes.extend_from_slice(field_name.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(value.as_bytes());
    digest_bytes(&bytes)
}

fn build_event(
    payload: &HookPayloadFields<'_>,
    operation_class: OperationClass,
    tool: &str,
    suffix: &str,
    tokens: TokenCounts,
    byte_count: u64,
    content_digest: String,
) -> Result<AttributedTokenEvent, ValidationError> {
    let event = AttributedTokenEvent {
        timestamp_ms: payload.metadata.timestamp_ms,
        mode: payload.metadata.mode,
        run_id: payload.metadata.run_id.to_owned(),
        task_id: payload.metadata.task_id.to_owned(),
        profile_id: payload.metadata.profile_id.to_owned(),
        adapter: payload.metadata.adapter.to_owned(),
        operation_class,
        tool: format!("{tool}.{suffix}"),
        tokens,
        byte_count,
        content_digest,
        repeat_of: None,
    };
    event.validate()?;
    Ok(event)
}

fn classify_payload(payload: &HookPayloadFields<'_>) -> OperationClass {
    let command = payload.command.or_else(|| {
        is_bash_tool(payload.tool_name)
            .then_some(payload.arguments)
            .flatten()
    });
    let path = (!is_bash_tool(payload.tool_name))
        .then_some(payload.arguments)
        .flatten();
    let classification = Classifier::new().classify(&Event {
        tool_name: Some(payload.tool_name),
        command,
        path,
        output: payload.result,
    });

    operation_class_from_classifier(classification.operation_class)
}

fn operation_class_from_classifier(value: &str) -> OperationClass {
    match value {
        crate::classifier::GIT_STATUS => OperationClass::VcStatus,
        crate::classifier::GIT_DIFF => OperationClass::VcDiff,
        crate::classifier::GIT_LOG => OperationClass::VcLog,
        crate::classifier::GIT_SHOW => OperationClass::VcShow,
        crate::classifier::GIT_BRANCH => OperationClass::VcBranchOps,
        crate::classifier::GIT_PUSH | crate::classifier::GIT_PULL => OperationClass::VcPushPull,
        crate::classifier::FILE_READ => OperationClass::FileRead,
        crate::classifier::FILE_SEARCH => OperationClass::FileSearch,
        crate::classifier::FILE_LIST => OperationClass::FileList,
        crate::classifier::TEST_OUTPUT => OperationClass::TestOutput,
        crate::classifier::BUILD_OUTPUT => OperationClass::BuildOutput,
        crate::classifier::EDIT_ECHO => OperationClass::EditEcho,
        _ => OperationClass::Other,
    }
}

fn tokens_for_arguments(tokens: TokenCounts, has_result: bool) -> TokenCounts {
    if has_result {
        TokenCounts::new(
            tokens.input_tokens,
            0,
            tokens.cache_read_tokens,
            tokens.cache_write_tokens,
        )
    } else {
        tokens
    }
}

fn tokens_for_result(tokens: TokenCounts, has_argument: bool) -> TokenCounts {
    if has_argument {
        TokenCounts::new(0, tokens.output_tokens, 0, 0)
    } else {
        tokens
    }
}

fn byte_count(value: &str) -> u64 {
    value.len().try_into().unwrap_or(u64::MAX)
}

fn hook_empty_digest() -> String {
    hook_field_digest("usage", "")
}

fn infer_mode(task_id: &str) -> CaptureMode {
    if task_id == crate::core::ADHOC_TASK_ID {
        CaptureMode::Passive
    } else {
        CaptureMode::Task
    }
}

fn is_bash_tool(tool_name: &str) -> bool {
    tool_name.eq_ignore_ascii_case("bash")
}

fn safe_tool_name(tool_name: &str) -> String {
    let trimmed = tool_name.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || trimmed
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b':' | b'\t' | b'\n' | b'\r'))
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        UNKNOWN_TOOL.to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_ARG_MARKER: &str = "SECRET_RAW_ARGUMENT_MARKER";
    const RAW_PATH_MARKER: &str = "/tmp/SECRET_RAW_PATH_MARKER/file.txt";
    const RAW_RESULT_MARKER: &str = "SECRET_RAW_RESULT_MARKER";
    const RAW_PROMPT_MARKER: &str = "SECRET_RAW_PROMPT_MARKER";
    const RAW_COMMAND_MARKER: &str = "SECRET_RAW_COMMAND_MARKER";
    const RAW_SOURCE_MARKER: &str = "fn SECRET_RAW_SOURCE_MARKER() {}";

    #[test]
    fn captures_argument_and_result_as_valid_events_without_raw_payloads() {
        let payload = fixture_payload();
        let captured = capture_hook_payload(&payload).expect("hook payload captures");

        assert_eq!(
            captured.argument_byte_count,
            payload.arguments.unwrap().len() as u64
        );
        assert_eq!(
            captured.result_byte_count,
            payload.result.unwrap().len() as u64
        );
        assert_eq!(captured.events.len(), 2);
        assert_eq!(captured.events[0].operation_class, OperationClass::FileRead);
        assert_eq!(captured.events[0].tokens.input_tokens, 10);
        assert_eq!(captured.events[1].tokens.output_tokens, 20);

        for event in &captured.events {
            event.validate().expect("valid attributed event");
            assert_eq!(event.mode, CaptureMode::Task);
            assert!(event.content_digest.starts_with("fnv1a64:"));
            assert!(!event.content_digest.contains(RAW_ARG_MARKER));
            assert!(!event.content_digest.contains(RAW_RESULT_MARKER));
            assert!(!event.content_digest.contains(RAW_PATH_MARKER));
        }
    }

    #[test]
    fn privacy_regression_serialized_events_never_include_raw_markers() {
        let captured = capture_hook_payload(&fixture_payload()).expect("hook payload captures");
        let mut serialized = Vec::new();

        for event in &captured.events {
            EventLog::append_event(&mut serialized, event).expect("serializes event");
        }

        let output = String::from_utf8(serialized).expect("event log is utf8");
        for marker in [
            RAW_ARG_MARKER,
            RAW_PATH_MARKER,
            RAW_RESULT_MARKER,
            RAW_PROMPT_MARKER,
        ] {
            assert!(
                !output.contains(marker),
                "serialized output leaked {marker}"
            );
        }
        assert!(output.contains("tool=Read.arguments"));
        assert!(output.contains("tool=Read.result"));
    }

    #[test]
    fn executed_hook_payload_returns_appendable_privacy_safe_event_log_records() {
        let payload = HookPayloadFields {
            metadata: HookCaptureMetadata::new(5, "run-5", "task-5", "profile-5"),
            tool_name: "Bash",
            command: Some(RAW_COMMAND_MARKER),
            arguments: Some(RAW_ARG_MARKER),
            result: Some(RAW_SOURCE_MARKER),
            tokens: TokenCounts::new(13, 21, 1, 2),
        };

        let executed = execute_hook_payload(&payload).expect("hook payload executes");
        assert_eq!(executed.captured.events.len(), 2);

        let serialized =
            String::from_utf8(executed.event_log_records.clone()).expect("event log is utf8");
        let parsed = EventLog::read_from(serialized.as_bytes()).expect("event log parses");
        assert_eq!(parsed.events, executed.captured.events);

        for marker in [
            RAW_COMMAND_MARKER,
            RAW_ARG_MARKER,
            RAW_SOURCE_MARKER,
            RAW_PATH_MARKER,
            RAW_RESULT_MARKER,
            RAW_PROMPT_MARKER,
        ] {
            assert!(
                !serialized.contains(marker),
                "serialized output leaked {marker}"
            );
        }

        assert!(serialized.contains("adapter=claude-code-hook"));
        assert!(serialized.contains("op_class=other"));
        assert!(serialized.contains("tool=Bash.arguments"));
        assert!(serialized.contains("tool=Bash.result"));
    }

    #[test]
    fn classifies_bash_git_commands_without_persisting_command() {
        let payload = HookPayloadFields {
            metadata: HookCaptureMetadata::new(2, "run-2", "task-2", "profile-2"),
            tool_name: "Bash",
            command: None,
            arguments: Some("git diff -- src/main.rs"),
            result: Some("diff --git a/src/main.rs b/src/main.rs"),
            tokens: TokenCounts::new(1, 2, 0, 0),
        };

        let captured = capture_hook_payload(&payload).expect("hook payload captures");
        assert_eq!(captured.events[0].operation_class, OperationClass::VcDiff);

        let mut serialized = Vec::new();
        EventLog::append_event(&mut serialized, &captured.events[0]).expect("serializes event");
        let output = String::from_utf8(serialized).expect("event log is utf8");
        assert!(!output.contains("git diff"));
        assert!(!output.contains("src/main.rs"));
    }

    #[test]
    fn unsafe_tool_names_are_not_persisted() {
        let payload = HookPayloadFields {
            metadata: HookCaptureMetadata::new(3, "run-3", "task-3", "profile-3"),
            tool_name: "/tmp/private-tool",
            command: None,
            arguments: Some("safe enough to hash"),
            result: None,
            tokens: TokenCounts::new(0, 0, 0, 0),
        };

        let captured = capture_hook_payload(&payload).expect("hook payload captures");
        assert_eq!(captured.events[0].tool, "unknown-tool.arguments");
    }

    #[test]
    fn passive_mode_state_stamps_adhoc_events_without_extra_task_command() {
        let state = ModeState::passive();
        let payload = HookPayloadFields {
            metadata: HookCaptureMetadata::from_mode_state(4, "run-passive", &state),
            tool_name: "Read",
            command: None,
            arguments: Some("src/lib.rs"),
            result: None,
            tokens: TokenCounts::new(1, 0, 0, 0),
        };

        let events = hook_events_from_payload(&payload).expect("hook payload captures");

        assert_eq!(events[0].mode, CaptureMode::Passive);
        assert_eq!(events[0].task_id, "adhoc");
        assert_eq!(events[0].profile_id, "adhoc");
    }

    fn fixture_payload() -> HookPayloadFields<'static> {
        HookPayloadFields {
            metadata: HookCaptureMetadata::new(1, "run-1", "task-1", "profile-1"),
            tool_name: "Read",
            command: None,
            arguments: Some(RAW_PATH_MARKER),
            result: Some(concat!(
                "contents ",
                "SECRET_RAW_ARGUMENT_MARKER ",
                "SECRET_RAW_RESULT_MARKER ",
                "SECRET_RAW_PROMPT_MARKER"
            )),
            tokens: TokenCounts::new(10, 20, 3, 4),
        }
    }
}
