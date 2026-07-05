use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli_report::{create_first_report_artifacts, create_report_share_artifact};

pub const DEFAULT_UPLOAD_CONFIG_FILE: &str = "upload.json";
const UPLOAD_PAYLOAD_FILE: &str = "upload.payload.json";
const DEFAULT_SHARE_SALT: &str = "upload-share";
const UPLOAD_SCHEMA_VERSION: &str = "vc-tokmeter.upload.v1";
const DEFAULT_CONSENT_VERSION: &str = "2026-07-05";
const DEFAULT_STUDY_ID: &str = "git-workflow-token-study";
const DEFAULT_PROTOCOL_VERSION: &str = "v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadPlanRequest {
    pub event_log_path: PathBuf,
    pub out_dir: PathBuf,
    pub endpoint: Option<String>,
    pub token: Option<String>,
    pub config_path: Option<PathBuf>,
    pub dry_run: bool,
    pub yes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadPlan {
    pub paths: UploadPaths,
    pub endpoint: Option<String>,
    pub token_source: UploadTokenSource,
    pub dry_run: bool,
    pub summary: UploadPayloadSummary,
    pub request: Option<UploadHttpRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadPaths {
    pub report_json: PathBuf,
    pub report_markdown: PathBuf,
    pub report_share: PathBuf,
    pub upload_payload: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadTokenSource {
    Missing,
    Flag,
    Config,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadPayloadSummary {
    pub payload_bytes: u64,
    pub schema_version: Option<String>,
    pub artifact_type: Option<String>,
    pub evidence_grade: Option<String>,
    pub total_tokens: Option<u64>,
    pub git_tokens: Option<u64>,
    pub git_token_share_basis_points: Option<u64>,
    pub fidelity: Option<String>,
    pub event_count: Option<u64>,
    pub run_count: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadHttpRequest {
    pub endpoint: String,
    pub content_type: String,
    pub authorization_header: String,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadHttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug)]
pub enum UploadError {
    Io(io::Error),
    Json(serde_json::Error),
    MissingUploadTarget { config_path: PathBuf },
    InvalidEndpoint(String),
    Http(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::MissingUploadTarget { config_path } => write!(
                formatter,
                "upload requires --endpoint and --token, or config at {}",
                config_path.display()
            ),
            Self::InvalidEndpoint(endpoint) => write!(
                formatter,
                "invalid upload endpoint {endpoint:?}; expected http:// or https://"
            ),
            Self::Http(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for UploadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::MissingUploadTarget { .. } | Self::InvalidEndpoint(_) | Self::Http(_) => None,
        }
    }
}

impl From<io::Error> for UploadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for UploadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UploadConfig {
    enabled: bool,
    endpoint: Option<String>,
    token: Option<String>,
}

pub fn prepare_upload_plan(request: &UploadPlanRequest) -> Result<UploadPlan, UploadError> {
    let config_path = request
        .config_path
        .clone()
        .unwrap_or_else(|| default_config_path(&request.event_log_path));
    let config = read_upload_config(&config_path)?;
    let config_endpoint = config
        .enabled
        .then_some(config.endpoint.as_deref())
        .flatten();
    let config_token = config.enabled.then_some(config.token.as_deref()).flatten();
    let endpoint = choose_nonempty(request.endpoint.as_deref(), config_endpoint);
    let (token, token_source) =
        match choose_nonempty_with_source(request.token.as_deref(), config_token) {
            Some((token, source)) => (Some(token), source),
            None => (None, UploadTokenSource::Missing),
        };
    let dry_run = request.dry_run;

    if let Some(endpoint) = endpoint {
        validate_endpoint(endpoint)?;
    }
    if !dry_run && (endpoint.is_none() || token.is_none()) {
        return Err(UploadError::MissingUploadTarget { config_path });
    }

    let artifacts = create_first_report_artifacts(&request.out_dir, Some(&request.event_log_path))?;
    let share_path = create_report_share_artifact(&artifacts, DEFAULT_SHARE_SALT)?;
    let report_json = fs::read_to_string(&artifacts.paths.report_json)?;
    let report_share = fs::read_to_string(&share_path)?;
    let payload = build_upload_payload(&report_json, &report_share)?;
    let payload_json = serde_json::to_vec_pretty(&payload)?;
    let upload_payload = request.out_dir.join(UPLOAD_PAYLOAD_FILE);
    fs::write(&upload_payload, &payload_json)?;
    let summary = summarize_payload(&payload, payload_json.len() as u64);
    let request = endpoint
        .zip(token)
        .map(|(endpoint, token)| UploadHttpRequest {
            endpoint: endpoint.to_owned(),
            content_type: "application/json".to_owned(),
            authorization_header: format!("Bearer {token}"),
            body: payload_json,
        });

    Ok(UploadPlan {
        paths: UploadPaths {
            report_json: artifacts.paths.report_json,
            report_markdown: artifacts.paths.report_markdown,
            report_share: share_path,
            upload_payload,
        },
        endpoint: endpoint.map(str::to_owned),
        token_source,
        dry_run,
        summary,
        request,
    })
}

pub fn send_upload(request: &UploadHttpRequest) -> Result<UploadHttpResponse, UploadError> {
    let response = reqwest::blocking::Client::new()
        .post(&request.endpoint)
        .header("content-type", &request.content_type)
        .header("authorization", &request.authorization_header)
        .body(request.body.clone())
        .send()
        .map_err(|error| UploadError::Http(format!("upload request failed: {error}")))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .map_err(|error| UploadError::Http(format!("cannot read upload response: {error}")))?;
    if !(200..300).contains(&status) {
        return Err(UploadError::Http(format!(
            "upload failed with HTTP {status}: {body}"
        )));
    }
    Ok(UploadHttpResponse { status, body })
}

pub fn render_upload_plan(plan: &UploadPlan) -> String {
    let mut out = String::new();
    out.push_str(if plan.dry_run {
        "upload mode: dry-run\n"
    } else {
        "upload mode: live\n"
    });
    out.push_str(&format!(
        "report.json: {}\n",
        plan.paths.report_json.display()
    ));
    out.push_str(&format!(
        "report.share.json: {}\n",
        plan.paths.report_share.display()
    ));
    out.push_str(&format!(
        "upload.payload.json: {}\n",
        plan.paths.upload_payload.display()
    ));
    out.push_str(&format!(
        "endpoint: {}\n",
        plan.endpoint.as_deref().unwrap_or("(missing)")
    ));
    out.push_str(&format!(
        "token: {}\n",
        render_token_source(plan.token_source)
    ));
    out.push_str(&render_payload_summary(&plan.summary));
    if plan.dry_run {
        out.push_str("dry-run: no network upload attempted; pass --yes to upload.\n");
    }
    out
}

pub fn render_upload_response(response: &UploadHttpResponse) -> String {
    format!("upload complete: status={}\n", response.status)
}

pub fn write_upload_config(path: &Path, endpoint: &str, token: &str) -> Result<(), UploadError> {
    validate_endpoint(endpoint)?;
    if token.trim().is_empty() {
        return Err(UploadError::MissingUploadTarget {
            config_path: path.to_path_buf(),
        });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(&serde_json::json!({
        "enabled": true,
        "endpoint": endpoint,
        "upload_token": token
    }))?;
    fs::write(path, content)?;
    Ok(())
}

pub fn remove_upload_config(path: &Path) -> Result<bool, UploadError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(UploadError::Io(error)),
    }
}

pub fn default_upload_config_path(event_log_path: &Path) -> PathBuf {
    default_config_path(event_log_path)
}

fn read_upload_config(path: &Path) -> Result<UploadConfig, UploadError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(UploadConfig {
                enabled: true,
                endpoint: None,
                token: None,
            });
        }
        Err(error) => return Err(UploadError::Io(error)),
    };
    let value: serde_json::Value = serde_json::from_str(&content)?;
    Ok(UploadConfig {
        enabled: value
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        endpoint: string_field(&value, "endpoint"),
        token: string_field(&value, "token").or_else(|| string_field(&value, "upload_token")),
    })
}

