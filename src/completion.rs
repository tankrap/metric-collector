use std::error::Error;
use std::fmt;
use std::io::{self, BufRead, IsTerminal, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_COMPLETION_FALLBACK: CompletionStatus = CompletionStatus::Aborted;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus {
    Completed,
    Failed,
    Aborted,
}

impl CompletionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }

    pub const fn report_bucket(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "incomplete",
        }
    }

    pub const fn is_completed(self) -> bool {
        matches!(self, Self::Completed)
    }

    pub const fn is_incomplete(self) -> bool {
        matches!(self, Self::Aborted)
    }

    pub fn parse(input: &str) -> Result<Self, CompletionStatusParseError> {
        parse_completion_status(input)
    }
}

impl fmt::Display for CompletionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRecord {
    pub run_id: String,
    pub task_id: String,
    pub done_condition: String,
    pub status: CompletionStatus,
    pub local_note: Option<String>,
    pub timestamp: u64,
}

impl CompletionRecord {
    pub fn new(
        run_id: impl Into<String>,
        task_id: impl Into<String>,
        done_condition: impl Into<String>,
        status: CompletionStatus,
        local_note: Option<impl Into<String>>,
        timestamp: u64,
    ) -> Result<Self, CompletionValidationError> {
        let record = Self {
            run_id: run_id.into(),
            task_id: task_id.into(),
            done_condition: done_condition.into(),
            status,
            local_note: normalize_local_note(local_note.map(Into::into)),
            timestamp,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn now(
        run_id: impl Into<String>,
        task_id: impl Into<String>,
        done_condition: impl Into<String>,
        status: CompletionStatus,
        local_note: Option<impl Into<String>>,
    ) -> Result<Self, CompletionValidationError> {
        Self::new(
            run_id,
            task_id,
            done_condition,
            status,
            local_note,
            unix_timestamp_now(),
        )
    }

    pub fn validate(&self) -> Result<(), CompletionValidationError> {
        validate_required_field("run_id", &self.run_id)?;
        validate_required_field("task_id", &self.task_id)?;
        validate_required_field("done_condition", &self.done_condition)?;

        if self.timestamp == 0 {
            return Err(CompletionValidationError::MissingTimestamp);
        }

        if matches!(self.local_note.as_deref(), Some(note) if note.trim().is_empty()) {
            return Err(CompletionValidationError::EmptyLocalNote);
        }

        Ok(())
    }

    pub const fn is_completed_run(&self) -> bool {
        self.status.is_completed()
    }

    pub fn to_share_record(&self) -> CompletionShareRecord<'_> {
        CompletionShareRecord {
            run_id: &self.run_id,
            task_id: &self.task_id,
            done_condition: &self.done_condition,
            status: self.status,
            timestamp: self.timestamp,
        }
    }

    pub fn without_local_note(&self) -> CompletionShareRecord<'_> {
        self.to_share_record()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionShareRecord<'a> {
    pub run_id: &'a str,
    pub task_id: &'a str,
    pub done_condition: &'a str,
    pub status: CompletionStatus,
    pub timestamp: u64,
}

impl<'a> From<&'a CompletionRecord> for CompletionShareRecord<'a> {
    fn from(record: &'a CompletionRecord) -> Self {
        record.to_share_record()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompletionValidationError {
    EmptyField { field: &'static str },
    MissingTimestamp,
    EmptyLocalNote,
}

impl fmt::Display for CompletionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field } => write!(formatter, "`{field}` must not be empty"),
            Self::MissingTimestamp => write!(formatter, "`timestamp` must be a non-zero Unix time"),
            Self::EmptyLocalNote => {
                write!(formatter, "`local_note` must be omitted instead of empty")
            }
        }
    }
}

impl Error for CompletionValidationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionStatusParseError {
    input: String,
}

impl CompletionStatusParseError {
    pub fn input(&self) -> &str {
        &self.input
    }
}

impl fmt::Display for CompletionStatusParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.input.trim().is_empty() {
            formatter.write_str("completion status response is empty")
        } else {
            write!(
                formatter,
                "unsupported completion status `{}`; expected yes/no/pass/fail/abort",
                self.input
            )
        }
    }
}

impl Error for CompletionStatusParseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionPromptFallback {
    NotInteractive,
    EmptyInput,
    Interrupted,
    InvalidInput,
}

