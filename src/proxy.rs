use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::{Role, WebSocket};
use tungstenite::{Message, connect};

use crate::core::{AttributedTokenEvent, EventLog, EventLogError, OperationClass, TokenCounts};
use crate::digest::digest_bytes;
use crate::mode::ModeState;
use crate::proxy_attribution::{AttributionEvent, attribute_proxy_json_with_mode};
use crate::proxy_privacy::{
    redact_provider_credentials, redact_provider_error, redact_sensitive_headers,
};
use crate::proxy_usage::{ProviderUsage, extract_provider_usage};
use crate::token_estimator::estimate_tool_result_tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub upstream_url: String,
    pub event_log_path: Option<PathBuf>,
    pub adapter_label: String,
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
            event_log_path: None,
            adapter_label: "proxy".to_owned(),
        })
    }

    pub fn with_event_log_path(mut self, event_log_path: impl Into<PathBuf>) -> Self {
        self.event_log_path = Some(event_log_path.into());
        self
    }

    pub fn with_adapter_label(mut self, adapter_label: impl Into<String>) -> Self {
        let adapter_label = adapter_label.into();
        if !adapter_label.trim().is_empty() {
            self.adapter_label = adapter_label;
        }
        self
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
    pub core_events: Vec<AttributedTokenEvent>,
    pub event_log_records: Vec<u8>,
    pub persisted_log_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyRuntimeError {
    InvalidRequestLine,
    InvalidUpstreamUrl { upstream_url: String },
    UnsupportedUpstreamScheme { scheme: String },
    MissingUpstreamHost { upstream_url: String },
    InvalidUpstreamPort { port: String },
    Io { message: String },
    EventLog { message: String },
}

impl fmt::Display for ProxyRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestLine => {
                f.write_str("proxy request must start with an HTTP request line")
            }
            Self::InvalidUpstreamUrl { upstream_url } => {
                write!(f, "proxy upstream URL is invalid: {upstream_url:?}")
            }
            Self::UnsupportedUpstreamScheme { scheme } => write!(
                f,
                "proxy upstream scheme must be http or https; got {scheme:?}"
            ),
            Self::MissingUpstreamHost { upstream_url } => {
                write!(
                    f,
                    "proxy upstream URL must include a host: {upstream_url:?}"
                )
            }
            Self::InvalidUpstreamPort { port } => {
                write!(f, "proxy upstream URL port is invalid: {port:?}")
            }
            Self::Io { message } => write!(f, "proxy upstream I/O failed: {message}"),
            Self::EventLog { message } => {
                write!(f, "proxy event log serialization failed: {message}")
            }
        }
    }
}

impl std::error::Error for ProxyRuntimeError {}

impl From<EventLogError> for ProxyRuntimeError {
    fn from(error: EventLogError) -> Self {
        Self::EventLog {
            message: redact_provider_error(&error.to_string()),
        }
    }
}

