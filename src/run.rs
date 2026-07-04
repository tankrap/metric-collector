use std::error::Error;
use std::fmt;
use std::str::FromStr;

pub const ADHOC_TASK_ID: &str = "adhoc";

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Profile {
    Baseline,
    Treatment,
    Adhoc,
}

impl Profile {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Treatment => "treatment",
            Self::Adhoc => "adhoc",
        }
    }

    pub const fn is_adhoc(&self) -> bool {
        matches!(self, Self::Adhoc)
    }

    pub const fn is_comparison(&self) -> bool {
        matches!(self, Self::Baseline | Self::Treatment)
    }

    pub fn parse(profile_id: &str) -> Result<Self, RunMetadataError> {
        profile_id.parse()
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Profile {
    type Err = RunMetadataError;

    fn from_str(profile_id: &str) -> Result<Self, Self::Err> {
        validate_profile_id_shape(profile_id)?;

        match profile_id {
            "baseline" => Ok(Self::Baseline),
            "treatment" => Ok(Self::Treatment),
            "adhoc" => Ok(Self::Adhoc),
            _ => Err(RunMetadataError::new(
                "profile_id",
                "must be one of: baseline, treatment, adhoc",
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RunId(String);

impl RunId {
    pub fn from_parts(started_at_ms: u64, counter: u64) -> Result<Self, RunMetadataError> {
        require_non_zero("started_at_ms", started_at_ms)?;

        Ok(Self(format!("run-{started_at_ms}-{counter:06}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunMetadata {
    pub run_id: RunId,
    pub task_id: String,
    pub profile: Profile,
    pub adapter: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

impl RunMetadata {
    pub fn new(
        run_id: RunId,
        task_id: impl Into<String>,
        profile: Profile,
        adapter: impl Into<String>,
        started_at_ms: u64,
        ended_at_ms: Option<u64>,
    ) -> Result<Self, RunMetadataError> {
        let task_id = task_id.into();
        let adapter = adapter.into();

        validate_task_id(&task_id)?;
        validate_adapter(&adapter)?;
        validate_timestamps(started_at_ms, ended_at_ms)?;

        if profile.is_adhoc() && task_id != ADHOC_TASK_ID {
            return Err(RunMetadataError::new(
                "task_id",
                format!("adhoc runs must use task_id '{ADHOC_TASK_ID}'"),
            ));
        }

        Ok(Self {
            run_id,
            task_id,
            profile,
            adapter,
            started_at_ms,
            ended_at_ms,
        })
    }

    pub fn for_task(
        started_at_ms: u64,
        counter: u64,
        task_id: impl Into<String>,
        profile: Profile,
        adapter: impl Into<String>,
    ) -> Result<Self, RunMetadataError> {
        let run_id = RunId::from_parts(started_at_ms, counter)?;
        Self::new(run_id, task_id, profile, adapter, started_at_ms, None)
    }

    pub fn adhoc(
        started_at_ms: u64,
        counter: u64,
        adapter: impl Into<String>,
    ) -> Result<Self, RunMetadataError> {
        Self::for_task(
            started_at_ms,
            counter,
            ADHOC_TASK_ID,
            Profile::Adhoc,
            adapter,
        )
    }

    pub fn finish(&mut self, ended_at_ms: u64) -> Result<(), RunMetadataError> {
        validate_timestamps(self.started_at_ms, Some(ended_at_ms))?;
        self.ended_at_ms = Some(ended_at_ms);
        Ok(())
    }

    pub fn stamp_event(&self, builder: &mut EventBuilder) {
        builder.run_id = Some(self.run_id.to_string());
        builder.task_id = Some(self.task_id.clone());
        builder.profile_id = Some(self.profile.as_str().to_owned());
        builder.adapter = Some(self.adapter.clone());
        builder.run_started_at_ms = Some(self.started_at_ms);
        builder.run_ended_at_ms = self.ended_at_ms;
    }

    pub fn report_metadata(&self) -> RunReportMetadata<'_> {
        RunReportMetadata {
            run_id: self.run_id.as_str(),
            task_id: &self.task_id,
            profile_id: self.profile.as_str(),
            adapter: &self.adapter,
            started_at_ms: self.started_at_ms,
            ended_at_ms: self.ended_at_ms,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventBuilder {
    pub timestamp_ms: Option<u64>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub profile_id: Option<String>,
    pub adapter: Option<String>,
    pub run_started_at_ms: Option<u64>,
    pub run_ended_at_ms: Option<u64>,
}

impl EventBuilder {
    pub fn new(timestamp_ms: u64) -> Self {
        Self {
            timestamp_ms: Some(timestamp_ms),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunReportMetadata<'a> {
    pub run_id: &'a str,
    pub task_id: &'a str,
    pub profile_id: &'a str,
    pub adapter: &'a str,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunMetadataError {
    pub field: &'static str,
    pub message: String,
}

impl RunMetadataError {
    pub fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

impl fmt::Display for RunMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

impl Error for RunMetadataError {}

pub fn validate_task_id(task_id: &str) -> Result<(), RunMetadataError> {
    validate_identifier("task_id", task_id)
}

fn validate_profile_id_shape(profile_id: &str) -> Result<(), RunMetadataError> {
    validate_identifier("profile_id", profile_id)
}

fn validate_adapter(adapter: &str) -> Result<(), RunMetadataError> {
    if adapter.trim().is_empty() {
        return Err(RunMetadataError::new("adapter", "is required"));
    }

    if adapter.trim() != adapter {
        return Err(RunMetadataError::new(
            "adapter",
            "must not contain leading or trailing whitespace",
        ));
    }

    Ok(())
}

fn validate_timestamps(
    started_at_ms: u64,
    ended_at_ms: Option<u64>,
) -> Result<(), RunMetadataError> {
    require_non_zero("started_at_ms", started_at_ms)?;

    if let Some(ended_at_ms) = ended_at_ms {
        require_non_zero("ended_at_ms", ended_at_ms)?;
        if ended_at_ms < started_at_ms {
            return Err(RunMetadataError::new(
                "ended_at_ms",
                "must be greater than or equal to started_at_ms",
            ));
        }
    }

    Ok(())
}

fn require_non_zero(field: &'static str, value: u64) -> Result<(), RunMetadataError> {
    if value == 0 {
        Err(RunMetadataError::new(field, "must be greater than zero"))
    } else {
        Ok(())
    }
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), RunMetadataError> {
    if value.trim().is_empty() {
        return Err(RunMetadataError::new(field, "is required"));
    }

    if value.trim() != value {
        return Err(RunMetadataError::new(
            field,
            "must not contain leading or trailing whitespace",
        ));
    }

    if value.len() > 128 {
        return Err(RunMetadataError::new(field, "must be 128 bytes or fewer"));
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(RunMetadataError::new(field, "is required"));
    };

    if !first.is_ascii_alphanumeric() {
        return Err(RunMetadataError::new(
            field,
            "must start with an ASCII letter or digit",
        ));
    }

    if !chars.all(is_identifier_continuation) {
        return Err(RunMetadataError::new(
            field,
            "may contain only ASCII letters, digits, '.', '_' or '-'",
        ));
    }

    Ok(())
}

fn is_identifier_continuation(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(profile: Profile) -> RunMetadata {
        RunMetadata::for_task(1_725_000_000_123, 7, "task-1", profile, "codex-cli").unwrap()
    }

    #[test]
    fn creates_valid_baseline_run_and_stamps_events() {
        let run = metadata(Profile::Baseline);
        let mut event = EventBuilder::new(1_725_000_000_456);

        run.stamp_event(&mut event);

        assert_eq!(run.run_id.as_str(), "run-1725000000123-000007");
        assert_eq!(event.timestamp_ms, Some(1_725_000_000_456));
        assert_eq!(event.run_id.as_deref(), Some("run-1725000000123-000007"));
        assert_eq!(event.task_id.as_deref(), Some("task-1"));
        assert_eq!(event.profile_id.as_deref(), Some("baseline"));
        assert_eq!(event.adapter.as_deref(), Some("codex-cli"));
        assert_eq!(event.run_started_at_ms, Some(1_725_000_000_123));
        assert_eq!(event.run_ended_at_ms, None);
    }

    #[test]
    fn creates_valid_treatment_run_and_records_end_timestamp() {
        let mut run = metadata(Profile::Treatment);

        run.finish(1_725_000_000_999).unwrap();

        let mut event = EventBuilder::default();
        run.stamp_event(&mut event);

        assert_eq!(run.profile.as_str(), "treatment");
        assert!(run.profile.is_comparison());
        assert_eq!(event.profile_id.as_deref(), Some("treatment"));
        assert_eq!(event.run_ended_at_ms, Some(1_725_000_000_999));
    }

    #[test]
    fn rejects_invalid_profiles_with_clear_errors() {
        let error = Profile::parse("control").unwrap_err();

        assert_eq!(error.field, "profile_id");
        assert_eq!(error.message, "must be one of: baseline, treatment, adhoc");

        let error = Profile::parse("baseline run").unwrap_err();
        assert_eq!(error.field, "profile_id");
        assert_eq!(
            error.message,
            "may contain only ASCII letters, digits, '.', '_' or '-'"
        );
    }

    #[test]
    fn rejects_invalid_task_ids_with_clear_errors() {
        let error = RunMetadata::for_task(
            1_725_000_000_123,
            0,
            "task/1",
            Profile::Baseline,
            "codex-cli",
        )
        .unwrap_err();

        assert_eq!(error.field, "task_id");
        assert_eq!(
            error.message,
            "may contain only ASCII letters, digits, '.', '_' or '-'"
        );

        let error = RunMetadata::for_task(
            1_725_000_000_123,
            0,
            " task-1",
            Profile::Baseline,
            "codex-cli",
        )
        .unwrap_err();
        assert_eq!(error.field, "task_id");
        assert_eq!(
            error.message,
            "must not contain leading or trailing whitespace"
        );
    }

    #[test]
    fn creates_adhoc_run_without_manifest_task() {
        let run = RunMetadata::adhoc(1_725_000_001_000, 12, "manual").unwrap();

        assert_eq!(run.run_id.as_str(), "run-1725000001000-000012");
        assert_eq!(run.task_id, ADHOC_TASK_ID);
        assert_eq!(run.profile, Profile::Adhoc);
        assert!(run.profile.is_adhoc());

        let error =
            RunMetadata::for_task(1_725_000_001_000, 12, "task-1", Profile::Adhoc, "manual")
                .unwrap_err();
        assert_eq!(error.field, "task_id");
        assert_eq!(error.message, "adhoc runs must use task_id 'adhoc'");
    }

    #[test]
    fn exposes_metadata_for_reports() {
        let mut run = metadata(Profile::Baseline);
        run.finish(1_725_000_000_321).unwrap();

        let report = run.report_metadata();

        assert_eq!(report.run_id, "run-1725000000123-000007");
        assert_eq!(report.task_id, "task-1");
        assert_eq!(report.profile_id, "baseline");
        assert_eq!(report.adapter, "codex-cli");
        assert_eq!(report.started_at_ms, 1_725_000_000_123);
        assert_eq!(report.ended_at_ms, Some(1_725_000_000_321));
    }
}