fn build_upload_payload(
    report_json: &str,
    report_share: &str,
) -> Result<serde_json::Value, UploadError> {
    let report: serde_json::Value = serde_json::from_str(report_json)?;
    let share: serde_json::Value = serde_json::from_str(report_share)?;
    let created_at = current_utc_timestamp();
    let session_id_hash = scoped_hex_hash(report_share, DEFAULT_SHARE_SALT, 16);
    let _share_artifact_type = share.get("artifact_type");
    Ok(serde_json::json!({
        "schema_version": UPLOAD_SCHEMA_VERSION,
        "artifact_type": "vc-tokmeter.upload",
        "created_at": created_at,
        "client": {
            "tokmeter_version": env!("CARGO_PKG_VERSION"),
            "surface": "other",
            "platform": {
                "os": platform_os(),
                "arch": platform_arch()
            }
        },
        "consent": {
            "upload_opt_in": true,
            "consent_version": DEFAULT_CONSENT_VERSION
        },
        "study": {
            "study_id": DEFAULT_STUDY_ID,
            "protocol_version": DEFAULT_PROTOCOL_VERSION
        },
        "session": {
            "session_id_hash": session_id_hash,
            "time_bucket_utc": current_utc_hour_bucket(),
            "duration_seconds": 0
        },
        "metrics": upload_metrics_value(&report),
        "redaction": {
            "source_artifact": "report-share",
            "digest_hex_chars": 12,
            "salt_scope": "client-local",
            "private_data_policy": "aggregate-only-no-raw-content"
        }
    }))
}

