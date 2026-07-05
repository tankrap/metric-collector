use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::classifier::{
    GIT_ACTION_BRANCH, GIT_ACTION_DIFF, GIT_ACTION_LOG, GIT_ACTION_SHOW, GIT_ACTION_STATUS,
};
use crate::core::{
    AttributedTokenEvent, CaptureMode, EventLog, EventLogError, OperationClass, TokenCounts,
};
use crate::digest::digest_bytes;
use crate::token_estimator::{estimate_tool_result_tokens, estimate_universal_payload_tokens};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const ADAPTER: &str = "mcp.git";
const PROFILE_ID: &str = "adhoc";
const TASK_ID: &str = "adhoc";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpGitConfig {
    pub event_log_path: PathBuf,
    pub workdir: PathBuf,
    pub run_id: String,
}

impl McpGitConfig {
    pub fn new(event_log_path: impl Into<PathBuf>, workdir: impl Into<PathBuf>) -> Self {
        Self {
            event_log_path: event_log_path.into(),
            workdir: workdir.into(),
            run_id: format!("mcp-git-{}", unix_time_ms()),
        }
    }
}

#[derive(Debug)]
pub enum McpGitError {
    Io(io::Error),
    Json(serde_json::Error),
    EventLog(EventLogError),
}

impl std::fmt::Display for McpGitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::EventLog(error) => write!(formatter, "event log error: {error}"),
        }
    }
}

impl std::error::Error for McpGitError {}

impl From<io::Error> for McpGitError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for McpGitError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<EventLogError> for McpGitError {
    fn from(error: EventLogError) -> Self {
        Self::EventLog(error)
    }
}

pub fn run_mcp_git_server<R, W>(
    reader: R,
    mut writer: W,
    config: McpGitConfig,
) -> Result<(), McpGitError>
where
    R: BufRead,
    W: Write,
{
    let mut state = McpGitState::new(config);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_mcp_message(&line, &mut state)? {
            writeln!(writer, "{response}")?;
            writer.flush()?;
        }
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct McpGitState {
    config: McpGitConfig,
}

impl McpGitState {
    fn new(config: McpGitConfig) -> Self {
        Self { config }
    }
}

fn handle_mcp_message(line: &str, state: &mut McpGitState) -> Result<Option<String>, McpGitError> {
    let request: Value = serde_json::from_str(line)?;
    let Some(id) = request.get("id").cloned() else {
        return Ok(None);
    };
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let response = match method {
        "initialize" => success(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "vc-tokmeter-git",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
        "tools/list" => success(id, json!({ "tools": tool_definitions() })),
        "tools/call" => match call_tool(&request, state) {
            Ok(result) => success(id, result),
            Err(message) => success(
                id,
                json!({
                    "content": [{ "type": "text", "text": message }],
                    "isError": true
                }),
            ),
        },
        _ => error(id, -32601, "method not found"),
    };

    Ok(Some(response.to_string()))
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "tokmeter_git_status",
            "title": "Git status",
            "description": "Run git status --short and log privacy-safe tokmeter metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "tokmeter_git_diff",
            "title": "Git diff",
            "description": "Run git diff, optionally for one path, and log privacy-safe tokmeter metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional repository-relative path." },
                    "staged": { "type": "boolean", "description": "Use git diff --cached." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "tokmeter_git_log",
            "title": "Git log",
            "description": "Run git log --oneline and log privacy-safe tokmeter metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "tokmeter_git_show",
            "title": "Git show",
            "description": "Run git show --stat for a revision and log privacy-safe tokmeter metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "revision": { "type": "string", "description": "Revision, default HEAD." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "tokmeter_git_branch",
            "title": "Git branch",
            "description": "Run git branch --show-current and log privacy-safe tokmeter metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }
    ])
}