impl CompletionPromptFallback {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotInteractive => "not_interactive",
            Self::EmptyInput => "empty_input",
            Self::Interrupted => "interrupted",
            Self::InvalidInput => "invalid_input",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionPromptDecision {
    pub status: CompletionStatus,
    pub used_fallback: Option<CompletionPromptFallback>,
    pub raw_input: Option<String>,
}

impl CompletionPromptDecision {
    pub const fn from_status(status: CompletionStatus) -> Self {
        Self {
            status,
            used_fallback: None,
            raw_input: None,
        }
    }

    pub fn fallback(
        status: CompletionStatus,
        reason: CompletionPromptFallback,
        raw_input: Option<String>,
    ) -> Self {
        Self {
            status,
            used_fallback: Some(reason),
            raw_input,
        }
    }

    pub const fn used_fallback(&self) -> bool {
        self.used_fallback.is_some()
    }
}

pub fn parse_completion_status(
    input: &str,
) -> Result<CompletionStatus, CompletionStatusParseError> {
    let normalized = normalize_status_input(input);
    match normalized.as_str() {
        "y" | "yes" | "pass" | "passed" | "p" | "done" | "complete" | "completed" | "success"
        | "succeeded" | "ok" => Ok(CompletionStatus::Completed),
        "n" | "no" | "fail" | "failed" | "f" | "failure" | "unsuccessful" => {
            Ok(CompletionStatus::Failed)
        }
        "abort" | "aborted" | "incomplete" | "interrupted" | "interrupt" | "cancel"
        | "canceled" | "cancelled" | "skip" | "skipped" => Ok(CompletionStatus::Aborted),
        _ => Err(CompletionStatusParseError {
            input: input.to_string(),
        }),
    }
}

pub fn completion_status_or_fallback(
    input: Option<&str>,
    fallback: CompletionStatus,
) -> CompletionPromptDecision {
    let Some(input) = input else {
        return CompletionPromptDecision::fallback(
            fallback,
            CompletionPromptFallback::NotInteractive,
            None,
        );
    };

    if input.trim().is_empty() {
        return CompletionPromptDecision::fallback(
            fallback,
            CompletionPromptFallback::EmptyInput,
            Some(input.to_string()),
        );
    }

    match parse_completion_status(input) {
        Ok(status) => CompletionPromptDecision {
            status,
            used_fallback: None,
            raw_input: Some(input.to_string()),
        },
        Err(_) => CompletionPromptDecision::fallback(
            fallback,
            CompletionPromptFallback::InvalidInput,
            Some(input.to_string()),
        ),
    }
}

pub fn prompt_completion_status<R, W>(
    reader: &mut R,
    writer: &mut W,
    done_condition: &str,
    fallback: CompletionStatus,
) -> io::Result<CompletionPromptDecision>
where
    R: BufRead,
    W: Write,
{
    write_prompt(writer, done_condition, fallback)?;

    let mut line = String::new();
    match read_prompt_line(reader, &mut line) {
        ReadPromptLine::Eof => Ok(CompletionPromptDecision::fallback(
            fallback,
            CompletionPromptFallback::NotInteractive,
            None,
        )),
        ReadPromptLine::Line => Ok(completion_status_or_fallback(Some(&line), fallback)),
        ReadPromptLine::Interrupted => Ok(CompletionPromptDecision::fallback(
            fallback,
            CompletionPromptFallback::Interrupted,
            None,
        )),
        ReadPromptLine::Error(error) => Err(error),
    }
}

pub fn prompt_completion_status_from_stdin(
    done_condition: &str,
    fallback: CompletionStatus,
) -> io::Result<CompletionPromptDecision> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        return Ok(CompletionPromptDecision::fallback(
            fallback,
            CompletionPromptFallback::NotInteractive,
            None,
        ));
    }

    let mut reader = stdin.lock();
    let mut stderr = io::stderr();
    prompt_completion_status(&mut reader, &mut stderr, done_condition, fallback)
}

pub fn write_prompt<W>(
    writer: &mut W,
    done_condition: &str,
    fallback: CompletionStatus,
) -> io::Result<()>
where
    W: Write,
{
    writeln!(writer, "Done condition: {}", done_condition.trim())?;
    write!(
        writer,
        "Completion status [yes/pass, no/fail, abort/incomplete] (default: {}): ",
        fallback
    )?;
    writer.flush()
}

pub fn completed_run_records(records: &[CompletionRecord]) -> Vec<&CompletionRecord> {
    records
        .iter()
        .filter(|record| record.is_completed_run())
        .collect()
}