fn upload_metrics_value(report: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "evidence_grade": upload_evidence_grade(report),
        "token_fidelity": upload_token_fidelity(report),
        "summary": {
            "events": number_at(report, &["totals", "event_count"]),
            "runs": number_at(report, &["totals", "run_count"]),
            "tasks": number_at(report, &["totals", "task_count"]),
            "input_tokens": number_at(report, &["totals", "input_tokens"]),
            "output_tokens": number_at(report, &["totals", "output_tokens"]),
            "cache_read_tokens": number_at(report, &["totals", "cache_read_tokens"]),
            "cache_write_tokens": number_at(report, &["totals", "cache_write_tokens"]),
            "total_tokens": number_at(report, &["totals", "total_tokens"]),
            "bytes": number_at(report, &["totals", "byte_count"])
        },
        "session_git_share": {
            "total_tokens": number_at(report, &["session_git_share", "total_tokens"]),
            "git_tokens": number_at(report, &["session_git_share", "git_tokens"]),
            "non_git_tokens": number_at(report, &["session_git_share", "non_git_tokens"]),
            "git_token_share": report.pointer("/session_git_share/git_token_share").cloned().unwrap_or(serde_json::Value::Null),
            "fidelity": report.pointer("/session_git_share/fidelity").cloned().unwrap_or(serde_json::Value::Null)
        },
        "token_sources": upload_token_sources(report),
        "git_workflow": upload_git_workflow(report),
        "warnings": upload_warnings(report)
    })
}

fn summarize_payload(payload: &serde_json::Value, payload_bytes: u64) -> UploadPayloadSummary {
    UploadPayloadSummary {
        payload_bytes,
        schema_version: payload
            .get("schema_version")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        artifact_type: string_at(payload, &["artifact_type"]),
        evidence_grade: string_at(payload, &["metrics", "evidence_grade"]),
        total_tokens: u64_at(payload, &["metrics", "summary", "total_tokens"]),
        git_tokens: u64_at(payload, &["metrics", "session_git_share", "git_tokens"]),
        git_token_share_basis_points: f64_at(
            payload,
            &["metrics", "session_git_share", "git_token_share"],
        )
        .map(|share| (share * 10_000.0).round().max(0.0) as u64),
        fidelity: string_at(payload, &["metrics", "session_git_share", "fidelity"]),
        event_count: u64_at(payload, &["metrics", "summary", "events"]),
        run_count: u64_at(payload, &["metrics", "summary", "runs"]),
    }
}

fn render_payload_summary(summary: &UploadPayloadSummary) -> String {
    format!(
        "payload: artifact={} schema={} bytes={} total_tokens={} git_tokens={} git_share={} fidelity={} events={} runs={}\n",
        summary.artifact_type.as_deref().unwrap_or("unknown"),
        summary
            .schema_version
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| "unknown".to_owned()),
        summary.payload_bytes,
        render_optional_u64(summary.total_tokens),
        render_optional_u64(summary.git_tokens),
        render_basis_points(summary.git_token_share_basis_points),
        summary.fidelity.as_deref().unwrap_or("unknown"),
        render_optional_u64(summary.event_count),
        render_optional_u64(summary.run_count),
    )
}

