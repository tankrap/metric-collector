use std::fmt;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub upstream_url: String,
}

impl ProxyConfig {
    pub fn new(
        bind_host: impl Into<String>,
        bind_port: u16,
        upstream_url: impl Into<String>,
    ) -> Result<Self, ProxyConfigError> {
        let bind_host = bind_host.into();

        if !is_allowed_localhost(&bind_host) {
            return Err(ProxyConfigError::NonLocalBindHost { bind_host });
        }

        Ok(Self {
            bind_host,
            bind_port,
            upstream_url: upstream_url.into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyConfigError {
    NonLocalBindHost { bind_host: String },
}

impl fmt::Display for ProxyConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonLocalBindHost { bind_host } => write!(
                f,
                "proxy bind host must be localhost-only (localhost, 127.0.0.1, or ::1); got {bind_host:?}"
            ),
        }
    }
}

impl std::error::Error for ProxyConfigError {}

pub fn run_proxy(_config: ProxyConfig) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "localhost-only Linear proxy is not implemented yet",
    ))
}

fn is_allowed_localhost(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_explicit_localhost_bind_hosts() {
        for host in ["localhost", "127.0.0.1", "::1"] {
            let config = ProxyConfig::new(host, 8080, "https://api.linear.app").unwrap();

            assert_eq!(config.bind_host, host);
            assert_eq!(config.bind_port, 8080);
            assert_eq!(config.upstream_url, "https://api.linear.app");
        }
    }

    #[test]
    fn stores_upstream_url_as_string() {
        let upstream_url = String::from("https://example.test/linear/graphql");

        let config = ProxyConfig::new("127.0.0.1", 3030, upstream_url.clone()).unwrap();

        assert_eq!(config.upstream_url, upstream_url);
    }

    #[test]
    fn rejects_wildcard_bind_hosts() {
        for host in ["0.0.0.0", "::"] {
            let err = ProxyConfig::new(host, 8080, "https://api.linear.app").unwrap_err();

            assert_eq!(
                err,
                ProxyConfigError::NonLocalBindHost {
                    bind_host: host.to_string()
                }
            );
        }
    }

    #[test]
    fn rejects_non_local_bind_hosts() {
        for host in [
            "192.168.1.10",
            "10.0.0.5",
            "example.com",
            "linear.app",
            "127.0.0.2",
            "[::1]",
        ] {
            let err = ProxyConfig::new(host, 8080, "https://api.linear.app").unwrap_err();

            assert_eq!(
                err,
                ProxyConfigError::NonLocalBindHost {
                    bind_host: host.to_string()
                }
            );
        }
    }

    #[test]
    fn config_error_message_identifies_allowed_hosts() {
        let err = ProxyConfig::new("0.0.0.0", 8080, "https://api.linear.app").unwrap_err();

        assert_eq!(
            err.to_string(),
            "proxy bind host must be localhost-only (localhost, 127.0.0.1, or ::1); got \"0.0.0.0\""
        );
    }

    #[test]
    fn run_proxy_returns_clear_not_implemented_error() {
        let config = ProxyConfig::new("localhost", 8080, "https://api.linear.app").unwrap();

        let err = run_proxy(config).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert_eq!(
            err.to_string(),
            "localhost-only Linear proxy is not implemented yet"
        );
    }
}
