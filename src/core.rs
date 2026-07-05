use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::str::FromStr;

pub const EVENT_LOG_SCHEMA_VERSION: u32 = 2;
pub const EVENT_LOG_SCHEMA_MAJOR_VERSION: u32 = 1;
pub const ADHOC_TASK_ID: &str = "adhoc";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationClass {
    VcStatus,
    VcDiff,
    VcLog,
    VcShow,
    VcBranchOps,
    VcPushPull,
    FileRead,
    FileSearch,
    FileList,
    EditEcho,
    TestOutput,
    BuildOutput,
    SessionMeta,
    Other,
}

impl OperationClass {
    pub const ALL: [OperationClass; 14] = [
        OperationClass::VcStatus,
        OperationClass::VcDiff,
        OperationClass::VcLog,
        OperationClass::VcShow,
        OperationClass::VcBranchOps,
        OperationClass::VcPushPull,
        OperationClass::FileRead,
        OperationClass::FileSearch,
        OperationClass::FileList,
        OperationClass::EditEcho,
        OperationClass::TestOutput,
        OperationClass::BuildOutput,
        OperationClass::SessionMeta,
        OperationClass::Other,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            OperationClass::VcStatus => "vc.status",
            OperationClass::VcDiff => "vc.diff",
            OperationClass::VcLog => "vc.log",
            OperationClass::VcShow => "vc.show",
            OperationClass::VcBranchOps => "vc.branch_ops",
            OperationClass::VcPushPull => "vc.push_pull",
            OperationClass::FileRead => "file.read",
            OperationClass::FileSearch => "file.search",
            OperationClass::FileList => "file.list",
            OperationClass::EditEcho => "edit.echo",
            OperationClass::TestOutput => "test.output",
            OperationClass::BuildOutput => "build.output",
            OperationClass::SessionMeta => "session.meta",
            OperationClass::Other => "other",
        }
    }

    pub fn from_str_or_other(value: &str) -> Self {
        OperationClass::from_str(value).unwrap_or(OperationClass::Other)
    }
}

impl fmt::Display for OperationClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for OperationClass {
    type Err = OperationClassParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "vc.status" => Ok(OperationClass::VcStatus),
            "vc.diff" => Ok(OperationClass::VcDiff),
            "vc.log" => Ok(OperationClass::VcLog),
            "vc.show" => Ok(OperationClass::VcShow),
            "vc.branch_ops" => Ok(OperationClass::VcBranchOps),
            "vc.push_pull" => Ok(OperationClass::VcPushPull),
            "file.read" => Ok(OperationClass::FileRead),
            "file.search" => Ok(OperationClass::FileSearch),
            "file.list" => Ok(OperationClass::FileList),
            "edit.echo" => Ok(OperationClass::EditEcho),
            "test.output" => Ok(OperationClass::TestOutput),
            "build.output" => Ok(OperationClass::BuildOutput),
            "session.meta" => Ok(OperationClass::SessionMeta),
            "other" => Ok(OperationClass::Other),
            _ => Err(OperationClassParseError {
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationClassParseError {
    value: String,
}

impl fmt::Display for OperationClassParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown operation class '{}'", self.value)
    }
}

impl Error for OperationClassParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureMode {
    Passive,
    Task,
}

impl CaptureMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Task => "task",
        }
    }
}

impl Default for CaptureMode {
    fn default() -> Self {
        Self::Passive
    }
}

impl fmt::Display for CaptureMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CaptureMode {
    type Err = CaptureModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "passive" => Ok(Self::Passive),
            "task" => Ok(Self::Task),
            _ => Err(CaptureModeParseError {
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureModeParseError {
    value: String,
}

impl fmt::Display for CaptureModeParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown capture mode '{}'", self.value)
    }
}

impl Error for CaptureModeParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenCounts {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl TokenCounts {
    pub const fn new(
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        }
    }