impl From<io::Error> for ProxyRuntimeError {
    fn from(error: io::Error) -> Self {
        Self::Io {
            message: redact_provider_error(&error.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyForwardedExchange {
    pub response_bytes: Vec<u8>,
    pub capture: ProxyCapture,
}

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
    process_proxy_exchange_with_mode_and_adapter(
        raw_request,
        provider_response_json,
        mode_state,
        "proxy",
    )
}

fn process_proxy_exchange_with_mode_and_adapter(
    raw_request: &str,
    provider_response_json: &str,
    mode_state: &ModeState,
    adapter_label: &str,
) -> Result<ProxyCapture, ProxyRuntimeError> {
    let request = parse_http_request(raw_request)?;
    let path = sanitize_request_path(&request.path);
    let redacted_headers = redact_sensitive_headers(&request.headers);
    let usage = extract_provider_usage(provider_response_json);
    let redacted_response = redact_provider_error(provider_response_json);
    let events = attribute_proxy_json_with_mode(&request.body, &redacted_response, mode_state);
    let mut core_events =
        core_events_from_proxy_events(&events, usage, current_timestamp_ms(), adapter_label);
    if core_events.is_empty() {
        core_events.extend(estimated_proxy_payload_event(
            provider_response_json,
            "proxy.estimated",
            "proxy.response.estimated",
            EstimatedProxyDirection::Output,
            current_timestamp_ms(),
        ));
    }
    let event_log_records = render_core_event_log_records(&core_events)?;
    let persisted_log_line =
        render_persisted_log_line(&request.method, &path, &redacted_headers, usage, &events);

    Ok(ProxyCapture {
        method: request.method,
        path,
        redacted_headers,
        usage,
        events,
        core_events,
        event_log_records,
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
    serve_proxy_listener(config, listener)
}

pub fn serve_proxy_listener(config: ProxyConfig, listener: TcpListener) -> io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = serve_proxy_connection(stream, &config) {
                    eprintln!("proxy connection failed: {error}");
                }
            }
            Err(error) => eprintln!("proxy accept failed: {error}"),
        }
    }

    Ok(())
}

fn is_allowed_localhost(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub fn forward_proxy_request(
    config: &ProxyConfig,
    raw_request: &str,
) -> Result<ProxyForwardedExchange, ProxyRuntimeError> {
    forward_proxy_request_with_mode(config, raw_request, &ModeState::passive())
}

pub fn forward_proxy_request_with_mode(
    config: &ProxyConfig,
    raw_request: &str,
    mode_state: &ModeState,
) -> Result<ProxyForwardedExchange, ProxyRuntimeError> {
    let upstream = ParsedUpstream::parse(&config.upstream_url)?;
    let upstream_request = build_upstream_request(raw_request, &upstream)?;
    let response_bytes = send_upstream_request(&upstream, &upstream_request)?;
    let response_body = response_body_string(&response_bytes);
    let capture = process_proxy_exchange_with_mode_and_adapter(
        raw_request,
        &response_body,
        mode_state,
        &config.adapter_label,
    )?;

    Ok(ProxyForwardedExchange {
        response_bytes,
        capture,
    })
}

fn serve_proxy_connection(mut stream: TcpStream, config: &ProxyConfig) -> io::Result<()> {
    let request = read_http_like_request(&mut stream)?;
    if is_websocket_upgrade_request(&request) {
        return serve_websocket_proxy_connection(stream, config, &request);
    }

    match forward_proxy_request(config, &request) {
        Ok(exchange) => {
            if let Some(event_log_path) = &config.event_log_path {
                append_proxy_event_log_records(
                    event_log_path,
                    &exchange.capture.event_log_records,
                )?;
            }
            stream.write_all(&exchange.response_bytes)
        }
        Err(error) => write_proxy_error_response(&mut stream, error),
    }
}

fn serve_websocket_proxy_connection(
    mut stream: TcpStream,
    config: &ProxyConfig,
    raw_request: &str,
) -> io::Result<()> {
    let request = parse_http_request(raw_request)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let upstream = ParsedUpstream::parse(&config.upstream_url)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let upstream_url = upstream.websocket_url_for_request_path(&request.path);
    let mut upstream_request = upstream_url
        .into_client_request()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;

    copy_websocket_forward_headers(&request.headers, upstream_request.headers_mut())?;

    let (mut upstream_ws, upstream_response) = match connect(upstream_request) {
        Ok(upstream) => upstream,
        Err(error) => {
            return write_proxy_error_response(
                &mut stream,
                ProxyRuntimeError::Io {
                    message: redact_provider_error(&error.to_string()),
                },
            );
        }
    };

    write_websocket_upgrade_response(&mut stream, &request, upstream_response.headers())?;
    let mut local_ws = WebSocket::from_raw_socket(stream, Role::Server, None);
    local_ws
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(150)))?;
    forward_initial_websocket_client_messages(&mut local_ws, &mut upstream_ws, config)?;
    relay_upstream_websocket_messages(&mut upstream_ws, &mut local_ws, config)
}

fn is_websocket_upgrade_request(raw_request: &str) -> bool {
    parse_http_request(raw_request)
        .map(|request| {
            request.headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("upgrade") && value.eq_ignore_ascii_case("websocket")
            })
        })
        .unwrap_or(false)
}

fn append_proxy_event_log_records(path: &PathBuf, records: &[u8]) -> io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(records)
}

