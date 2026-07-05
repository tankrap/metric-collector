use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};

use crate::mode::ModeState;
use crate::proxy_attribution::{AttributionEvent, attribute_proxy_json_with_mode};
use crate::proxy_privacy::{
    redact_provider_credentials, redact_provider_error, redact_sensitive_headers,
};
use crate::proxy_usage::{ProviderUsage, extract_provider_usage};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyHttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyCapture {
    pub method: String,
    pub path: String,
    pub redacted_headers: Vec<(String, String)>,
    pub usage: ProviderUsage,
    pub events: Vec<AttributionEvent>,
    pub persisted_log_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyRuntimeError {
    InvalidRequestLine,
}

impl fmt::Display for ProxyRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestLine => {
                f.write_str("proxy request must start with an HTTP request line")
            }
        }
    }
}

impl std::error::Error for ProxyRuntimeError {}

pub fn process_proxy_exchange(
    raw_request: &str,
    provider_response_json: &str,
) -> Result<ProxyCapture, ProxyRuntimeError> {
    process_proxy_exchange_with_mode(raw_request, provider_response_json, &ModeState::passive())
}

pub fn process_proxy_exchange_with_mode(
    raw_request: &str,
    provider_response_json: &str,
    mode_state: &ModeState,
) -> Result<ProxyCapture, ProxyRuntimeError> {
    let request = parse_http_request(raw_request)?;
    let path = sanitize_request_path(&request.path);
    let redacted_headers = redact_sensitive_headers(&request.headers);
    let usage = extract_provider_usage(provider_response_json);
    let redacted_response = redact_provider_error(provider_response_json);
    let events = attribute_proxy_json_with_mode(&request.body, &redacted_response, mode_state);
    let persisted_log_line =
        render_persisted_log_line(&request.method, &path, &redacted_headers, usage, &events);

    Ok(ProxyCapture {
        method: request.method,
        path,
        redacted_headers,
        usage,
        events,
        persisted_log_line,
    })
}

pub fn parse_http_request(raw_request: &str) -> Result<ProxyHttpRequest, ProxyRuntimeError> {
    let mut sections = raw_request.splitn(2, "\r\n\r\n");
    let head = sections.next().unwrap_or_default();
    let body = sections
        .next()
        .or_else(|| raw_request.split_once("\n\n").map(|(_, body)| body))
        .unwrap_or_default()
        .to_owned();
    let mut lines = head.lines();
    let request_line = lines
        .next()
        .ok_or(ProxyRuntimeError::InvalidRequestLine)?
        .trim();
    let mut request_line_parts = request_line.split_whitespace();
    let method = request_line_parts
        .next()
        .ok_or(ProxyRuntimeError::InvalidRequestLine)?;
    let path = request_line_parts
        .next()
        .ok_or(ProxyRuntimeError::InvalidRequestLine)?;

    if request_line_parts.next().is_none() {
        return Err(ProxyRuntimeError::InvalidRequestLine);
    }

    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();

    Ok(ProxyHttpRequest {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body,
    })
}

pub fn run_proxy(config: ProxyConfig) -> io::Result<()> {
    let listener = TcpListener::bind((config.bind_host.as_str(), config.bind_port))?;

    for stream in listener.incoming() {
        serve_proxy_connection(stream?)?;
    }

    Ok(())
}

fn is_allowed_localhost(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn serve_proxy_connection(mut stream: TcpStream) -> io::Result<()> {
    let request = read_http_like_request(&mut stream)?;
    let response_body = match parse_http_request(&request) {
        Ok(parsed) => format!(
            "{{\"status\":\"accepted\",\"method\":\"{}\",\"path\":\"{}\"}}\n",
            escape_json_string(&parsed.method),
            escape_json_string(&sanitize_request_path(&parsed.path))
        ),
        Err(error) => format!(
            "{{\"status\":\"bad_request\",\"error\":\"{}\"}}\n",
            escape_json_string(&error.to_string())
        ),
    };

    write!(
        stream,
        "HTTP/1.1 202 Accepted\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    )
}

fn read_http_like_request(stream: &mut impl Read) -> io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let bytes_read = stream.read(&mut chunk)?;
        if bytes_read == 0 {
            break;
        }

        buffer.extend_from_slice(&chunk[..bytes_read]);

        let Some((body_start, header_text)) = request_body_start_and_headers(&buffer) else {
            continue;
        };
        let content_length = content_length(&header_text).unwrap_or(0);

        if buffer.len().saturating_sub(body_start) >= content_length {
            break;
        }
    }

    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn request_body_start_and_headers(buffer: &[u8]) -> Option<(usize, String)> {
    let header_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            buffer
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
        })?;

    Some((
        header_end.0 + header_end.1,
        String::from_utf8_lossy(&buffer[..header_end.0]).into_owned(),
    ))
}