fn render_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn render_basis_points(value: Option<u64>) -> String {
    value
        .map(|basis_points| format!("{:.2}%", basis_points as f64 / 100.0))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn render_token_source(source: UploadTokenSource) -> &'static str {
    match source {
        UploadTokenSource::Missing => "(missing)",
        UploadTokenSource::Flag => "provided by flag",
        UploadTokenSource::Config => "provided by config",
    }
}

fn validate_endpoint(endpoint: &str) -> Result<(), UploadError> {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        Ok(())
    } else {
        Err(UploadError::InvalidEndpoint(endpoint.to_owned()))
    }
}

fn choose_nonempty<'a>(flag: Option<&'a str>, config: Option<&'a str>) -> Option<&'a str> {
    flag.filter(|value| !value.trim().is_empty())
        .or_else(|| config.filter(|value| !value.trim().is_empty()))
}

fn choose_nonempty_with_source<'a>(
    flag: Option<&'a str>,
    config: Option<&'a str>,
) -> Option<(&'a str, UploadTokenSource)> {
    if let Some(value) = flag.filter(|value| !value.trim().is_empty()) {
        return Some((value, UploadTokenSource::Flag));
    }
    config
        .filter(|value| !value.trim().is_empty())
        .map(|value| (value, UploadTokenSource::Config))
}

fn default_config_path(event_log_path: &Path) -> PathBuf {
    event_log_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(DEFAULT_UPLOAD_CONFIG_FILE)
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn number_at(value: &serde_json::Value, path: &[&str]) -> serde_json::Value {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn upload_evidence_grade(report: &serde_json::Value) -> &'static str {
    match string_at(report, &["evidence_grade"]).as_deref() {
        Some("Grade P") => "P",
        _ => "O",
    }
}

fn upload_token_fidelity(report: &serde_json::Value) -> String {
    normalize_token_fidelity(
        report
            .pointer("/session_git_share/fidelity")
            .and_then(serde_json::Value::as_str),
    )
    .to_owned()
}

fn upload_token_sources(report: &serde_json::Value) -> serde_json::Value {
    let rows = report
        .get("token_sources")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    serde_json::json!({
                        "source": normalize_token_source(row.get("source").and_then(serde_json::Value::as_str)),
                        "events": number_at(row, &["totals", "event_count"]),
                        "total_tokens": number_at(row, &["totals", "total_tokens"]),
                        "input_tokens": number_at(row, &["totals", "input_tokens"]),
                        "output_tokens": number_at(row, &["totals", "output_tokens"]),
                        "cache_read_tokens": number_at(row, &["totals", "cache_read_tokens"]),
                        "cache_write_tokens": number_at(row, &["totals", "cache_write_tokens"]),
                        "bytes": number_at(row, &["totals", "byte_count"]),
                        "token_share": row.get("token_share").cloned().unwrap_or(serde_json::Value::Null)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::Value::Array(rows)
}

fn upload_git_workflow(report: &serde_json::Value) -> serde_json::Value {
    let rows = report
        .pointer("/git_workflow/rows")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    serde_json::json!({
                        "action_subtype": normalize_action_subtype(row.get("action_subtype").and_then(serde_json::Value::as_str)),
                        "direction": normalize_direction(row.get("direction").and_then(serde_json::Value::as_str)),
                        "operation_class": normalize_operation_class(row.get("operation_class").and_then(serde_json::Value::as_str)),
                        "events": number_at(row, &["totals", "event_count"]),
                        "total_tokens": number_at(row, &["totals", "total_tokens"]),
                        "input_tokens": number_at(row, &["totals", "input_tokens"]),
                        "output_tokens": number_at(row, &["totals", "output_tokens"]),
                        "cache_read_tokens": number_at(row, &["totals", "cache_read_tokens"]),
                        "cache_write_tokens": number_at(row, &["totals", "cache_write_tokens"]),
                        "bytes": number_at(row, &["totals", "byte_count"]),
                        "token_share": row.get("token_share").cloned().unwrap_or(serde_json::Value::Null)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::json!({
        "totals": upload_summary_totals(report.pointer("/git_workflow/totals")),
        "action_subtypes": rows
    })
}

fn upload_summary_totals(value: Option<&serde_json::Value>) -> serde_json::Value {
    let totals = value.unwrap_or(&serde_json::Value::Null);
    serde_json::json!({
        "events": number_at(totals, &["event_count"]),
        "runs": number_at(totals, &["run_count"]),
        "tasks": number_at(totals, &["task_count"]),
        "input_tokens": number_at(totals, &["input_tokens"]),
        "output_tokens": number_at(totals, &["output_tokens"]),
        "cache_read_tokens": number_at(totals, &["cache_read_tokens"]),
        "cache_write_tokens": number_at(totals, &["cache_write_tokens"]),
        "total_tokens": number_at(totals, &["total_tokens"]),
        "bytes": number_at(totals, &["byte_count"])
    })
}

fn upload_warnings(report: &serde_json::Value) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    if upload_evidence_grade(report) == "O" {
        warnings.push("observational_only");
    }
    if upload_token_fidelity(report) != "exact" {
        warnings.push("mixed_fidelity");
    }
    warnings
}

fn normalize_token_source(value: Option<&str>) -> &'static str {
    match value.unwrap_or("other") {
        "codex exec exact usage" => "codex exec exact usage",
        "proxy exact usage" => "proxy exact usage",
        "proxy estimate" | "proxy estimate request" | "proxy estimate response" => "proxy estimate",
        "mcp tool" => "mcp tool",
        "mcp tool request" => "mcp tool request",
        "mcp tool response" => "mcp tool response",
        "hook" => "hook",
        "hook request" => "hook request",
        "hook response" => "hook response",
        _ => "other",
    }
}

fn normalize_token_fidelity(value: Option<&str>) -> &'static str {
    match value.unwrap_or("unknown") {
        "exact" => "exact",
        "estimated" => "estimated",
        "mixed" => "mixed",
        _ => "unknown",
    }
}

fn normalize_action_subtype(value: Option<&str>) -> &'static str {
    match value.unwrap_or("vc.other") {
        "git.status" | "status" | "vc.status" => "git.status",
        "git.diff" | "diff" | "vc.diff" => "git.diff",
        "git.log" | "log" | "vc.log" => "git.log",
        "git.show" | "show" | "vc.show" => "git.show",
        "git.branch" | "branch" | "vc.branch_ops" => "git.branch",
        "git.commit" | "commit" => "git.commit",
        _ => "git.other",
    }
}

fn normalize_direction(value: Option<&str>) -> &'static str {
    match value.unwrap_or("unknown") {
        "request" => "request",
        "response" => "response",
        "summary" => "summary",
        _ => "unknown",
    }
}