fn forward_initial_websocket_client_messages<S>(
    local_ws: &mut tungstenite::WebSocket<TcpStream>,
    upstream_ws: &mut tungstenite::WebSocket<S>,
    config: &ProxyConfig,
) -> io::Result<()>
where
    S: Read + Write,
{
    loop {
        match local_ws.read() {
            Ok(message) => {
                let should_stop = matches!(message, Message::Close(_));
                capture_websocket_message(&message, config, EstimatedProxyDirection::Input)?;
                upstream_ws.send(message).map_err(websocket_io_error)?;
                if should_stop {
                    return Ok(());
                }
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(websocket_io_error(error)),
        }
    }
}

fn relay_upstream_websocket_messages<S>(
    upstream_ws: &mut tungstenite::WebSocket<S>,
    local_ws: &mut tungstenite::WebSocket<TcpStream>,
    config: &ProxyConfig,
) -> io::Result<()>
where
    S: Read + Write,
{
    loop {
        let message = match upstream_ws.read() {
            Ok(message) => message,
            Err(tungstenite::Error::ConnectionClosed) => return Ok(()),
            Err(error) => return Err(websocket_io_error(error)),
        };

        capture_websocket_message(&message, config, EstimatedProxyDirection::Output)?;
        let should_stop = matches!(message, Message::Close(_));
        local_ws.send(message).map_err(websocket_io_error)?;

        if should_stop {
            return Ok(());
        }
    }
}

fn capture_websocket_message(
    message: &Message,
    config: &ProxyConfig,
    direction: EstimatedProxyDirection,
) -> io::Result<()> {
    let Some(event_log_path) = &config.event_log_path else {
        return Ok(());
    };
    let Some(text) = websocket_message_text(message) else {
        return Ok(());
    };
    let usage = extract_provider_usage(text);
    let events = if usage.has_any_usage() {
        core_events_from_proxy_events(&[], usage, current_timestamp_ms(), "proxy.ws")
    } else {
        estimated_proxy_payload_event(
            text,
            "proxy.ws.estimated",
            direction.websocket_tool_name(),
            direction,
            current_timestamp_ms(),
        )
        .into_iter()
        .collect()
    };
    if events.is_empty() {
        return Ok(());
    }
    let records = render_core_event_log_records(&events)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    append_proxy_event_log_records(event_log_path, &records)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EstimatedProxyDirection {
    Input,
    Output,
}

impl EstimatedProxyDirection {
    const fn websocket_tool_name(self) -> &'static str {
        match self {
            Self::Input => "proxy.ws.request.estimated",
            Self::Output => "proxy.ws.response.estimated",
        }
    }

    const fn event_label(self) -> &'static str {
        match self {
            Self::Input => "request",
            Self::Output => "response",
        }
    }
}

fn estimated_proxy_payload_event(
    payload: &str,
    adapter: &str,
    tool: &str,
    direction: EstimatedProxyDirection,
    timestamp_ms: u64,
) -> Option<AttributedTokenEvent> {
    if payload.trim().is_empty() {
        return None;
    }

    let estimate = estimate_tool_result_tokens(payload);
    let estimated_tokens = estimate.tokens.output_tokens;
    if estimated_tokens == 0 {
        return None;
    }
    let mode = ModeState::passive();
    let tokens = match direction {
        EstimatedProxyDirection::Input => TokenCounts::new(estimated_tokens, 0, 0, 0),
        EstimatedProxyDirection::Output => TokenCounts::new(0, estimated_tokens, 0, 0),
    };

    Some(AttributedTokenEvent {
        timestamp_ms,
        mode: mode.mode,
        run_id: "proxy-passive".to_owned(),
        task_id: mode.task_id,
        profile_id: mode.profile_id,
        adapter: adapter.to_owned(),
        operation_class: OperationClass::Other,
        tool: tool.to_owned(),
        tokens,
        byte_count: payload.len() as u64,
        content_digest: estimate.content_digest,
        repeat_of: None,
        action_subtype: None,
        direction: Some(direction.event_label().to_owned()),
    })
}

fn websocket_message_text(message: &Message) -> Option<&str> {
    match message {
        Message::Text(text) => Some(text.as_ref()),
        _ => None,
    }
}

fn copy_websocket_forward_headers(
    headers: &[(String, String)],
    upstream_headers: &mut tungstenite::http::HeaderMap,
) -> io::Result<()> {
    for (name, value) in headers {
        if is_hop_by_hop_websocket_header(name) {
            continue;
        }
        let Ok(header_name) = tungstenite::http::HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(header_value) = tungstenite::http::HeaderValue::from_str(value) else {
            continue;
        };
        upstream_headers.insert(header_name, header_value);
    }

    Ok(())
}

fn write_websocket_upgrade_response(
    mut stream: impl Write,
    request: &ProxyHttpRequest,
    upstream_headers: &tungstenite::http::HeaderMap,
) -> io::Result<()> {
    let Some(key) = header_value(&request.headers, "sec-websocket-key") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing Sec-WebSocket-Key header",
        ));
    };

    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n",
        derive_accept_key(key.as_bytes())
    )?;

    if let Some(protocol) = upstream_headers
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
    {
        write!(stream, "Sec-WebSocket-Protocol: {protocol}\r\n")?;
    }

    stream.write_all(b"\r\n")
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn is_hop_by_hop_websocket_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("host")
        || name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("upgrade")
        || name.eq_ignore_ascii_case("sec-websocket-key")
        || name.eq_ignore_ascii_case("sec-websocket-version")
        || name.eq_ignore_ascii_case("sec-websocket-extensions")
}