fn call_tool(request: &Value, state: &mut McpGitState) -> Result<Value, String> {
    let params = request.get("params").unwrap_or(&Value::Null);
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing tool name".to_owned())?;
    let args = params.get("arguments").unwrap_or(&Value::Null);
    let spec = GitToolSpec::from_call(name, args)?;
    let output = run_git_tool(&state.config.workdir, &spec)?;

    persist_tool_events(&state.config, &spec, &output).map_err(|error| error.to_string())?;

    Ok(json!({
        "content": [{ "type": "text", "text": output }],
        "isError": false
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GitToolSpec {
    tool_name: &'static str,
    action_subtype: &'static str,
    operation_class: OperationClass,
    args: Vec<String>,
}

impl GitToolSpec {
    fn from_call(name: &str, arguments: &Value) -> Result<Self, String> {
        match name {
            "tokmeter_git_status" => Ok(Self {
                tool_name: "tokmeter_git_status",
                action_subtype: GIT_ACTION_STATUS,
                operation_class: OperationClass::VcStatus,
                args: vec!["status".to_owned(), "--short".to_owned()],
            }),
            "tokmeter_git_diff" => {
                let mut args = vec!["diff".to_owned()];
                if bool_arg(arguments, "staged") {
                    args.push("--cached".to_owned());
                }
                if let Some(path) = optional_path_arg(arguments, "path")? {
                    args.push("--".to_owned());
                    args.push(path);
                }
                Ok(Self {
                    tool_name: "tokmeter_git_diff",
                    action_subtype: GIT_ACTION_DIFF,
                    operation_class: OperationClass::VcDiff,
                    args,
                })
            }
            "tokmeter_git_log" => {
                let limit = integer_arg(arguments, "limit").unwrap_or(10).clamp(1, 50);
                Ok(Self {
                    tool_name: "tokmeter_git_log",
                    action_subtype: GIT_ACTION_LOG,
                    operation_class: OperationClass::VcLog,
                    args: vec![
                        "log".to_owned(),
                        "--oneline".to_owned(),
                        "-n".to_owned(),
                        limit.to_string(),
                    ],
                })
            }
            "tokmeter_git_show" => {
                let revision = optional_revision_arg(arguments, "revision")?
                    .unwrap_or_else(|| "HEAD".to_owned());
                Ok(Self {
                    tool_name: "tokmeter_git_show",
                    action_subtype: GIT_ACTION_SHOW,
                    operation_class: OperationClass::VcShow,
                    args: vec![
                        "show".to_owned(),
                        "--stat".to_owned(),
                        "--oneline".to_owned(),
                        "--no-renames".to_owned(),
                        revision,
                    ],
                })
            }
            "tokmeter_git_branch" => Ok(Self {
                tool_name: "tokmeter_git_branch",
                action_subtype: GIT_ACTION_BRANCH,
                operation_class: OperationClass::VcBranchOps,
                args: vec!["branch".to_owned(), "--show-current".to_owned()],
            }),
            _ => Err(format!("unknown tokmeter git tool: {name}")),
        }
    }

    fn normalized_request_label(&self) -> String {
        format!("mcp.git {}", self.action_subtype)
    }
}

fn run_git_tool(workdir: &Path, spec: &GitToolSpec) -> Result<String, String> {
    let output = Command::new("git")
        .args(&spec.args)
        .current_dir(workdir)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stderr.trim().is_empty() {
        stdout.into_owned()
    } else if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        format!("{stdout}\n{stderr}")
    };

    if output.status.success() {
        Ok(combined)
    } else {
        Err(if combined.trim().is_empty() {
            "git command failed without output".to_owned()
        } else {
            combined
        })
    }
}

fn persist_tool_events(
    config: &McpGitConfig,
    spec: &GitToolSpec,
    output: &str,
) -> Result<(), EventLogError> {
    if let Some(parent) = config.event_log_path.parent() {
        fs::create_dir_all(parent).map_err(EventLogError::Io)?;
    }

    let request_label = spec.normalized_request_label();
    let request_tokens = estimate_universal_payload_tokens(&request_label);
    let response_estimate = estimate_tool_result_tokens(output);
    let timestamp = unix_time_ms();

    EventLog::append_event_to_file(
        &config.event_log_path,
        &AttributedTokenEvent {
            timestamp_ms: timestamp,
            mode: CaptureMode::Passive,
            run_id: config.run_id.clone(),
            task_id: TASK_ID.to_owned(),
            profile_id: PROFILE_ID.to_owned(),
            adapter: ADAPTER.to_owned(),
            operation_class: spec.operation_class,
            tool: spec.tool_name.to_owned(),
            tokens: TokenCounts::new(request_tokens, 0, 0, 0),
            byte_count: request_label.len() as u64,
            content_digest: digest_bytes(request_label.as_bytes()),
            repeat_of: None,
            action_subtype: Some(spec.action_subtype.to_owned()),
            direction: Some("request".to_owned()),
        },
    )?;
    EventLog::append_event_to_file(
        &config.event_log_path,
        &AttributedTokenEvent {
            timestamp_ms: timestamp.saturating_add(1),
            mode: CaptureMode::Passive,
            run_id: config.run_id.clone(),
            task_id: TASK_ID.to_owned(),
            profile_id: PROFILE_ID.to_owned(),
            adapter: ADAPTER.to_owned(),
            operation_class: spec.operation_class,
            tool: spec.tool_name.to_owned(),
            tokens: TokenCounts::new(0, response_estimate.tokens.output_tokens, 0, 0),
            byte_count: output.len() as u64,
            content_digest: digest_bytes(output.as_bytes()),
            repeat_of: None,
            action_subtype: Some(spec.action_subtype.to_owned()),
            direction: Some("response".to_owned()),
        },
    )
}

fn bool_arg(arguments: &Value, key: &str) -> bool {
    arguments.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn integer_arg(arguments: &Value, key: &str) -> Option<i64> {
    arguments.get(key).and_then(Value::as_i64)
}

fn optional_path_arg(arguments: &Value, key: &str) -> Result<Option<String>, String> {
    let Some(value) = arguments.get(key).and_then(Value::as_str) else {
        return Ok(None);
    };
    validate_safe_arg("path", value)?;
    Ok(Some(value.to_owned()))
}

fn optional_revision_arg(arguments: &Value, key: &str) -> Result<Option<String>, String> {
    let Some(value) = arguments.get(key).and_then(Value::as_str) else {
        return Ok(None);
    };
    validate_safe_arg("revision", value)?;
    Ok(Some(value.to_owned()))
}

fn validate_safe_arg(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.starts_with('-') {
        return Err(format!("{field} must not start with '-'"));
    }
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err(format!("{field} contains unsupported control characters"));
    }
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn initializes_and_lists_git_tools() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            "\n"
        );
        let mut output = Vec::new();
        let config = McpGitConfig {
            event_log_path: PathBuf::from("/tmp/vc-tokmeter-mcp-test/events.jsonl"),
            workdir: PathBuf::from("/tmp"),
            run_id: "test-run".to_owned(),
        };

        run_mcp_git_server(Cursor::new(input), &mut output, config).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("\"protocolVersion\":\"2025-06-18\""));
        assert!(output.contains("\"tools\""));
        assert!(output.contains("tokmeter_git_status"));
        assert!(output.contains("tokmeter_git_diff"));
    }

    #[test]
    fn rejects_unsafe_path_arguments() {
        let args = json!({ "path": "--help" });
        let err = GitToolSpec::from_call("tokmeter_git_diff", &args).unwrap_err();

        assert!(err.contains("must not start"));
    }

    #[test]
    fn status_tool_call_logs_privacy_safe_events() {
        let workdir = temp_dir("mcp-git-work");
        run_git(&workdir, ["init"]);
        fs::write(workdir.join("tracked.txt"), "tracked\n").unwrap();
        run_git(&workdir, ["add", "tracked.txt"]);
        run_git(&workdir, ["commit", "-m", "initial"]);
        fs::write(workdir.join("tracked.txt"), "changed\n").unwrap();
        let event_log = workdir.join(".tokmeter/events.jsonl");
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tokmeter_git_status","arguments":{}}}"#,
            "\n"
        );
        let mut output = Vec::new();
        let config = McpGitConfig {
            event_log_path: event_log.clone(),
            workdir,
            run_id: "test-run".to_owned(),
        };

        run_mcp_git_server(Cursor::new(input), &mut output, config).unwrap();
        let output = String::from_utf8(output).unwrap();
        let persisted = fs::read_to_string(event_log).unwrap();

        assert!(output.contains("tracked.txt"));
        assert!(persisted.contains("adapter=mcp.git"));
        assert!(persisted.contains("op_class=vc.status"));
        assert!(persisted.contains("action_subtype=git.status"));
        assert!(persisted.contains("direction=request"));
        assert!(persisted.contains("direction=response"));
        assert!(!persisted.contains("tracked.txt"));
        assert!(!persisted.contains("changed"));
    }

    #[test]
    fn diff_tool_call_logs_git_diff_events() {
        let workdir = temp_dir("mcp-git-diff-work");
        run_git(&workdir, ["init"]);
        fs::write(workdir.join("tracked.txt"), "tracked\n").unwrap();
        run_git(&workdir, ["add", "tracked.txt"]);
        run_git(&workdir, ["commit", "-m", "initial"]);
        fs::write(workdir.join("tracked.txt"), "changed\n").unwrap();
        let event_log = workdir.join(".tokmeter/events.jsonl");
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tokmeter_git_diff","arguments":{"path":"tracked.txt"}}}"#,
            "\n"
        );
        let mut output = Vec::new();
        let config = McpGitConfig {
            event_log_path: event_log.clone(),
            workdir,
            run_id: "test-run".to_owned(),
        };

        run_mcp_git_server(Cursor::new(input), &mut output, config).unwrap();
        let output = String::from_utf8(output).unwrap();
        let persisted = fs::read_to_string(event_log).unwrap();

        assert!(output.contains("diff --git"));
        assert!(persisted.contains("adapter=mcp.git"));
        assert!(persisted.contains("op_class=vc.diff"));
        assert!(persisted.contains("action_subtype=git.diff"));
        assert!(persisted.contains("direction=request"));
        assert!(persisted.contains("direction=response"));
        assert!(!persisted.contains("tracked.txt"));
        assert!(!persisted.contains("changed"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", unix_time_ms()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn run_git<const N: usize>(workdir: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(workdir)
            .env("GIT_AUTHOR_NAME", "Tokmeter Test")
            .env("GIT_AUTHOR_EMAIL", "tokmeter@example.test")
            .env("GIT_COMMITTER_NAME", "Tokmeter Test")
            .env("GIT_COMMITTER_EMAIL", "tokmeter@example.test")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