fn content_length(header_text: &str) -> Option<usize> {
    header_text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

fn render_persisted_log_line(
    method: &str,
    path: &str,
    redacted_headers: &[(String, String)],
    usage: ProviderUsage,
    events: &[AttributionEvent],
) -> String {
    let headers_json = redacted_headers
        .iter()
        .map(|(name, value)| {
            format!(
                "[\"{}\",\"{}\"]",
                escape_json_string(name),
                escape_json_string(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let events_json = events
        .iter()
        .map(|event| {
            let allocation_json = event
                .token_allocation
                .as_ref()
                .map(|allocation| {
                    format!(
                        concat!(
                            "{{",
                            "\"input_tokens\":{},",
                            "\"output_tokens\":{},",
                            "\"cache_read_tokens\":{},",
                            "\"cache_write_tokens\":{}",
                            "}}"
                        ),
                        allocation.input_tokens,
                        allocation.output_tokens,
                        allocation.cache_read_tokens,
                        allocation.cache_write_tokens
                    )
                })
                .unwrap_or_else(|| "null".to_owned());

            format!(
                concat!(
                    "{{",
                    "\"mode\":\"{}\",",
                    "\"task_id\":\"{}\",",
                    "\"profile_id\":\"{}\",",
                    "\"op_class\":\"{}\",",
                    "\"tool\":\"{}\",",
                    "\"byte_count\":{},",
                    "\"digest\":\"{}\",",
                    "\"token_allocation\":{},",
                    "\"unattributed\":{}",
                    "}}"
                ),
                event.mode,
                escape_json_string(&event.task_id),
                escape_json_string(&event.profile_id),
                escape_json_string(&event.op_class),
                escape_json_string(&event.tool),
                event.byte_count,
                escape_json_string(&event.digest),
                allocation_json,
                event.unattributed
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        concat!(
            "{{",
            "\"event\":\"proxy.exchange\",",
            "\"method\":\"{}\",",
            "\"path\":\"{}\",",
            "\"headers\":[{}],",
            "\"usage\":{{",
            "\"input_tokens\":{},",
            "\"output_tokens\":{},",
            "\"cache_read_tokens\":{},",
            "\"cache_write_tokens\":{}",
            "}},",
            "\"events\":[{}]",
            "}}"
        ),
        escape_json_string(method),
        escape_json_string(path),
        headers_json,
        optional_u64_json(usage.input_tokens),
        optional_u64_json(usage.output_tokens),
        optional_u64_json(usage.cache_read_tokens),
        optional_u64_json(usage.cache_write_tokens),
        events_json
    )
}

fn sanitize_request_path(path: &str) -> String {
    let redacted = redact_provider_credentials(path);

    redacted
        .split_once('?')
        .map(|(route, _)| format!("{route}?[REDACTED_QUERY]"))
        .unwrap_or(redacted)
}

fn optional_u64_json(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_owned())
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }

    escaped
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
    fn parses_http_like_request_and_redacts_sensitive_headers() {
        let request = parse_http_request(concat!(
            "POST /v1/chat/completions HTTP/1.1\r\n",
            "Host: 127.0.0.1:48123\r\n",
            "Authorization: Bearer sk-live-secret\r\n",
            "content-type: application/json\r\n",
            "\r\n",
            "{\"prompt\":\"private\"}"
        ))
        .unwrap();

        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(
            redact_sensitive_headers(&request.headers),
            vec![
                ("Host".to_owned(), "127.0.0.1:48123".to_owned()),
                (
                    "Authorization".to_owned(),
                    crate::proxy_privacy::REDACTED.to_owned()
                ),
                ("content-type".to_owned(), "application/json".to_owned()),
            ]
        );
    }

    #[test]
    fn reads_http_like_request_until_declared_body_length() {
        let raw_request = concat!(
            "POST /v1/responses HTTP/1.1\r\n",
            "content-length: 15\r\n",
            "\r\n",
            "{\"ok\":true}tail"
        );
        let mut input = std::io::Cursor::new(raw_request.as_bytes());

        let request = read_http_like_request(&mut input).unwrap();

        assert_eq!(
            request,
            concat!(
                "POST /v1/responses HTTP/1.1\r\n",
                "content-length: 15\r\n",
                "\r\n",
                "{\"ok\":true}tail"
            )
        );
    }

    #[test]
    fn proxy_exchange_preserves_cache_usage_fields() {
        let request = concat!(
            "POST /v1/responses HTTP/1.1\r\n",
            "Authorization: Bearer sk-live-secret\r\n",
            "\r\n",
            r#"{"messages":[{"tool_calls":[{"id":"call_1","type":"tool_call","name":"Bash","arguments":"git diff -- src/proxy.rs"}]}]}"#
        );
        let response = r#"{
            "usage": {
                "input_tokens": 40,
                "output_tokens": 12,
                "cache_read_tokens": 9,
                "cache_write_tokens": 3
            },
            "content": [{
                "type": "tool_result",
                "tool_use_id": "call_1",
                "content": "diff --git a/src/proxy.rs b/src/proxy.rs\n+line",
                "input_tokens": 40,
                "output_tokens": 12,
                "cache_read_tokens": 9,
                "cache_write_tokens": 3
            }]
        }"#;

        let capture = process_proxy_exchange(request, response).unwrap();

        assert_eq!(capture.usage.input_tokens, Some(40));
        assert_eq!(capture.usage.output_tokens, Some(12));
        assert_eq!(capture.usage.cache_read_tokens, Some(9));
        assert_eq!(capture.usage.cache_write_tokens, Some(3));
        assert_eq!(capture.events.len(), 1);
        assert_eq!(capture.events[0].op_class, "vc.diff");
        assert_eq!(capture.events[0].tool, "Bash");
        assert_eq!(
            capture.events[0]
                .token_allocation
                .as_ref()
                .unwrap()
                .cache_read_tokens,
            9
        );
        assert_eq!(
            capture.events[0]
                .token_allocation
                .as_ref()
                .unwrap()
                .cache_write_tokens,
            3
        );
        assert!(
            capture
                .persisted_log_line
                .contains("\"cache_read_tokens\":9")
        );
        assert!(
            capture
                .persisted_log_line
                .contains("\"cache_write_tokens\":3")
        );
    }

    #[test]
    fn proxy_exchange_does_not_persist_credentials_or_content() {
        let request = concat!(
            "POST /v1/messages?api_key=PRIVATE_CREDENTIAL_FIXTURE HTTP/1.1\r\n",
            "Authorization: Bearer sk-live-credential-123456789\r\n",
            "X-API-KEY: TOKMETER_FIXTURE_SECRET\r\n",
            "\r\n",
            r#"{"prompt":"PROMPT_SHOULD_NOT_PERSIST","messages":"CONTENT_SHOULD_NOT_PERSIST"}"#
        );
        let response = r#"{
            "usage": {"input_tokens": 1, "output_tokens": 2},
            "content": [{
                "type": "tool_result",
                "tool": "Read",
                "content": "SECRET_RAW_TOOL_OUTPUT"
            }],
            "authorization": "Bearer sk-response-secret-123"
        }"#;

        let capture = process_proxy_exchange(request, response).unwrap();
        let rendered = format!("{capture:?}\n{}", capture.persisted_log_line);
        let failures = crate::proxy_privacy::scan_persisted_log_string(&rendered);

        assert_eq!(failures, Vec::new());
        assert!(capture.redacted_headers.iter().any(
            |(name, value)| name == "Authorization" && value == crate::proxy_privacy::REDACTED
        ));
        assert!(!rendered.contains("sk-live-credential-123456789"));
        assert!(!rendered.contains("TOKMETER_FIXTURE_SECRET"));
        assert!(!rendered.contains("PROMPT_SHOULD_NOT_PERSIST"));
        assert!(!rendered.contains("CONTENT_SHOULD_NOT_PERSIST"));
        assert!(!rendered.contains("SECRET_RAW_TOOL_OUTPUT"));
    }

    #[test]
    fn proxy_exchange_outputs_attribution_compatible_event_shape() {
        let request = concat!(
            "POST /v1/responses HTTP/1.1\r\n",
            "\r\n",
            r#"{"messages":[{"tool_calls":[{"id":"call_1","type":"tool_call","name":"Bash","arguments":"cargo test proxy"}]}]}"#
        );
        let response = r#"{
            "content": [{
                "type": "tool_result",
                "tool_use_id": "call_1",
                "content": "running 1 test\ntest result: ok"
            }]
        }"#;

        let capture = process_proxy_exchange(request, response).unwrap();

        assert_eq!(capture.method, "POST");
        assert_eq!(capture.path, "/v1/responses");
        assert_eq!(capture.events.len(), 1);
        assert_eq!(capture.events[0].mode, crate::core::CaptureMode::Passive);
        assert_eq!(capture.events[0].task_id, "adhoc");
        assert_eq!(capture.events[0].profile_id, "adhoc");
        assert_eq!(capture.events[0].op_class, "test.output");
        assert_eq!(capture.events[0].tool, "Bash");
        assert!(!capture.events[0].unattributed);
        assert!(
            capture
                .persisted_log_line
                .contains("\"event\":\"proxy.exchange\"")
        );
        assert!(
            capture
                .persisted_log_line
                .contains("\"op_class\":\"test.output\"")
        );
    }

    #[test]
    fn proxy_loop_rejects_non_localhost_bind_hosts_before_binding() {
        let error = ProxyConfig::new("0.0.0.0", 0, "https://api.linear.app").unwrap_err();

        assert_eq!(
            error,
            ProxyConfigError::NonLocalBindHost {
                bind_host: "0.0.0.0".to_owned()
            }
        );
    }
}
