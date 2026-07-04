use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySetupSettings {
    pub host: String,
    pub port: u16,
    pub upstream_base_url: String,
    pub adapter: ProxyAdapterMetadata,
}

impl ProxySetupSettings {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        upstream_base_url: impl Into<String>,
        adapter: ProxyAdapterMetadata,
    ) -> Result<Self, ProxySetupError> {
        let host = host.into();
        let upstream_base_url = upstream_base_url.into();

        validate_host(&host)?;
        validate_port(port)?;
        validate_upstream_base_url(&upstream_base_url)?;
        adapter.validate()?;

        Ok(Self {
            host,
            port,
            upstream_base_url,
            adapter,
        })
    }

    pub fn for_supported_target(
        host: impl Into<String>,
        port: u16,
        upstream_base_url: impl Into<String>,
        agent_label: &str,
        provider_label: &str,
    ) -> Result<Self, ProxySetupError> {
        let target = setup_target_for(agent_label, provider_label)?;

        Self::new(
            host,
            port,
            upstream_base_url,
            ProxyAdapterMetadata::new(
                target.agent_label,
                target.provider_label,
                target.adapter_label,
            ),
        )
    }

    pub fn local_base_url(&self) -> String {
        format!("http://{}:{}", host_for_url(&self.host), self.port)
    }

    pub fn provider_base_url(&self) -> Result<String, ProxySetupError> {
        let target = setup_target_for(&self.adapter.agent_label, &self.adapter.provider_label)?;
        Ok(format!(
            "{}{}",
            self.local_base_url(),
            target.base_url_suffix
        ))
    }

    pub fn metadata_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("proxy.host", self.host.clone()),
            ("proxy.port", self.port.to_string()),
            ("proxy.upstream_base_url", self.upstream_base_url.clone()),
            ("proxy.agent", self.adapter.agent_label.clone()),
            ("proxy.provider", self.adapter.provider_label.clone()),
            ("proxy.adapter", self.adapter.adapter_label.clone()),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyAdapterMetadata {
    pub agent_label: String,
    pub provider_label: String,
    pub adapter_label: String,
}

impl ProxyAdapterMetadata {
    pub fn new(
        agent_label: impl Into<String>,
        provider_label: impl Into<String>,
        adapter_label: impl Into<String>,
    ) -> Self {
        Self {
            agent_label: agent_label.into(),
            provider_label: provider_label.into(),
            adapter_label: adapter_label.into(),
        }
    }

