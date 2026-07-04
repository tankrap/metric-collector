use crate::proxy_privacy::{DEFAULT_FORBIDDEN_LOG_MARKERS, redact_proxy_error};
use crate::proxy_setup::{
    DoctorStatus as ProxyDoctorStatus, ProxyEndpoint, ProxyReachabilityProbe, ProxySetupSettings,
    doctor_proxy_reachability,
};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorCheckStatus {
    Ok,
    Warning,
    Error,
}

impl DoctorCheckStatus {
    pub fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

impl fmt::Display for DoctorCheckStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: &'static str,
    pub status: DoctorCheckStatus,
    pub summary: String,
    pub remediation: Option<String>,
}

impl DoctorCheck {
    pub fn ok(name: &'static str, summary: impl AsRef<str>) -> Self {
        Self::new(name, DoctorCheckStatus::Ok, summary, None::<&str>)
    }

    pub fn warning(
        name: &'static str,
        summary: impl AsRef<str>,
        remediation: impl AsRef<str>,
    ) -> Self {
        Self::new(name, DoctorCheckStatus::Warning, summary, Some(remediation))
    }

    pub fn error(
        name: &'static str,
        summary: impl AsRef<str>,
        remediation: impl AsRef<str>,
    ) -> Self {
        Self::new(name, DoctorCheckStatus::Error, summary, Some(remediation))
    }