pub fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn validate_required_field(
    field: &'static str,
    value: &str,
) -> Result<(), CompletionValidationError> {
    if value.trim().is_empty() {
        return Err(CompletionValidationError::EmptyField { field });
    }

    Ok(())
}

fn normalize_local_note(local_note: Option<String>) -> Option<String> {
    local_note
        .map(|note| note.trim().to_string())
        .filter(|note| !note.is_empty())
}

fn normalize_status_input(input: &str) -> String {
    input
        .trim()
        .trim_matches(|ch: char| ch == '.' || ch == '!' || ch == '?')
        .to_ascii_lowercase()
}

enum ReadPromptLine {
    Line,
    Eof,
    Interrupted,
    Error(io::Error),
}

fn read_prompt_line<R>(reader: &mut R, line: &mut String) -> ReadPromptLine
where
    R: BufRead,
{
    loop {
        let (amount, found_newline) = match reader.fill_buf() {
            Ok(buffer) if buffer.is_empty() => {
                return if line.is_empty() {
                    ReadPromptLine::Eof
                } else {
                    ReadPromptLine::Line
                };
            }
            Ok(buffer) => {
                let amount = buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|index| index + 1)
                    .unwrap_or(buffer.len());
                line.push_str(&String::from_utf8_lossy(&buffer[..amount]));
                (amount, buffer[..amount].contains(&b'\n'))
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                return ReadPromptLine::Interrupted;
            }
            Err(error) => return ReadPromptLine::Error(error),
        };

        reader.consume(amount);

        if found_newline {
            return ReadPromptLine::Line;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn parses_completed_status_words() {
        for input in [
            "yes",
            "Y",
            "pass",
            "PASSED",
            "p",
            "done",
            "complete",
            "completed",
            "success",
            "ok",
            "yes!",
        ] {
            assert_eq!(
                parse_completion_status(input).unwrap(),
                CompletionStatus::Completed,
                "{input}"
            );
        }
    }

    #[test]
    fn parses_failed_status_words() {
        for input in ["no", "N", "fail", "FAILED", "f", "failure", "unsuccessful"] {
            assert_eq!(
                parse_completion_status(input).unwrap(),
                CompletionStatus::Failed,
                "{input}"
            );
        }
    }

    #[test]
    fn parses_aborted_and_incomplete_status_words() {
        for input in [
            "abort",
            "ABORTED",
            "incomplete",
            "interrupted",
            "interrupt",
            "cancel",
            "canceled",
            "cancelled",
            "skip",
            "skipped",
        ] {
            assert_eq!(
                parse_completion_status(input).unwrap(),
                CompletionStatus::Aborted,
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_unknown_status_words() {
        let error = parse_completion_status("maybe").unwrap_err();

        assert_eq!(error.input(), "maybe");
        assert_eq!(
            error.to_string(),
            "unsupported completion status `maybe`; expected yes/no/pass/fail/abort"
        );
    }

    #[test]
    fn distinguishes_status_report_buckets() {
        assert_eq!(CompletionStatus::Completed.as_str(), "completed");
        assert_eq!(CompletionStatus::Failed.as_str(), "failed");
        assert_eq!(CompletionStatus::Aborted.as_str(), "aborted");
        assert_eq!(CompletionStatus::Aborted.report_bucket(), "incomplete");
        assert!(CompletionStatus::Completed.is_completed());
        assert!(CompletionStatus::Aborted.is_incomplete());
    }

    #[test]
    fn validates_completion_record() {
        let record = CompletionRecord::new(
            "run-1",
            "task-1",
            "tests pass",
            CompletionStatus::Completed,
            Some("manual note"),
            42,
        )
        .unwrap();

        assert_eq!(record.run_id, "run-1");
        assert_eq!(record.task_id, "task-1");
        assert_eq!(record.done_condition, "tests pass");
        assert_eq!(record.status, CompletionStatus::Completed);
        assert_eq!(record.local_note.as_deref(), Some("manual note"));
        assert_eq!(record.timestamp, 42);
    }

    #[test]
    fn rejects_empty_required_fields() {
        let error = CompletionRecord::new(
            " ",
            "task-1",
            "tests pass",
            CompletionStatus::Completed,
            None::<String>,
            42,
        )
        .unwrap_err();

        assert_eq!(
            error,
            CompletionValidationError::EmptyField { field: "run_id" }
        );
    }

    #[test]
    fn rejects_missing_timestamp() {
        let error = CompletionRecord::new(
            "run-1",
            "task-1",
            "tests pass",
            CompletionStatus::Completed,
            None::<String>,
            0,
        )
        .unwrap_err();

        assert_eq!(error, CompletionValidationError::MissingTimestamp);
    }

    #[test]
    fn normalizes_blank_local_note_to_none() {
        let record = CompletionRecord::new(
            "run-1",
            "task-1",
            "tests pass",
            CompletionStatus::Completed,
            Some("   "),
            42,
        )
        .unwrap();

        assert_eq!(record.local_note, None);
    }

    #[test]
    fn share_record_omits_local_note() {
        let record = CompletionRecord::new(
            "run-1",
            "task-1",
            "tests pass",
            CompletionStatus::Failed,
            Some("sensitive local debugging note"),
            42,
        )
        .unwrap();

        let share = record.to_share_record();

        assert_eq!(
            share,
            CompletionShareRecord {
                run_id: "run-1",
                task_id: "task-1",
                done_condition: "tests pass",
                status: CompletionStatus::Failed,
                timestamp: 42,
            }
        );
    }

    #[test]
    fn prompt_accepts_reader_input_without_stdin() {
        let mut reader = io::Cursor::new(b"pass\n");
        let mut writer = Vec::new();

        let decision = prompt_completion_status(
            &mut reader,
            &mut writer,
            "all acceptance checks pass",
            CompletionStatus::Aborted,
        )
        .unwrap();

        assert_eq!(decision.status, CompletionStatus::Completed);
        assert_eq!(decision.used_fallback, None);
        assert_eq!(decision.raw_input.as_deref(), Some("pass\n"));

        let prompt = String::from_utf8(writer).unwrap();
        assert!(prompt.contains("Done condition: all acceptance checks pass"));
        assert!(prompt.contains("default: aborted"));
    }

    #[test]
    fn prompt_falls_back_on_eof() {
        let mut reader = io::Cursor::new(Vec::<u8>::new());
        let mut writer = Vec::new();

        let decision = prompt_completion_status(
            &mut reader,
            &mut writer,
            "all acceptance checks pass",
            CompletionStatus::Aborted,
        )
        .unwrap();

        assert_eq!(
            decision,
            CompletionPromptDecision::fallback(
                CompletionStatus::Aborted,
                CompletionPromptFallback::NotInteractive,
                None,
            )
        );
    }

    #[test]
    fn prompt_falls_back_on_invalid_or_empty_response() {
        let invalid = completion_status_or_fallback(Some("maybe"), CompletionStatus::Aborted);
        assert_eq!(invalid.status, CompletionStatus::Aborted);
        assert_eq!(
            invalid.used_fallback,
            Some(CompletionPromptFallback::InvalidInput)
        );
        assert_eq!(invalid.raw_input.as_deref(), Some("maybe"));

        let empty = completion_status_or_fallback(Some(" \n"), CompletionStatus::Aborted);
        assert_eq!(empty.status, CompletionStatus::Aborted);
        assert_eq!(
            empty.used_fallback,
            Some(CompletionPromptFallback::EmptyInput)
        );
    }

    #[test]
    fn prompt_falls_back_on_interrupted_read() {
        struct InterruptedReader;

        impl BufRead for InterruptedReader {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            }

            fn consume(&mut self, _amount: usize) {}
        }

        impl io::Read for InterruptedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            }
        }

        let mut reader = InterruptedReader;
        let mut writer = Vec::new();

        let decision = prompt_completion_status(
            &mut reader,
            &mut writer,
            "all acceptance checks pass",
            CompletionStatus::Aborted,
        )
        .unwrap();

        assert_eq!(
            decision,
            CompletionPromptDecision::fallback(
                CompletionStatus::Aborted,
                CompletionPromptFallback::Interrupted,
                None,
            )
        );
    }

    #[test]
    fn filters_completed_run_records() {
        let completed = CompletionRecord::new(
            "run-1",
            "task-1",
            "done",
            CompletionStatus::Completed,
            None::<String>,
            1,
        )
        .unwrap();
        let failed = CompletionRecord::new(
            "run-2",
            "task-2",
            "done",
            CompletionStatus::Failed,
            None::<String>,
            2,
        )
        .unwrap();
        let aborted = CompletionRecord::new(
            "run-3",
            "task-3",
            "done",
            CompletionStatus::Aborted,
            None::<String>,
            3,
        )
        .unwrap();
        let records = vec![completed, failed, aborted];

        let completed_records = completed_run_records(&records);

        assert_eq!(completed_records.len(), 1);
        assert_eq!(completed_records[0].run_id, "run-1");
    }
}