    fn validate(&self) -> Result<(), ProxySetupError> {
        validate_label("agent label", &self.agent_label)?;
        validate_label("provider label", &self.provider_label)?;
        validate_label("adapter label", &self.adapter_label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupTarget {
    pub agent_label: &'static str,
    pub provider_label: &'static str,
    pub adapter_label: &'static str,
    pub environment_variable: &'static str,
    pub base_url_suffix: &'static str,
}

const SETUP_TARGETS: &[SetupTarget] = &[
    SetupTarget {
        agent_label: "codex-cli",
        provider_label: "openai",
        adapter_label: "proxy.codex.openai",
        environment_variable: "OPENAI_BASE_URL",
        base_url_suffix: "/v1",
    },
    SetupTarget {
        agent_label: "openai-compatible",
        provider_label: "openai",
        adapter_label: "proxy.openai-compatible",
        environment_variable: "OPENAI_BASE_URL",
        base_url_suffix: "/v1",
    },
    SetupTarget {
        agent_label: "claude-code",
        provider_label: "anthropic",
        adapter_label: "proxy.claude.anthropic",
        environment_variable: "ANTHROPIC_BASE_URL",
        base_url_suffix: "",
    },
    SetupTarget {
        agent_label: "anthropic-compatible",
        provider_label: "anthropic",
        adapter_label: "proxy.anthropic-compatible",
        environment_variable: "ANTHROPIC_BASE_URL",
        base_url_suffix: "",
    },
];

pub fn supported_setup_targets() -> &'static [SetupTarget] {
    SETUP_TARGETS
}

pub fn setup_target_for(
    agent_label: &str,
    provider_label: &str,
) -> Result<&'static SetupTarget, ProxySetupError> {
    let agent_label = normalize_label(agent_label);
    let provider_label = normalize_label(provider_label);

    SETUP_TARGETS
        .iter()
        .find(|target| {
            agent_matches(&agent_label, target.agent_label)
                && provider_matches(&provider_label, target.provider_label)
        })
        .ok_or(ProxySetupError::UnsupportedTarget {
            agent_label,
            provider_label,
        })
}

pub fn render_setup_instructions(settings: &ProxySetupSettings) -> Result<String, ProxySetupError> {
    let mut output = String::new();
    write_setup_instructions(&mut output, settings)?;
    Ok(output)
}

pub fn write_setup_instructions(
    output: &mut impl fmt::Write,
    settings: &ProxySetupSettings,
) -> Result<(), ProxySetupError> {
    let target = setup_target_for(
        &settings.adapter.agent_label,
        &settings.adapter.provider_label,
    )?;
    let provider_base_url = settings.provider_base_url()?;

    writeln!(
        output,
        "Configure {} for {} through vc-tokmeter",
        display_label(target.agent_label),
        display_label(target.provider_label)
    )?;
    writeln!(output)?;
    writeln!(output, "Proxy endpoint: {}", settings.local_base_url())?;
    writeln!(output, "Provider base URL: {provider_base_url}")?;
    writeln!(output, "Upstream provider: {}", settings.upstream_base_url)?;
    writeln!(output)?;
    writeln!(
        output,
        "Set this before starting the agent:\n  export {}=\"{}\"",
        target.environment_variable, provider_base_url
    )?;
    writeln!(output)?;
    writeln!(
        output,
        "Keep provider credentials in the agent or shell environment; tokmeter stores only proxy host, port, upstream, and adapter metadata."
    )?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
    pub base_url: String,
}

impl ProxyEndpoint {
    pub fn from_settings(settings: &ProxySetupSettings) -> Self {
        Self {
            host: settings.host.clone(),
            port: settings.port,
            base_url: settings.local_base_url(),
        }
    }
}

pub trait ProxyReachabilityProbe {
    fn probe(&self, endpoint: &ProxyEndpoint) -> Result<(), ProxyProbeError>;
}

impl<F> ProxyReachabilityProbe for F
where
    F: Fn(&ProxyEndpoint) -> Result<(), ProxyProbeError>,
{
    fn probe(&self, endpoint: &ProxyEndpoint) -> Result<(), ProxyProbeError> {
        self(endpoint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyProbeError {
    message: String,
}

impl ProxyProbeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProxyProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProxyProbeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorStatus {
    Reachable,
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyDoctorCheck {
    pub status: DoctorStatus,
    pub endpoint: ProxyEndpoint,
    pub message: String,
}

impl ProxyDoctorCheck {
    pub fn is_reachable(&self) -> bool {
        self.status == DoctorStatus::Reachable
    }
}

pub fn doctor_proxy_reachability<P>(settings: &ProxySetupSettings, probe: &P) -> ProxyDoctorCheck
where
    P: ProxyReachabilityProbe + ?Sized,
{
    let endpoint = ProxyEndpoint::from_settings(settings);

    match probe.probe(&endpoint) {
        Ok(()) => ProxyDoctorCheck {
            status: DoctorStatus::Reachable,
            message: format!("ok: proxy reachable at {}", endpoint.base_url),
            endpoint,
        },
        Err(error) => ProxyDoctorCheck {
            status: DoctorStatus::Unreachable,
            message: format!(
                "error: proxy not reachable at {} ({error})",
                endpoint.base_url
            ),
            endpoint,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxySetupError {
    BlankField {
        field: &'static str,
    },
    InvalidHost {
        host: String,
    },
    InvalidPort {
        port: u16,
    },
    InvalidUpstreamBaseUrl {
        upstream_base_url: String,
        reason: &'static str,
    },
    CredentialBearingUpstreamBaseUrl {
        upstream_base_url: String,
    },
    UnsupportedTarget {
        agent_label: String,
        provider_label: String,
    },
    FormatFailed,
}

impl fmt::Display for ProxySetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} is required"),
            Self::InvalidHost { host } => write!(
                formatter,
                "proxy host must be a bare host name or IP address; got {host:?}"
            ),
            Self::InvalidPort { port } => {
                write!(
                    formatter,
                    "proxy port must be greater than zero; got {port}"
                )
            }
            Self::InvalidUpstreamBaseUrl {
                upstream_base_url,
                reason,
            } => write!(
                formatter,
                "upstream base URL {upstream_base_url:?} is invalid: {reason}"
            ),
            Self::CredentialBearingUpstreamBaseUrl { upstream_base_url } => write!(
                formatter,
                "upstream base URL must not include credentials; got {upstream_base_url:?}"
            ),
            Self::UnsupportedTarget {
                agent_label,
                provider_label,
            } => write!(
                formatter,
                "unsupported proxy setup target: agent={agent_label:?}, provider={provider_label:?}"
            ),
            Self::FormatFailed => formatter.write_str("failed to format proxy setup output"),
        }
    }
}

impl std::error::Error for ProxySetupError {}

impl From<fmt::Error> for ProxySetupError {
    fn from(_: fmt::Error) -> Self {
        Self::FormatFailed
    }
}

fn validate_host(host: &str) -> Result<(), ProxySetupError> {
    if host.is_empty() {
        return Err(ProxySetupError::BlankField {
            field: "proxy host",
        });
    }

    if host.trim() != host
        || host.contains("://")
        || host.contains('/')
        || host.contains('@')
        || host.chars().any(char::is_whitespace)
    {
        return Err(ProxySetupError::InvalidHost {
            host: host.to_owned(),
        });
    }

    Ok(())
}

fn validate_port(port: u16) -> Result<(), ProxySetupError> {
    if port == 0 {
        return Err(ProxySetupError::InvalidPort { port });
    }

    Ok(())
}

fn validate_label(field: &'static str, value: &str) -> Result<(), ProxySetupError> {
    if value.trim().is_empty() {
        return Err(ProxySetupError::BlankField { field });
    }

    Ok(())
}

fn validate_upstream_base_url(upstream_base_url: &str) -> Result<(), ProxySetupError> {
    if upstream_base_url.trim().is_empty() {
        return Err(ProxySetupError::BlankField {
            field: "upstream base URL",
        });
    }

    if upstream_base_url.trim() != upstream_base_url {
        return Err(ProxySetupError::InvalidUpstreamBaseUrl {
            upstream_base_url: upstream_base_url.to_owned(),
            reason: "leading or trailing whitespace is not allowed",
        });
    }

    let Some(authority_and_path) = upstream_base_url
        .strip_prefix("https://")
        .or_else(|| upstream_base_url.strip_prefix("http://"))
    else {
        return Err(ProxySetupError::InvalidUpstreamBaseUrl {
            upstream_base_url: upstream_base_url.to_owned(),
            reason: "expected http:// or https://",
        });
    };

    if authority_and_path.is_empty() {
        return Err(ProxySetupError::InvalidUpstreamBaseUrl {
            upstream_base_url: upstream_base_url.to_owned(),
            reason: "missing host",
        });
    }

    let authority_end = authority_and_path
        .find(['/', '?', '#'])
        .unwrap_or(authority_and_path.len());
    let authority = &authority_and_path[..authority_end];

    if authority.is_empty() {
        return Err(ProxySetupError::InvalidUpstreamBaseUrl {
            upstream_base_url: upstream_base_url.to_owned(),
            reason: "missing host",
        });
    }

    if authority.contains('@') {
        return Err(ProxySetupError::CredentialBearingUpstreamBaseUrl {
            upstream_base_url: upstream_base_url.to_owned(),
        });
    }

    if authority_and_path.contains('?') || authority_and_path.contains('#') {
        return Err(ProxySetupError::InvalidUpstreamBaseUrl {
            upstream_base_url: upstream_base_url.to_owned(),
            reason: "base URLs must not include query strings or fragments",
        });
    }

    Ok(())
}

fn normalize_label(label: &str) -> String {
    label
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| match character {
            '_' | ' ' => '-',
            other => other,
        })
        .collect()
}

fn agent_matches(label: &str, canonical: &str) -> bool {
    match canonical {
        "codex-cli" => matches!(label, "codex" | "codex-cli"),
        "openai-compatible" => matches!(label, "openai" | "openai-compatible" | "generic-openai"),
        "claude-code" => matches!(label, "claude" | "claude-code"),
        "anthropic-compatible" => {
            matches!(
                label,
                "anthropic" | "anthropic-compatible" | "generic-anthropic"
            )
        }
        _ => label == canonical,
    }
}

fn provider_matches(label: &str, canonical: &str) -> bool {
    match canonical {
        "openai" => matches!(label, "openai" | "openai-compatible"),
        "anthropic" => matches!(label, "anthropic" | "anthropic-compatible"),
        _ => label == canonical,
    }
}

fn display_label(label: &str) -> &'static str {
    match label {
        "codex-cli" => "Codex CLI",
        "openai-compatible" => "OpenAI-compatible agents",
        "claude-code" => "Claude Code",
        "anthropic-compatible" => "Anthropic-compatible agents",
        "openai" => "OpenAI-compatible providers",
        "anthropic" => "Anthropic-compatible providers",
        _ => "supported agents",
    }
}

fn host_for_url(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

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
    fn prints_codex_openai_base_url_instructions() {
        let output = render_setup_instructions(&settings()).unwrap();

        assert!(output.contains("Configure Codex CLI for OpenAI-compatible providers"));
        assert!(output.contains("Proxy endpoint: http://127.0.0.1:48123"));
        assert!(output.contains("Provider base URL: http://127.0.0.1:48123/v1"));
        assert!(output.contains("export OPENAI_BASE_URL=\"http://127.0.0.1:48123/v1\""));
        assert!(output.contains("Upstream provider: https://api.openai.com/v1"));
    }

    #[test]
    fn prints_claude_anthropic_base_url_instructions() {
        let settings = ProxySetupSettings::for_supported_target(
            "localhost",
            48124,
            "https://api.anthropic.com",
            "claude code",
            "anthropic",
        )
        .unwrap();

        let output = render_setup_instructions(&settings).unwrap();

        assert!(output.contains("Configure Claude Code for Anthropic-compatible providers"));
        assert!(output.contains("Provider base URL: http://localhost:48124"));
        assert!(output.contains("export ANTHROPIC_BASE_URL=\"http://localhost:48124\""));
    }

    #[test]
    fn doctor_reports_reachable_proxy() {
        let called = Cell::new(false);
        let check = doctor_proxy_reachability(&settings(), &|endpoint: &ProxyEndpoint| {
            called.set(true);
            assert_eq!(endpoint.host, "127.0.0.1");
            assert_eq!(endpoint.port, 48123);
            assert_eq!(endpoint.base_url, "http://127.0.0.1:48123");
            Ok(())
        });

        assert!(called.get());
        assert_eq!(check.status, DoctorStatus::Reachable);
        assert!(check.is_reachable());
        assert_eq!(
            check.message,
            "ok: proxy reachable at http://127.0.0.1:48123"
        );
    }

    #[test]
    fn doctor_reports_unreachable_proxy() {
        let check = doctor_proxy_reachability(&settings(), &|_: &ProxyEndpoint| {
            Err(ProxyProbeError::new("connection refused"))
        });

        assert_eq!(check.status, DoctorStatus::Unreachable);
        assert!(!check.is_reachable());
        assert_eq!(
            check.message,
            "error: proxy not reachable at http://127.0.0.1:48123 (connection refused)"
        );
    }

    #[test]
    fn settings_debug_and_metadata_have_no_credential_fields() {
        let settings = settings();
        let debug = format!("{settings:?}").to_ascii_lowercase();
        let metadata = format!("{:?}", settings.metadata_pairs()).to_ascii_lowercase();

        for forbidden in [
            "api_key",
            "apikey",
            "authorization",
            "password",
            "secret",
            "token",
        ] {
            assert!(!debug.contains(forbidden), "debug included {}", forbidden);
            assert!(
                !metadata.contains(forbidden),
                "metadata included {}",
                forbidden
            );
        }

        assert_eq!(
            settings.metadata_pairs(),
            vec![
                ("proxy.host", "127.0.0.1".to_owned()),
                ("proxy.port", "48123".to_owned()),
                (
                    "proxy.upstream_base_url",
                    "https://api.openai.com/v1".to_owned()
                ),
                ("proxy.agent", "codex-cli".to_owned()),
                ("proxy.provider", "openai".to_owned()),
                ("proxy.adapter", "proxy.codex.openai".to_owned()),
            ]
        );
    }

    #[test]
    fn settings_reject_upstream_urls_with_credentials() {
        let error = ProxySetupSettings::for_supported_target(
            "127.0.0.1",
            48123,
            "https://user:pass@api.openai.com/v1",
            "codex",
            "openai",
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ProxySetupError::CredentialBearingUpstreamBaseUrl { .. }
        ));
    }

    #[test]
    fn settings_reject_upstream_urls_with_query_strings() {
        let error = ProxySetupSettings::for_supported_target(
            "127.0.0.1",
            48123,
            "https://api.openai.com/v1?api_key=sk-test",
            "codex",
            "openai",
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ProxySetupError::InvalidUpstreamBaseUrl { .. }
        ));
    }
}