    pub fn total(&self) -> Option<u64> {
        self.input_tokens
            .checked_add(self.output_tokens)?
            .checked_add(self.cache_read_tokens)?
            .checked_add(self.cache_write_tokens)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        self.total()
            .ok_or_else(|| ValidationError::new("tokens", "token bucket totals overflow u64"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedTokenEvent {
    pub timestamp_ms: u64,
    pub mode: CaptureMode,
    pub run_id: String,
    pub task_id: String,
    pub profile_id: String,
    pub adapter: String,
    pub operation_class: OperationClass,
    pub tool: String,
    pub tokens: TokenCounts,
    pub byte_count: u64,
    pub content_digest: String,
    pub repeat_of: Option<String>,
    pub action_subtype: Option<String>,
    pub direction: Option<String>,
}

impl AttributedTokenEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        require_non_zero("timestamp_ms", self.timestamp_ms)?;
        require_not_blank("run_id", &self.run_id)?;
        require_not_blank("task_id", &self.task_id)?;
        require_not_blank("profile_id", &self.profile_id)?;
        require_not_blank("adapter", &self.adapter)?;
        require_not_blank("tool", &self.tool)?;
        require_not_blank("content_digest", &self.content_digest)?;

        if let Some(repeat_of) = &self.repeat_of {
            require_not_blank("repeat_of", repeat_of)?;
        }
        validate_optional_label("action_subtype", self.action_subtype.as_deref())?;
        validate_optional_label("direction", self.direction.as_deref())?;

        self.tokens.validate()?;
        let total_tokens = self.tokens.total().unwrap_or(0);
        if total_tokens == 0 && self.byte_count == 0 {
            return Err(ValidationError::new(
                "tokens",
                "at least one token bucket or byte_count must be greater than zero",
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLog {
    pub events: Vec<AttributedTokenEvent>,
}

impl EventLog {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn push(&mut self, event: AttributedTokenEvent) -> Result<(), ValidationError> {
        event.validate()?;
        self.events.push(event);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        for event in &self.events {
            event.validate()?;
        }
        Ok(())
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), EventLogError> {
        for event in &self.events {
            Self::append_event(writer, event)?;
        }
        Ok(())
    }

    pub fn append_event<W: Write>(
        writer: &mut W,
        event: &AttributedTokenEvent,
    ) -> Result<(), EventLogError> {
        event.validate().map_err(EventLogError::Validation)?;
        writer.write_all(serialize_event(event).as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(())
    }

    pub fn read_from<R: BufRead>(reader: R) -> Result<Self, EventLogError> {
        let mut events = Vec::new();

        for (index, line_result) in reader.lines().enumerate() {
            let line_number = index + 1;
            let line = line_result?;
            let event = parse_event_line(&line, line_number)?;
            event.validate().map_err(EventLogError::Validation)?;
            events.push(event);
        }

        Ok(Self { events })
    }

    pub fn append_event_to_file<P: AsRef<Path>>(
        path: P,
        event: &AttributedTokenEvent,
    ) -> Result<(), EventLogError> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        Self::append_event(&mut file, event)
    }

    pub fn read_file<P: AsRef<Path>>(path: P) -> Result<Self, EventLogError> {
        let file = File::open(path)?;
        Self::read_from(BufReader::new(file))
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

pub fn migrate_event_log_to_latest(log: &EventLog) -> EventLog {
    EventLog {
        events: log
            .events
            .iter()
            .cloned()
            .map(|mut event| {
                if event.mode == CaptureMode::Passive && event.task_id.trim().is_empty() {
                    event.task_id = ADHOC_TASK_ID.to_owned();
                }
                event
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: &'static str,
    pub message: String,
}

impl ValidationError {
    pub fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

impl Error for ValidationError {}

#[derive(Debug)]
pub enum EventLogError {
    Io(io::Error),
    Parse { line: usize, message: String },
    Validation(ValidationError),
}

impl fmt::Display for EventLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventLogError::Io(error) => write!(formatter, "I/O error: {}", error),
            EventLogError::Parse { line, message } => {
                write!(formatter, "line {}: {}", line, message)
            }
            EventLogError::Validation(error) => write!(formatter, "validation error: {}", error),
        }
    }
}

impl Error for EventLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EventLogError::Io(error) => Some(error),
            EventLogError::Validation(error) => Some(error),
            EventLogError::Parse { .. } => None,
        }
    }
}

impl From<io::Error> for EventLogError {
    fn from(error: io::Error) -> Self {
        EventLogError::Io(error)
    }
}

fn serialize_event(event: &AttributedTokenEvent) -> String {
    let repeat_of = event.repeat_of.as_deref().unwrap_or("");
    let action_subtype = event.action_subtype.as_deref().unwrap_or("");
    let direction = event.direction.as_deref().unwrap_or("");
    IntoIterator::into_iter([
        ("schema", EVENT_LOG_SCHEMA_VERSION.to_string()),
        ("timestamp_ms", event.timestamp_ms.to_string()),
        ("mode", event.mode.as_str().to_owned()),
        ("run_id", escape_value(&event.run_id)),
        ("task_id", escape_value(&event.task_id)),
        ("profile_id", escape_value(&event.profile_id)),
        ("adapter", escape_value(&event.adapter)),
        ("op_class", event.operation_class.as_str().to_owned()),
        ("tool", escape_value(&event.tool)),
        ("input_tokens", event.tokens.input_tokens.to_string()),
        ("output_tokens", event.tokens.output_tokens.to_string()),
        (
            "cache_read_tokens",
            event.tokens.cache_read_tokens.to_string(),
        ),
        (
            "cache_write_tokens",
            event.tokens.cache_write_tokens.to_string(),
        ),
        ("byte_count", event.byte_count.to_string()),
        ("digest", escape_value(&event.content_digest)),
        ("repeat_of", escape_value(repeat_of)),
        ("action_subtype", escape_value(action_subtype)),
        ("direction", escape_value(direction)),
    ])
    .map(|(key, value)| {
        let mut field = String::with_capacity(key.len() + value.len() + 1);
        field.push_str(key);
        field.push('=');
        field.push_str(&value);
        field
    })
    .collect::<Vec<_>>()
    .join("\t")
}

fn parse_event_line(line: &str, line_number: usize) -> Result<AttributedTokenEvent, EventLogError> {
    if line.is_empty() {
        return Err(parse_error(line_number, "empty log record"));
    }

    let mut fields = HashMap::new();
    for field in line.split('\t') {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| parse_error(line_number, "field is missing '=' separator"))?;

        if fields.insert(key, value).is_some() {
            return Err(parse_error(
                line_number,
                format!("duplicate field '{}'", key),
            ));
        }
    }

    let schema = parse_u32(
        field(&fields, "schema", line_number)?,
        "schema",
        line_number,
    )?;
    if schema == 0 {
        return Err(parse_error(line_number, "schema must be greater than zero"));
    }
    let schema_major = schema_major_version(schema);
    if schema_major > EVENT_LOG_SCHEMA_MAJOR_VERSION {
        return Err(parse_error(
            line_number,
            format!(
                "schema major {} is newer than supported major {}",
                schema_major, EVENT_LOG_SCHEMA_MAJOR_VERSION
            ),
        ));
    }
    if schema > EVENT_LOG_SCHEMA_VERSION {
        return Err(parse_error(
            line_number,
            format!(
                "schema {} is newer than supported schema {}",
                schema, EVENT_LOG_SCHEMA_VERSION
            ),
        ));
    }

    let repeat_of = optional_string_field(&fields, "repeat_of", line_number)?;
    let mode = if schema == 1 {
        CaptureMode::Passive
    } else {
        field(&fields, "mode", line_number)?
            .parse()
            .map_err(|error: CaptureModeParseError| parse_error(line_number, error.to_string()))?
    };

    Ok(AttributedTokenEvent {
        timestamp_ms: parse_u64(
            field(&fields, "timestamp_ms", line_number)?,
            "timestamp_ms",
            line_number,
        )?,
        mode,
        run_id: string_field(&fields, "run_id", line_number)?,
        task_id: string_field(&fields, "task_id", line_number)?,
        profile_id: string_field(&fields, "profile_id", line_number)?,
        adapter: string_field(&fields, "adapter", line_number)?,
        operation_class: field(&fields, "op_class", line_number)?.parse().map_err(
            |error: OperationClassParseError| parse_error(line_number, error.to_string()),
        )?,
        tool: string_field(&fields, "tool", line_number)?,
        tokens: TokenCounts {
            input_tokens: parse_u64(
                field(&fields, "input_tokens", line_number)?,
                "input_tokens",
                line_number,
            )?,
            output_tokens: parse_u64(
                field(&fields, "output_tokens", line_number)?,
                "output_tokens",
                line_number,
            )?,
            cache_read_tokens: parse_u64(
                field(&fields, "cache_read_tokens", line_number)?,
                "cache_read_tokens",
                line_number,
            )?,
            cache_write_tokens: parse_u64(
                field(&fields, "cache_write_tokens", line_number)?,
                "cache_write_tokens",
                line_number,
            )?,
        },
        byte_count: parse_u64(
            field(&fields, "byte_count", line_number)?,
            "byte_count",
            line_number,
        )?,
        content_digest: string_field(&fields, "digest", line_number)?,
        repeat_of,
        action_subtype: optional_string_field(&fields, "action_subtype", line_number)?,
        direction: optional_string_field(&fields, "direction", line_number)?,
    })
}

fn schema_major_version(schema: u32) -> u32 {
    if schema < 1000 { 1 } else { schema / 1000 }
}

fn field<'a>(
    fields: &'a HashMap<&str, &str>,
    key: &'static str,
    line_number: usize,
) -> Result<&'a str, EventLogError> {
    fields
        .get(key)
        .copied()
        .ok_or_else(|| parse_error(line_number, format!("missing required field '{}'", key)))
}

fn string_field(
    fields: &HashMap<&str, &str>,
    key: &'static str,
    line_number: usize,
) -> Result<String, EventLogError> {
    unescape_value(field(fields, key, line_number)?)
        .map_err(|message| parse_error(line_number, format!("{}: {}", key, message)))
}

fn optional_string_field(
    fields: &HashMap<&str, &str>,
    key: &'static str,
    line_number: usize,
) -> Result<Option<String>, EventLogError> {
    match fields.get(key).copied() {
        Some("") | None => Ok(None),
        Some(value) => unescape_value(value)
            .map(Some)
            .map_err(|message| parse_error(line_number, format!("{}: {}", key, message))),
    }
}

fn parse_u32(value: &str, field: &'static str, line_number: usize) -> Result<u32, EventLogError> {
    value.parse().map_err(|_| {
        parse_error(
            line_number,
            format!("{} must be an unsigned integer", field),
        )
    })
}

fn parse_u64(value: &str, field: &'static str, line_number: usize) -> Result<u64, EventLogError> {
    value.parse().map_err(|_| {
        parse_error(
            line_number,
            format!("{} must be an unsigned integer", field),
        )
    })
}

fn parse_error(line: usize, message: impl Into<String>) -> EventLogError {
    EventLogError::Parse {
        line,
        message: message.into(),
    }
}

fn require_non_zero(field: &'static str, value: u64) -> Result<(), ValidationError> {
    if value == 0 {
        Err(ValidationError::new(field, "must be greater than zero"))
    } else {
        Ok(())
    }
}

fn require_not_blank(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::new(field, "is required"))
    } else {
        Ok(())
    }
}

fn validate_optional_label(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), ValidationError> {
    let Some(value) = value else {
        return Ok(());
    };
    require_not_blank(field, value)?;
    if value.chars().all(is_label_char) {
        Ok(())
    } else {
        Err(ValidationError::new(
            field,
            "must contain only ASCII letters, digits, '.', '_', ':', '/', or '-'",
        ))
    }
}

fn is_label_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '/' | '-')
}

fn escape_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' | b':' | b'/' => {
                escaped.push(byte as char)
            }
            _ => {
                escaped.push('%');
                escaped.push(hex_digit(byte >> 4));
                escaped.push(hex_digit(byte & 0x0f));
            }
        }
    }
    escaped
}