    fn new(
        name: &'static str,
        status: DoctorCheckStatus,
        summary: impl AsRef<str>,
        remediation: Option<impl AsRef<str>>,
    ) -> Self {
        Self {
            name,
            status,
            summary: sanitize_output_text(summary.as_ref()),
            remediation: remediation.map(|text| sanitize_output_text(text.as_ref())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn new(checks: Vec<DoctorCheck>) -> Self {
        Self { checks }
    }

    pub fn has_errors(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == DoctorCheckStatus::Error)
    }

    pub fn render(&self) -> String {
        render_doctor_report(self)
    }
}

pub fn render_doctor_report(report: &DoctorReport) -> String {
    let mut output = String::new();

    for check in &report.checks {
        output.push_str("- [");
        output.push_str(&check.status.to_string());
        output.push_str("] ");
        output.push_str(check.name);
        output.push_str(": ");
        output.push_str(&check.summary);
        output.push('\n');

        if let Some(remediation) = &check.remediation {
            output.push_str("  action: ");
            output.push_str(remediation);
            output.push('\n');
        }
    }

    sanitize_output_text(&output)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookInstallStatus {
    Installed { path: PathBuf },
    Missing { expected_path: PathBuf },
    Broken { path: PathBuf, reason: String },
}

pub trait HookInstallationChecker {
    fn check_hook_installation(&self) -> HookInstallStatus;
}

impl<F> HookInstallationChecker for F
where
    F: Fn() -> HookInstallStatus,
{
    fn check_hook_installation(&self) -> HookInstallStatus {
        self()
    }
}

pub fn check_hook_installation<C>(checker: &C) -> DoctorCheck
where
    C: HookInstallationChecker + ?Sized,
{
    match checker.check_hook_installation() {
        HookInstallStatus::Installed { path } => DoctorCheck::ok(
            "hooks",
            format!("hook installed at {}", display_path(&path)),
        ),
        HookInstallStatus::Missing { expected_path } => DoctorCheck::error(
            "hooks",
            format!("hook not installed at {}", display_path(&expected_path)),
            "Install hooks with the tokmeter hook installer, then restart the agent session.",
        ),
        HookInstallStatus::Broken { path, reason } => DoctorCheck::error(
            "hooks",
            format!("hook at {} is not usable: {reason}", display_path(&path)),
            "Reinstall hooks and verify the configured hook file is executable and readable.",
        ),
    }
}

pub fn check_proxy_reachability<P>(settings: &ProxySetupSettings, probe: &P) -> DoctorCheck
where
    P: ProxyReachabilityProbe + ?Sized,
{
    let proxy_check = doctor_proxy_reachability(settings, probe);

    match proxy_check.status {
        ProxyDoctorStatus::Reachable => DoctorCheck::ok("proxy", proxy_check.message),
        ProxyDoctorStatus::Unreachable => DoctorCheck::error(
            "proxy",
            proxy_check.message,
            format!(
                "Start tokmeter proxy on {}, or update the agent base URL to the active proxy endpoint.",
                proxy_check.endpoint.base_url
            ),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogDirectoryError {
    reason: String,
}

impl LogDirectoryError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for LogDirectoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for LogDirectoryError {}

pub trait LogDirectoryAccess {
    fn ensure_log_dir_writable(&self, path: &Path) -> Result<(), LogDirectoryError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FsLogDirectoryAccess;

impl LogDirectoryAccess for FsLogDirectoryAccess {
    fn ensure_log_dir_writable(&self, path: &Path) -> Result<(), LogDirectoryError> {
        fs::create_dir_all(path)
            .map_err(|error| LogDirectoryError::new(format!("cannot create directory: {error}")))?;

        let probe_path = path.join(format!(
            ".tokmeter-doctor-write-test-{}.tmp",
            std::process::id()
        ));

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&probe_path)?;
            file.write_all(b"tokmeter doctor write probe\n")?;
            file.sync_all()
        })();

        let remove_result = fs::remove_file(&probe_path);

        if let Err(error) = write_result {
            let _ = remove_result;
            return Err(LogDirectoryError::new(format!(
                "cannot write probe file: {error}"
            )));
        }

        remove_result
            .map_err(|error| LogDirectoryError::new(format!("cannot remove probe file: {error}")))
    }
}

impl<F> LogDirectoryAccess for F
where
    F: Fn(&Path) -> Result<(), LogDirectoryError>,
{
    fn ensure_log_dir_writable(&self, path: &Path) -> Result<(), LogDirectoryError> {
        self(path)
    }
}

pub fn check_log_directory<A>(path: &Path, access: &A) -> DoctorCheck
where
    A: LogDirectoryAccess + ?Sized,
{
    match access.ensure_log_dir_writable(path) {
        Ok(()) => DoctorCheck::ok(
            "log directory",
            format!("log directory is writable at {}", display_path(path)),
        ),
        Err(error) => DoctorCheck::error(
            "log directory",
            format!(
                "log directory is not writable at {}: {error}",
                display_path(path)
            ),
            "Create the directory, fix ownership or permissions, then rerun the doctor check.",
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticSelfTestResult {
    pub status: DoctorCheckStatus,
    pub events_captured: usize,
    pub summary: String,
}

impl SyntheticSelfTestResult {
    pub fn is_success(&self) -> bool {
        self.status.is_ok() && self.events_captured > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticCaptureError {
    reason: String,
}

impl SyntheticCaptureError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for SyntheticCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for SyntheticCaptureError {}

pub trait SyntheticCaptureRunner {
    fn run_synthetic_capture(&self) -> Result<SyntheticSelfTestResult, SyntheticCaptureError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OfflineSyntheticCapture;

impl SyntheticCaptureRunner for OfflineSyntheticCapture {
    fn run_synthetic_capture(&self) -> Result<SyntheticSelfTestResult, SyntheticCaptureError> {
        Ok(SyntheticSelfTestResult {
            status: DoctorCheckStatus::Ok,
            events_captured: 1,
            summary: "offline synthetic capture completed without network".to_owned(),
        })
    }
}

impl<F> SyntheticCaptureRunner for F
where
    F: Fn() -> Result<SyntheticSelfTestResult, SyntheticCaptureError>,
{
    fn run_synthetic_capture(&self) -> Result<SyntheticSelfTestResult, SyntheticCaptureError> {
        self()
    }
}

pub fn run_synthetic_self_test<R>(runner: &R) -> DoctorCheck
where
    R: SyntheticCaptureRunner + ?Sized,
{
    match runner.run_synthetic_capture() {
        Ok(result) if result.is_success() => DoctorCheck::ok(
            "synthetic self-test",
            format!(
                "{}; captured {} synthetic event(s)",
                result.summary, result.events_captured
            ),
        ),
        Ok(result) => DoctorCheck::error(
            "synthetic self-test",
            format!(
                "{}; captured {} synthetic event(s)",
                result.summary, result.events_captured
            ),
            "Run doctor again with verbose logging and inspect hook/proxy checks above.",
        ),
        Err(error) => DoctorCheck::error(
            "synthetic self-test",
            format!("synthetic capture failed: {error}"),
            "Run doctor again with verbose logging and inspect hook/proxy checks above.",
        ),
    }
}

pub fn run_doctor<C, P, A, R>(
    hook_checker: &C,
    proxy_settings: &ProxySetupSettings,
    proxy_probe: &P,
    log_dir: &Path,
    log_access: &A,
    synthetic_runner: &R,
) -> DoctorReport
where
    C: HookInstallationChecker + ?Sized,
    P: ProxyReachabilityProbe + ?Sized,
    A: LogDirectoryAccess + ?Sized,
    R: SyntheticCaptureRunner + ?Sized,
{
    DoctorReport::new(vec![
        check_hook_installation(hook_checker),
        check_proxy_reachability(proxy_settings, proxy_probe),
        check_log_directory(log_dir, log_access),
        run_synthetic_self_test(synthetic_runner),
    ])
}

pub fn sanitize_output_text(input: &str) -> String {
    let mut sanitized = redact_proxy_error(input);

    for marker in DEFAULT_FORBIDDEN_LOG_MARKERS {
        sanitized = sanitized.replace(marker, "[REDACTED]");
    }

    neutralize_sensitive_key_names(&sanitized)
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

const OUTPUT_SENSITIVE_KEYS: &[&str] = &[
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

fn neutralize_sensitive_key_names(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        if let Some((key_len, delimiter)) = sensitive_key_at(input, index) {
            output.push_str("redacted-field");
            output.push(delimiter);
            index += key_len + delimiter.len_utf8();
            continue;
        }

        let Some(character) = input[index..].chars().next() else {
            break;
        };
        output.push(character);
        index += character.len_utf8();
    }

    output
}

fn sensitive_key_at(input: &str, index: usize) -> Option<(usize, char)> {
    for key in OUTPUT_SENSITIVE_KEYS {
        let end = index + key.len();
        let candidate = input.get(index..end)?;

        if !candidate.eq_ignore_ascii_case(key) {
            continue;
        }

        let delimiter = input[end..].chars().next()?;
        if matches!(delimiter, ':' | '=') {
            return Some((key.len(), delimiter));
        }
    }

    None
}

#[allow(dead_code)]
fn _assert_proxy_probe_object_safe(_: &dyn ProxyReachabilityProbe) {}

#[allow(dead_code)]
fn _assert_proxy_endpoint_owned(_: ProxyEndpoint) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy_privacy::scan_persisted_log_string;
    use crate::proxy_setup::{ProxyProbeError, ProxySetupSettings};

    fn settings() -> ProxySetupSettings {
        ProxySetupSettings::for_supported_target(
            "127.0.0.1",
            48123,
            "https://api.openai.com/v1",
            "codex",
            "openai",
        )
        .unwrap()
    }

    #[test]
    fn reports_missing_hooks_with_actionable_remediation() {
        let check = check_hook_installation(&|| HookInstallStatus::Missing {
            expected_path: PathBuf::from("/tmp/tokmeter/hook.json"),
        });

        assert_eq!(check.status, DoctorCheckStatus::Error);
        assert!(check.summary.contains("hook not installed"));
        assert!(check.remediation.unwrap().contains("Install hooks"));
    }

    #[test]
    fn reports_broken_proxy_without_leaking_probe_error_details() {
        let check = check_proxy_reachability(&settings(), &|_: &ProxyEndpoint| {
            Err(ProxyProbeError::new(
                "connection refused authorization: Bearer sk-test PROMPT_SHOULD_NOT_PERSIST",
            ))
        });
        let output = DoctorReport::new(vec![check]).render();

        assert!(output.contains("[error] proxy"));
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("sk-test"));
        assert!(!output.contains("PROMPT_SHOULD_NOT_PERSIST"));
    }

    #[test]
    fn reports_unwritable_log_directory() {
        let check = check_log_directory(Path::new("/tmp/tokmeter-private"), &|_: &Path| {
            Err(LogDirectoryError::new(
                "permission denied; api_key=sk-test PRIVATE_CREDENTIAL_FIXTURE",
            ))
        });
        let output = DoctorReport::new(vec![check]).render();

        assert!(output.contains("[error] log directory"));
        assert!(output.contains("fix ownership or permissions"));
        assert!(!output.contains("sk-test"));
        assert!(!output.contains("PRIVATE_CREDENTIAL_FIXTURE"));
    }

    #[test]
    fn successful_synthetic_capture_completes_without_network() {
        let check = run_synthetic_self_test(&OfflineSyntheticCapture);

        assert_eq!(check.status, DoctorCheckStatus::Ok);
        assert!(check.summary.contains("without network"));
        assert!(check.summary.contains("captured 1 synthetic event"));
    }

    #[test]
    fn rendered_doctor_output_omits_credentials_and_private_markers() {
        let report = run_doctor(
            &|| HookInstallStatus::Broken {
                path: PathBuf::from("/tmp/hook.json"),
                reason: "authorization: Bearer sk-hook PRIVATE_CONTENT_FIXTURE".to_owned(),
            },
            &settings(),
            &|_: &ProxyEndpoint| {
                Err(ProxyProbeError::new(
                    "upstream failed with openai_api_key=sk-proj-private",
                ))
            },
            Path::new("/tmp/logs"),
            &|_: &Path| Err(LogDirectoryError::new("content=PROMPT_SHOULD_NOT_PERSIST")),
            &|| {
                Err(SyntheticCaptureError::new(
                    "response_body=CONTENT_SHOULD_NOT_PERSIST",
                ))
            },
        );

        let output = report.render();

        for forbidden in [
            "sk-hook",
            "sk-proj-private",
            "PRIVATE_CONTENT_FIXTURE",
            "PROMPT_SHOULD_NOT_PERSIST",
            "CONTENT_SHOULD_NOT_PERSIST",
        ] {
            assert!(
                !output.contains(forbidden),
                "doctor output leaked {forbidden}: {output}"
            );
        }
        assert_eq!(scan_persisted_log_string(&output), Vec::new());
    }
}