fn normalize_operation_class(value: Option<&str>) -> &'static str {
    let value = value.unwrap_or("other");
    if value.starts_with("vc.") {
        "version_control"
    } else if value.starts_with("file.") || value.starts_with("edit.") {
        "file_interaction"
    } else if value == "session.meta" {
        "generated_context"
    } else {
        "other"
    }
}

fn current_utc_timestamp() -> String {
    let seconds = unix_seconds_now();
    format_utc_timestamp(seconds)
}

fn current_utc_hour_bucket() -> String {
    let seconds = unix_seconds_now();
    format_utc_timestamp(seconds - seconds % 3600)
}

fn unix_seconds_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn format_utc_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u64, d as u64)
}

fn scoped_hex_hash(value: &str, salt: &str, chars: usize) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in salt.bytes().chain(value.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
        .chars()
        .take(chars)
        .collect::<String>()
}

fn platform_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        _ => "unknown",
    }
}

fn platform_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => "unknown",
    }
}

fn string_at(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn u64_at(value: &serde_json::Value, path: &[&str]) -> Option<u64> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(serde_json::Value::as_u64)
}

fn f64_at(value: &serde_json::Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(serde_json::Value::as_f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AttributedTokenEvent, CaptureMode, EventLog, OperationClass, TokenCounts};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_writes_redacted_payload_without_upload_request() {
        let temp = temp_dir("dry-run");
        let event_log = temp.join(".tokmeter").join("events.jsonl");
        fs::create_dir_all(event_log.parent().unwrap()).unwrap();
        write_event_log_fixture(&event_log);

        let plan = prepare_upload_plan(&UploadPlanRequest {
            event_log_path: event_log.clone(),
            out_dir: temp.join("report"),
            endpoint: None,
            token: None,
            config_path: None,
            dry_run: true,
            yes: false,
        })
        .unwrap();

        assert!(plan.dry_run);
        assert!(plan.request.is_none());
        assert!(plan.paths.upload_payload.exists());
        assert_eq!(plan.summary.total_tokens, Some(12));
        assert_eq!(plan.summary.git_tokens, Some(12));

        let payload = fs::read_to_string(&plan.paths.upload_payload).unwrap();
        assert!(payload.contains("\"artifact_type\": \"vc-tokmeter.upload\""));
        assert!(payload.contains("\"schema_version\": \"vc-tokmeter.upload.v1\""));
        assert!(payload.contains("\"metrics\""));
        assert!(payload.contains("\"redaction\""));
        assert!(payload.contains("\"total_tokens\": 12"));
        assert!(!payload.contains(event_log.to_string_lossy().as_ref()));
        assert!(!payload.contains("events.jsonl"));
        assert!(!payload.contains("\"raw_events\""));
        assert!(!payload.contains("SECRET_SOURCE_MARKER"));
    }

    #[test]
    fn upload_builds_request_with_flag_endpoint_and_token() {
        let temp = temp_dir("yes-request");
        let event_log = temp.join(".tokmeter").join("events.jsonl");
        fs::create_dir_all(event_log.parent().unwrap()).unwrap();
        write_event_log_fixture(&event_log);

        let plan = prepare_upload_plan(&UploadPlanRequest {
            event_log_path: event_log,
            out_dir: temp.join("report"),
            endpoint: Some("https://collector.example.test/v1/uploads".to_owned()),
            token: Some("upload-secret-token".to_owned()),
            config_path: None,
            dry_run: false,
            yes: false,
        })
        .unwrap();

        let request = plan.request.unwrap();
        assert_eq!(
            request.endpoint,
            "https://collector.example.test/v1/uploads"
        );
        assert_eq!(request.content_type, "application/json");
        assert_eq!(request.authorization_header, "Bearer upload-secret-token");
        assert!(
            String::from_utf8(request.body)
                .unwrap()
                .contains("vc-tokmeter.upload")
        );
    }

    #[test]
    fn non_dry_run_requires_endpoint_and_token() {
        let temp = temp_dir("missing-target");
        let event_log = temp.join(".tokmeter").join("events.jsonl");
        fs::create_dir_all(event_log.parent().unwrap()).unwrap();
        write_event_log_fixture(&event_log);

        let error = prepare_upload_plan(&UploadPlanRequest {
            event_log_path: event_log,
            out_dir: temp.join("report"),
            endpoint: None,
            token: None,
            config_path: None,
            dry_run: false,
            yes: true,
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("upload requires --endpoint and --token")
        );
    }

    #[test]
    fn config_file_can_supply_endpoint_and_token() {
        let temp = temp_dir("config");
        let event_log = temp.join(".tokmeter").join("events.jsonl");
        let config = temp.join(".tokmeter").join("upload.json");
        fs::create_dir_all(event_log.parent().unwrap()).unwrap();
        write_event_log_fixture(&event_log);
        fs::write(
            &config,
            r#"{"endpoint":"https://collector.example.test/upload","upload_token":"from-config"}"#,
        )
        .unwrap();

        let plan = prepare_upload_plan(&UploadPlanRequest {
            event_log_path: event_log,
            out_dir: temp.join("report"),
            endpoint: None,
            token: None,
            config_path: None,
            dry_run: false,
            yes: true,
        })
        .unwrap();

        assert_eq!(plan.token_source, UploadTokenSource::Config);
        assert_eq!(
            plan.request.as_ref().unwrap().endpoint,
            "https://collector.example.test/upload"
        );
        assert_eq!(
            plan.request.unwrap().authorization_header,
            "Bearer from-config"
        );
    }

    #[test]
    fn write_upload_config_persists_opt_in_credentials_without_rendering_token() {
        let temp = temp_dir("write-config");
        let event_log = temp.join(".tokmeter").join("events.jsonl");
        let config = default_upload_config_path(&event_log);
        fs::create_dir_all(event_log.parent().unwrap()).unwrap();
        write_event_log_fixture(&event_log);

        write_upload_config(
            &config,
            "https://collector.example.test/upload",
            "setup-secret-token",
        )
        .unwrap();

        let plan = prepare_upload_plan(&UploadPlanRequest {
            event_log_path: event_log,
            out_dir: temp.join("report"),
            endpoint: None,
            token: None,
            config_path: None,
            dry_run: true,
            yes: false,
        })
        .unwrap();
        let rendered = render_upload_plan(&plan);

        assert_eq!(plan.token_source, UploadTokenSource::Config);
        assert_eq!(
            plan.request.as_ref().unwrap().authorization_header,
            "Bearer setup-secret-token"
        );
        assert!(rendered.contains("token: provided by config"));
        assert!(!rendered.contains("setup-secret-token"));
    }

    #[test]
    fn disabled_upload_config_is_ignored() {
        let temp = temp_dir("disabled-config");
        let event_log = temp.join(".tokmeter").join("events.jsonl");
        let config = temp.join(".tokmeter").join("upload.json");
        fs::create_dir_all(event_log.parent().unwrap()).unwrap();
        write_event_log_fixture(&event_log);
        fs::write(
            &config,
            r#"{"enabled":false,"endpoint":"https://collector.example.test/upload","upload_token":"disabled-secret"}"#,
        )
        .unwrap();

        let plan = prepare_upload_plan(&UploadPlanRequest {
            event_log_path: event_log,
            out_dir: temp.join("report"),
            endpoint: None,
            token: None,
            config_path: None,
            dry_run: true,
            yes: false,
        })
        .unwrap();

        assert_eq!(plan.token_source, UploadTokenSource::Missing);
        assert!(plan.request.is_none());
    }

    #[test]
    fn remove_upload_config_deletes_saved_credentials() {
        let temp = temp_dir("remove-config");
        let config = temp.join(".tokmeter").join("upload.json");
        write_upload_config(
            &config,
            "https://collector.example.test/upload",
            "remove-secret-token",
        )
        .unwrap();

        assert!(remove_upload_config(&config).unwrap());
        assert!(!config.exists());
        assert!(!remove_upload_config(&config).unwrap());
    }

    #[test]
    fn render_does_not_print_token_value() {
        let plan = UploadPlan {
            paths: UploadPaths {
                report_json: PathBuf::from("report.json"),
                report_markdown: PathBuf::from("report.md"),
                report_share: PathBuf::from("report.share.json"),
                upload_payload: PathBuf::from("upload.payload.json"),
            },
            endpoint: Some("https://collector.example.test/upload".to_owned()),
            token_source: UploadTokenSource::Flag,
            dry_run: true,
            summary: UploadPayloadSummary {
                payload_bytes: 123,
                schema_version: Some("vc-tokmeter.upload.v1".to_owned()),
                artifact_type: Some("vc-tokmeter.upload".to_owned()),
                evidence_grade: Some("Grade O".to_owned()),
                total_tokens: Some(100),
                git_tokens: Some(5),
                git_token_share_basis_points: Some(50),
                fidelity: Some("estimated".to_owned()),
                event_count: Some(2),
                run_count: Some(1),
            },
            request: Some(UploadHttpRequest {
                endpoint: "https://collector.example.test/upload".to_owned(),
                content_type: "application/json".to_owned(),
                authorization_header: "Bearer secret-token".to_owned(),
                body: vec![],
            }),
        };

        let rendered = render_upload_plan(&plan);

        assert!(rendered.contains("token: provided by flag"));
        assert!(!rendered.contains("secret-token"));
    }

    fn write_event_log_fixture(path: &Path) {
        let event = AttributedTokenEvent {
            timestamp_ms: 1,
            mode: CaptureMode::Passive,
            run_id: "run-1".to_owned(),
            task_id: "adhoc".to_owned(),
            profile_id: "adhoc".to_owned(),
            adapter: "hook".to_owned(),
            operation_class: OperationClass::VcStatus,
            tool: "Bash".to_owned(),
            tokens: TokenCounts::new(7, 5, 0, 0),
            byte_count: 64,
            content_digest: "digest-secret-source-marker".to_owned(),
            repeat_of: None,
            action_subtype: Some("status".to_owned()),
            direction: Some("response".to_owned()),
        };
        EventLog::append_event_to_file(path, &event).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vc-tokmeter-upload-{name}-{nanos}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