fn websocket_io_error(error: tungstenite::Error) -> io::Error {
    io::Error::other(redact_provider_error(&error.to_string()))
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUpstream {
    scheme: String,
    host: String,
    port: u16,
    base_path: String,
    host_header: String,
}

impl ParsedUpstream {
    fn parse(upstream_url: &str) -> Result<Self, ProxyRuntimeError> {
        let Some((scheme, rest)) = upstream_url.split_once("://") else {
            return Err(ProxyRuntimeError::InvalidUpstreamUrl {
                upstream_url: upstream_url.to_owned(),
            });
        };

        if !matches!(scheme, "http" | "https") {
            return Err(ProxyRuntimeError::UnsupportedUpstreamScheme {
                scheme: scheme.to_owned(),
            });
        }

        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.is_empty() {
            return Err(ProxyRuntimeError::MissingUpstreamHost {
                upstream_url: upstream_url.to_owned(),
            });
        }

        let default_port = if scheme == "https" { 443 } else { 80 };
        let (host, port) = parse_authority(authority, default_port)?;
        let host_header = if port == default_port {
            host.clone()
        } else {
            format!("{host}:{port}")
        };

        Ok(Self {
            scheme: scheme.to_owned(),
            host,
            port,
            base_path: normalize_base_path(path),
            host_header,
        })
    }

    fn target_for_request_path(&self, request_path: &str) -> String {
        join_paths(&self.base_path, request_path)
    }

    fn target_url_for_request_path(&self, request_path: &str) -> String {
        format!(
            "{}://{}{}",
            self.scheme,
            self.host_header,
            self.target_for_request_path(request_path)
        )
    }

    fn websocket_url_for_request_path(&self, request_path: &str) -> String {
        let scheme = if self.scheme == "https" { "wss" } else { "ws" };
        format!(
            "{}://{}{}",
            scheme,
            self.host_header,
            self.target_for_request_path(request_path)
        )
    }
}

fn parse_authority(authority: &str, default_port: u16) -> Result<(String, u16), ProxyRuntimeError> {
    if authority.starts_with('[') {
        let Some(host_end) = authority.find(']') else {
            return Err(ProxyRuntimeError::MissingUpstreamHost {
                upstream_url: format!("http://{authority}"),
            });
        };
        let host = authority[1..host_end].to_owned();
        let port = authority
            .get(host_end + 1..)
            .and_then(|suffix| suffix.strip_prefix(':'))
            .map(parse_port)
            .transpose()?
            .unwrap_or(default_port);
        return Ok((host, port));
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), parse_port(port)?),
        None => (authority.to_owned(), default_port),
    };

    if host.is_empty() {
        return Err(ProxyRuntimeError::MissingUpstreamHost {
            upstream_url: format!("http://{authority}"),
        });
    }

    Ok((host, port))
}

fn parse_port(port: &str) -> Result<u16, ProxyRuntimeError> {
    port.parse()
        .map_err(|_| ProxyRuntimeError::InvalidUpstreamPort {
            port: port.to_owned(),
        })
}

fn normalize_base_path(path: &str) -> String {
    let trimmed = path.trim_matches('/');

    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

fn join_paths(base_path: &str, request_path: &str) -> String {
    if base_path.is_empty() {
        return request_path.to_owned();
    }

    let (path, query) = request_path
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((request_path, None));
    let base = base_path.trim_end_matches('/');
    let request = path.trim_start_matches('/');
    let base_last_segment = base.rsplit('/').next().unwrap_or_default();
    let request_without_duplicate = request
        .strip_prefix(base_last_segment)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .unwrap_or(request);
    let joined = if request_without_duplicate.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{request_without_duplicate}")
    };

    query
        .map(|query| format!("{joined}?{query}"))
        .unwrap_or(joined)
}

fn build_upstream_request(
    raw_request: &str,
    upstream: &ParsedUpstream,
) -> Result<Vec<u8>, ProxyRuntimeError> {
    let request = parse_http_request(raw_request)?;
    let target = upstream.target_for_request_path(&request.path);
    let mut output = Vec::new();

    write!(output, "{} {} HTTP/1.1\r\n", request.method, target)
        .expect("writing to Vec cannot fail");

    let mut saw_host = false;
    let mut saw_connection = false;
    let mut saw_content_length = false;

    for (name, value) in &request.headers {
        if name.eq_ignore_ascii_case("host") {
            saw_host = true;
            write!(output, "Host: {}\r\n", upstream.host_header)
                .expect("writing to Vec cannot fail");
        } else if name.eq_ignore_ascii_case("connection") {
            saw_connection = true;
            output.extend_from_slice(b"Connection: close\r\n");
        } else if name.eq_ignore_ascii_case("proxy-connection") {
            continue;
        } else if name.eq_ignore_ascii_case("content-length") {
            saw_content_length = true;
            write!(output, "Content-Length: {}\r\n", request.body.len())
                .expect("writing to Vec cannot fail");
        } else {
            write!(output, "{}: {}\r\n", name, value).expect("writing to Vec cannot fail");
        }
    }

    if !saw_host {
        write!(output, "Host: {}\r\n", upstream.host_header).expect("writing to Vec cannot fail");
    }
    if !saw_connection {
        output.extend_from_slice(b"Connection: close\r\n");
    }
    if !request.body.is_empty() && !saw_content_length {
        write!(output, "Content-Length: {}\r\n", request.body.len())
            .expect("writing to Vec cannot fail");
    }

    output.extend_from_slice(b"\r\n");
    output.extend_from_slice(request.body.as_bytes());

    Ok(output)
}