fn unescape_value(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("percent escape is truncated".to_owned());
            }

            let high = hex_value(bytes[index + 1])
                .ok_or_else(|| "percent escape contains a non-hex digit".to_owned())?;
            let low = hex_value(bytes[index + 2])
                .ok_or_else(|| "percent escape contains a non-hex digit".to_owned())?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).map_err(|_| "decoded value is not valid UTF-8".to_owned())
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!("hex digit input must be less than 16"),
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_event() -> AttributedTokenEvent {
        AttributedTokenEvent {
            timestamp_ms: 1_725_000_123_456,
            mode: CaptureMode::Task,
            run_id: "run-123".to_owned(),
            task_id: "task with tab\tand newline\n".to_owned(),
            profile_id: "default".to_owned(),
            adapter: "codex-cli".to_owned(),
            operation_class: OperationClass::VcDiff,
            tool: "git diff".to_owned(),
            tokens: TokenCounts::new(120, 45, 10, 3),
            byte_count: 4096,
            content_digest: "sha256:abcdef0123456789".to_owned(),
            repeat_of: Some("event-001".to_owned()),
            action_subtype: Some("git.diff".to_owned()),
            direction: Some("response".to_owned()),
        }
    }

    #[test]
    fn operation_class_taxonomy_strings_are_frozen() {
        let actual = OperationClass::ALL.map(OperationClass::as_str);

        assert_eq!(
            actual,
            [
                "vc.status",
                "vc.diff",
                "vc.log",
                "vc.show",
                "vc.branch_ops",
                "vc.push_pull",
                "file.read",
                "file.search",
                "file.list",
                "edit.echo",
                "test.output",
                "build.output",
                "session.meta",
                "other",
            ]
        );
    }

    #[test]
    fn operation_class_unknown_values_are_rejected_or_normalized() {
        assert!("vc.commit".parse::<OperationClass>().is_err());
        assert_eq!(
            OperationClass::from_str_or_other("vc.commit"),
            OperationClass::Other
        );
    }

    #[test]
    fn capture_mode_strings_are_frozen() {
        assert_eq!(CaptureMode::Passive.as_str(), "passive");
        assert_eq!(CaptureMode::Task.as_str(), "task");
        assert_eq!(
            "passive".parse::<CaptureMode>().unwrap(),
            CaptureMode::Passive
        );
        assert_eq!("task".parse::<CaptureMode>().unwrap(), CaptureMode::Task);
        assert!("adhoc".parse::<CaptureMode>().is_err());
    }

    #[test]
    fn validation_reports_required_fields() {
        let mut event = fixture_event();
        event.run_id.clear();

        let error = event.validate().unwrap_err();

        assert_eq!(error.field, "run_id");
        assert!(error.to_string().contains("is required"));
    }

    #[test]
    fn validation_requires_measured_tokens_or_bytes() {
        let mut event = fixture_event();
        event.tokens = TokenCounts::new(0, 0, 0, 0);
        event.byte_count = 0;

        let error = event.validate().unwrap_err();

        assert_eq!(error.field, "tokens");
        assert!(error.to_string().contains("at least one token bucket"));
    }

    #[test]
    fn validation_catches_token_total_overflow() {
        let tokens = TokenCounts::new(u64::MAX, 1, 0, 0);

        let error = tokens.validate().unwrap_err();

        assert_eq!(error.field, "tokens");
        assert!(error.to_string().contains("overflow"));
    }

    #[test]
    fn event_round_trips_through_line_format_without_content_fields() {
        let event = fixture_event();
        let line = serialize_event(&event);

        assert!(line.starts_with("schema=2\ttimestamp_ms="));
        assert!(line.contains("\tmode=task\t"));
        assert!(line.contains("\top_class=vc.diff\t"));
        assert!(!line.contains("\tcontent="));
        assert!(!line.contains("\tcontent_digest="));
        assert!(!line.contains("task with tab\tand newline\n"));

        let parsed = parse_event_line(&line, 1).unwrap();

        assert_eq!(parsed, event);
    }

    #[test]
    fn event_log_round_trips_multiple_records_in_memory() {
        let first = fixture_event();
        let mut second = fixture_event();
        second.timestamp_ms += 1;
        second.operation_class = OperationClass::FileSearch;
        second.repeat_of = None;

        let log = EventLog {
            events: vec![first.clone(), second.clone()],
        };
        let mut bytes = Vec::new();

        log.write_to(&mut bytes).unwrap();
        let read = EventLog::read_from(BufReader::new(bytes.as_slice())).unwrap();

        assert_eq!(read.events, vec![first, second]);
    }

    #[test]
    fn append_and_read_file_log() {
        let event = fixture_event();
        let path = std::env::temp_dir().join(format!(
            "vc-tokmeter-core-test-{}-{}.log",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        EventLog::append_event_to_file(&path, &event).unwrap();
        let read = EventLog::read_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(read.events, vec![event]);
    }

    #[test]
    fn parser_rejects_missing_required_fields_with_actionable_error() {
        let error = parse_event_line("schema=1\ttimestamp_ms=123", 7).unwrap_err();

        assert!(matches!(error, EventLogError::Parse { line: 7, .. }));
        assert!(
            error
                .to_string()
                .contains("missing required field 'run_id'")
        );
    }

    #[test]
    fn parser_rejects_newer_schema_versions() {
        let mut line = serialize_event(&fixture_event());
        line = line.replacen("schema=2", "schema=3", 1);

        let error = parse_event_line(&line, 1).unwrap_err();

        assert!(error.to_string().contains("newer than supported schema 2"));
    }

    #[test]
    fn parser_rejects_unknown_major_schema_versions() {
        let mut line = serialize_event(&fixture_event());
        line = line.replacen("schema=2", "schema=2000", 1);

        let error = parse_event_line(&line, 1).unwrap_err();

        assert!(error.to_string().contains("schema major 2"));
    }

    #[test]
    fn parser_backfills_schema_one_dev_events_as_passive() {
        let line = serialize_event(&fixture_event())
            .replacen("schema=2", "schema=1", 1)
            .replace("\tmode=task", "");

        let parsed = parse_event_line(&line, 1).unwrap();

        assert_eq!(parsed.mode, CaptureMode::Passive);
    }
}