fn send_upstream_request(upstream: &ParsedUpstream, request: &[u8]) -> io::Result<Vec<u8>> {
    if upstream.scheme == "https" {
        return send_https_upstream_request(upstream, request);
    }

    let mut stream = TcpStream::connect((upstream.host.as_str(), upstream.port))?;
    stream.write_all(request)?;
    stream.flush()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response)
}

fn send_https_upstream_request(upstream: &ParsedUpstream, request: &[u8]) -> io::Result<Vec<u8>> {
    let raw_request = String::from_utf8_lossy(request);
    let parsed = parse_http_request(&raw_request)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let url = upstream.target_url_for_request_path(&parsed.path);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .map_err(reqwest_io_error)?;
    let method = reqwest::Method::from_bytes(parsed.method.as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let mut request_builder = client.request(method, url);

    for (name, value) in parsed.headers {
        if name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("connection")
            || name.eq_ignore_ascii_case("proxy-connection")
            || name.eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        request_builder = request_builder.header(name, value);
    }

    let response = request_builder
        .body(parsed.body)
        .send()
        .map_err(reqwest_io_error)?;
    render_reqwest_response(response)
}

fn render_reqwest_response(response: reqwest::blocking::Response) -> io::Result<Vec<u8>> {
    let status = response.status();
    let mut output = Vec::new();
    write!(
        output,
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )?;

    let headers = response.headers().clone();
    let body = response.bytes().map_err(reqwest_io_error)?;
    let mut saw_content_length = false;

    for (name, value) in &headers {
        if name.as_str().eq_ignore_ascii_case("connection")
            || name.as_str().eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        if name.as_str().eq_ignore_ascii_case("content-length") {
            saw_content_length = true;
        }
        let Ok(value) = value.to_str() else {
            continue;
        };
        write!(output, "{}: {}\r\n", name.as_str(), value)?;
    }

    if !saw_content_length {
        write!(output, "content-length: {}\r\n", body.len())?;
    }
    output.extend_from_slice(b"connection: close\r\n\r\n");
    output.extend_from_slice(&body);
    Ok(output)
}

fn reqwest_io_error(error: reqwest::Error) -> io::Error {
    io::Error::other(redact_provider_error(&error.to_string()))
}

fn response_body_string(response_bytes: &[u8]) -> String {
    let body_start = response_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            response_bytes
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
        .unwrap_or(0);

    String::from_utf8_lossy(&response_bytes[body_start..]).into_owned()
}

fn write_proxy_error_response(mut stream: impl Write, error: ProxyRuntimeError) -> io::Result<()> {
    let response_body = format!(
        "{{\"status\":\"bad_gateway\",\"error\":\"{}\"}}\n",
        escape_json_string(&redact_provider_error(&error.to_string()))
    );

    write!(
        stream,
        "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    )
}

fn core_events_from_proxy_events(
    events: &[AttributionEvent],
    usage: ProviderUsage,
    timestamp_ms: u64,
    adapter: &str,
) -> Vec<AttributedTokenEvent> {
    if events.is_empty() {
        return usage
            .has_any_usage()
            .then(|| {
                let mode = ModeState::passive();
                AttributedTokenEvent {
                    timestamp_ms,
                    mode: mode.mode,
                    run_id: "proxy-passive".to_owned(),
                    task_id: mode.task_id,
                    profile_id: mode.profile_id,
                    adapter: adapter.to_owned(),
                    operation_class: OperationClass::Other,
                    tool: "proxy.usage".to_owned(),
                    tokens: token_counts_from_usage(usage),
                    byte_count: 0,
                    content_digest: digest_bytes(b"proxy:usage"),
                    repeat_of: None,
                    action_subtype: None,
                    direction: None,
                }
            })
            .into_iter()
            .collect();
    }

    events
        .iter()
        .enumerate()
        .map(|(index, event)| AttributedTokenEvent {
            timestamp_ms: timestamp_ms.saturating_add(index as u64),
            mode: event.mode,
            run_id: format!("proxy-{}", event.task_id),
            task_id: event.task_id.clone(),
            profile_id: event.profile_id.clone(),
            adapter: adapter.to_owned(),
            operation_class: OperationClass::from_str_or_other(&event.op_class),
            tool: safe_event_tool(&event.tool),
            tokens: event
                .token_allocation
                .as_ref()
                .map(|allocation| {
                    TokenCounts::new(
                        allocation.input_tokens,
                        allocation.output_tokens,
                        allocation.cache_read_tokens,
                        allocation.cache_write_tokens,
                    )
                })
                .unwrap_or_else(|| token_counts_from_usage(usage)),
            byte_count: event.byte_count,
            content_digest: digest_bytes(
                format!("proxy:{}:{}:{}", event.task_id, event.op_class, index).as_bytes(),
            ),
            repeat_of: None,
            action_subtype: None,
            direction: Some("response".to_owned()),
        })
        .collect()
}

fn token_counts_from_usage(usage: ProviderUsage) -> TokenCounts {
    TokenCounts::new(
        usage.input_tokens.unwrap_or(0),
        usage.output_tokens.unwrap_or(0),
        usage.cache_read_tokens.unwrap_or(0),
        usage.cache_write_tokens.unwrap_or(0),
    )
}

fn safe_event_tool(tool: &str) -> String {
    let sanitized: String = tool
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.trim_matches('-').is_empty() {
        "proxy.tool".to_owned()
    } else {
        sanitized
    }
}

fn render_core_event_log_records(
    events: &[AttributedTokenEvent],
) -> Result<Vec<u8>, ProxyRuntimeError> {
    let mut records = Vec::new();

    for event in events {
        EventLog::append_event(&mut records, event)?;
    }

    Ok(records)
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(1)
        .max(1)
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
    fn upstream_request_builder_and_capture_preserve_usage() {
        let response_body = r#"{"usage":{"input_tokens":44,"output_tokens":11,"cache_read_tokens":8,"cache_write_tokens":2},"content":[{"type":"tool_result","tool_use_id":"call_1","content":"PRIVATE_CONTENT_FIXTURE","input_tokens":44,"output_tokens":11,"cache_read_tokens":8,"cache_write_tokens":2}]}"#;
        let upstream = ParsedUpstream::parse("http://127.0.0.1:43210/provider").unwrap();
        let request_body = r#"{"messages":[{"tool_calls":[{"id":"call_1","type":"tool_call","name":"Bash","arguments":"git status --short"}]}]}"#;
        let request = format!(
            concat!(
                "POST /v1/responses?api_key=PRIVATE_CREDENTIAL_FIXTURE HTTP/1.1\r\n",
                "Host: proxy.local\r\n",
                "Authorization: Bearer sk-live-forward-secret\r\n",
                "Content-Type: application/json\r\n",
                "Content-Length: {}\r\n",
                "\r\n",
                "{}"
            ),
            request_body.len(),
            request_body
        );

        let upstream_request =
            String::from_utf8(build_upstream_request(&request, &upstream).unwrap()).unwrap();
        let capture = process_proxy_exchange(&request, response_body).unwrap();

        assert!(
            upstream_request.contains(
                "POST /provider/v1/responses?api_key=PRIVATE_CREDENTIAL_FIXTURE HTTP/1.1"
            )
        );
        assert!(upstream_request.contains("Host: 127.0.0.1:43210"));
        assert!(upstream_request.contains("Authorization: Bearer sk-live-forward-secret"));
        assert_eq!(capture.path, "/v1/responses?[REDACTED_QUERY]");
        assert_eq!(capture.usage.input_tokens, Some(44));
        assert_eq!(capture.usage.output_tokens, Some(11));
        assert_eq!(capture.usage.cache_read_tokens, Some(8));
        assert_eq!(capture.usage.cache_write_tokens, Some(2));
        assert_eq!(capture.events, Vec::new());
        assert_eq!(capture.core_events.len(), 1);
        assert_eq!(capture.core_events[0].tokens.input_tokens, 44);
        assert_eq!(capture.core_events[0].tokens.cache_read_tokens, 8);
        assert_eq!(
            capture.core_events[0].operation_class,
            crate::core::OperationClass::Other
        );

        let parsed =
            crate::core::EventLog::read_from(capture.event_log_records.as_slice()).unwrap();
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(
            parsed.events[0].operation_class,
            crate::core::OperationClass::Other
        );
    }

    #[test]
    fn https_upstream_defaults_to_tls_port_and_builds_target_url() {
        let upstream = ParsedUpstream::parse("https://api.openai.com/v1").unwrap();

        assert_eq!(upstream.scheme, "https");
        assert_eq!(upstream.host, "api.openai.com");
        assert_eq!(upstream.port, 443);
        assert_eq!(upstream.host_header, "api.openai.com");
        assert_eq!(
            upstream.target_url_for_request_path("/responses"),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn upstream_path_join_avoids_duplicate_api_version_segment() {
        let upstream = ParsedUpstream::parse("https://api.openai.com/v1").unwrap();

        assert_eq!(
            upstream.target_url_for_request_path("/v1/responses"),
            "https://api.openai.com/v1/responses"
        );
        assert_eq!(
            upstream.target_url_for_request_path("/v1/responses?stream=true"),
            "https://api.openai.com/v1/responses?stream=true"
        );
    }

    #[test]
    fn websocket_upgrade_requests_are_detected_case_insensitively() {
        let request = concat!(
            "GET /v1/responses HTTP/1.1\r\n",
            "Host: 127.0.0.1:17683\r\n",
            "Connection: Upgrade\r\n",
            "Upgrade: websocket\r\n",
            "Sec-WebSocket-Key: fixture\r\n",
            "\r\n"
        );

        assert!(is_websocket_upgrade_request(request));
    }

    #[test]
    fn websocket_upstream_url_uses_wss_for_https_upstream() {
        let upstream = ParsedUpstream::parse("https://api.openai.com/v1").unwrap();

        assert_eq!(
            upstream.websocket_url_for_request_path("/v1/responses"),
            "wss://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn websocket_usage_capture_emits_proxy_ws_event_without_content() {
        let path = std::env::temp_dir().join(format!(
            "tokmeter-ws-{}-{}.jsonl",
            std::process::id(),
            current_timestamp_ms()
        ));
        let config = ProxyConfig::new("127.0.0.1", 17683, "https://api.openai.com")
            .unwrap()
            .with_event_log_path(&path);
        let message = Message::Text(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":31,"output_tokens":7},"output_text":"PRIVATE_CONTENT_FIXTURE"}}"#
                .into(),
        );

        capture_websocket_message(&message, &config, EstimatedProxyDirection::Output).unwrap();

        let persisted = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(persisted.contains("adapter=proxy.ws"));
        assert!(persisted.contains("input_tokens=31"));
        assert!(persisted.contains("output_tokens=7"));
        assert!(!persisted.contains("PRIVATE_CONTENT_FIXTURE"));
    }

    #[test]
    fn websocket_missing_usage_emits_estimated_proxy_event_without_content() {
        let path = std::env::temp_dir().join(format!(
            "tokmeter-ws-estimated-{}-{}.jsonl",
            std::process::id(),
            current_timestamp_ms()
        ));
        let config = ProxyConfig::new("127.0.0.1", 17683, "https://chatgpt.com/backend-api")
            .unwrap()
            .with_event_log_path(&path);
        let message = Message::Text(
            r#"{"type":"delta","message":{"content":"PRIVATE_SUBSCRIPTION_CONTENT_FIXTURE with enough text to estimate tokens"}}"#
                .into(),
        );

        capture_websocket_message(&message, &config, EstimatedProxyDirection::Output).unwrap();

        let persisted = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(persisted.contains("adapter=proxy.ws.estimated"));
        assert!(persisted.contains("tool=proxy.ws.response.estimated"));
        assert!(persisted.contains("output_tokens="));
        assert!(!persisted.contains("output_tokens=0"));
        assert!(!persisted.contains("PRIVATE_SUBSCRIPTION_CONTENT_FIXTURE"));
    }

    #[test]
    fn http_missing_usage_emits_estimated_proxy_event_without_content() {
        let request = concat!("POST /backend-api/conversation HTTP/1.1\r\n", "\r\n", "{}");
        let response = r#"{"message":{"content":"PRIVATE_HTTP_CONTENT_FIXTURE with enough text to estimate tokens"}}"#;

        let capture = process_proxy_exchange(request, response).unwrap();
        let persisted = String::from_utf8(capture.event_log_records).unwrap();

        assert_eq!(capture.core_events.len(), 1);
        assert_eq!(capture.core_events[0].adapter, "proxy.estimated");
        assert!(capture.core_events[0].tokens.output_tokens > 0);
        assert!(persisted.contains("adapter=proxy.estimated"));
        assert!(!persisted.contains("PRIVATE_HTTP_CONTENT_FIXTURE"));
    }

    #[test]
    fn websocket_upgrade_response_uses_consumed_request_handshake() {
        let request = parse_http_request(concat!(
            "GET /v1/responses HTTP/1.1\r\n",
            "Host: 127.0.0.1:17683\r\n",
            "Connection: Upgrade\r\n",
            "Upgrade: websocket\r\n",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
            "\r\n"
        ))
        .unwrap();
        let mut upstream_headers = tungstenite::http::HeaderMap::new();
        upstream_headers.insert(
            "sec-websocket-protocol",
            tungstenite::http::HeaderValue::from_static("fixture-protocol"),
        );
        let mut response = Vec::new();

        write_websocket_upgrade_response(&mut response, &request, &upstream_headers).unwrap();

        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        assert!(response.contains("Sec-WebSocket-Protocol: fixture-protocol\r\n"));
    }

    #[test]
    fn proxy_config_can_carry_event_log_path_for_runtime_persistence() {
        let config = ProxyConfig::new("127.0.0.1", 17683, "https://api.openai.com/v1")
            .unwrap()
            .with_event_log_path("/tmp/tokmeter-proxy-events.jsonl");

        assert_eq!(
            config.event_log_path,
            Some(std::path::PathBuf::from("/tmp/tokmeter-proxy-events.jsonl"))
        );
    }

    #[test]
    fn proxy_config_can_label_exact_usage_for_specific_adapter() {
        let config = ProxyConfig::new("127.0.0.1", 17684, "https://api.anthropic.com")
            .unwrap()
            .with_adapter_label("proxy.claude.anthropic");

        assert_eq!(config.adapter_label, "proxy.claude.anthropic");
    }

    #[test]
    fn custom_adapter_label_is_used_for_exact_http_usage_events() {
        let request = concat!("POST /v1/messages HTTP/1.1\r\n", "\r\n", "{}");
        let response = r#"{"usage":{"input_tokens":17,"output_tokens":5}}"#;

        let capture = process_proxy_exchange_with_mode_and_adapter(
            request,
            response,
            &ModeState::passive(),
            "proxy.claude.anthropic",
        )
        .unwrap();

        assert_eq!(capture.core_events.len(), 1);
        assert_eq!(capture.core_events[0].adapter, "proxy.claude.anthropic");
        assert_eq!(capture.core_events[0].tokens.input_tokens, 17);
        assert_eq!(capture.core_events[0].tokens.output_tokens, 5);
    }

    #[test]
    fn forwarded_proxy_capture_does_not_persist_credentials_or_content() {
        let response_body = r#"{"usage":{"input_tokens":7,"output_tokens":3},"content":[{"type":"tool_result","tool":"Read","content":"SECRET_RAW_TOOL_OUTPUT"}],"authorization":"Bearer sk-response-secret"}"#;
        let request_body =
            r#"{"prompt":"PROMPT_SHOULD_NOT_PERSIST","content":"CONTENT_SHOULD_NOT_PERSIST"}"#;
        let request = format!(
            concat!(
                "POST /v1/messages?api_key=PRIVATE_CREDENTIAL_FIXTURE HTTP/1.1\r\n",
                "Host: proxy.local\r\n",
                "Authorization: Bearer sk-live-forward-secret\r\n",
                "X-API-KEY: TOKMETER_FIXTURE_SECRET\r\n",
                "Content-Length: {}\r\n",
                "\r\n",
                "{}"
            ),
            request_body.len(),
            request_body
        );

        let capture = process_proxy_exchange(&request, response_body).unwrap();
        let persisted = format!(
            "{:?}\n{}\n{}",
            capture,
            capture.persisted_log_line,
            String::from_utf8(capture.event_log_records.clone()).unwrap()
        );
        let failures = crate::proxy_privacy::scan_persisted_log_string(&persisted);

        assert_eq!(failures, Vec::new());
        assert!(!persisted.contains("sk-live-forward-secret"));
        assert!(!persisted.contains("TOKMETER_FIXTURE_SECRET"));
        assert!(!persisted.contains("PROMPT_SHOULD_NOT_PERSIST"));
        assert!(!persisted.contains("CONTENT_SHOULD_NOT_PERSIST"));
        assert!(!persisted.contains("SECRET_RAW_TOOL_OUTPUT"));
    }

    #[test]
    fn proxy_capture_emits_unattributed_core_event_when_usage_has_no_tool_result() {
        let request = "POST /v1/responses HTTP/1.1\r\n\r\n{\"input\":\"PRIVATE_PROMPT_FIXTURE\"}";
        let response = r#"{"usage":{"input_tokens":5,"output_tokens":2}}"#;

        let capture = process_proxy_exchange(request, response).unwrap();

        assert_eq!(capture.events, Vec::new());
        assert_eq!(capture.core_events.len(), 1);
        assert_eq!(
            capture.core_events[0].operation_class,
            crate::core::OperationClass::Other
        );
        assert_eq!(capture.core_events[0].tokens.input_tokens, 5);
        assert_eq!(capture.core_events[0].tokens.output_tokens, 2);
        assert!(!capture.event_log_records.is_empty());
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
