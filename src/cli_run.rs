use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::completion::{CompletionRecord, CompletionStatus};
use crate::mode::ModeState;
use crate::run::{Profile as RunProfile, RunMetadata};
use crate::scheduler::{
    Profile as SchedulerProfile, RunKey, RunMatrixEntry, RunMatrixScheduler, RunState,
};
use crate::tasks::{Task, TaskManifest};

const DEFAULT_RUN_ADAPTER: &str = "tokmeter";
pub const COMPLETED_RUNS_FILE_NAME: &str = "completed-runs.tsv";
const COMPLETED_RUNS_HEADER: &str = "vc-tokmeter completed-runs v1";
const COMPLETED_RUNS_FIELD_COUNT: usize = 9;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunCliArgs {
    pub task_id: Option<String>,
    pub profile_id: Option<String>,
    pub next: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunSelection {
    Explicit,
    Next {
        repetition: usize,
        pending_before_selection: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedRun {
    pub task_id: String,
    pub profile_id: String,
    pub repetition: usize,
}

impl CompletedRun {
    pub fn new(
        task_id: impl Into<String>,
        profile_id: impl Into<String>,
        repetition: usize,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            profile_id: profile_id.into(),
            repetition,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedRunRecord {
    pub run_id: String,
    pub task_id: String,
    pub profile_id: String,
    pub repetition: usize,
    pub status: CompletionStatus,
    pub adapter: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub completed_at_unix: u64,
}

impl CompletedRunRecord {
    pub fn new(
        run_id: impl Into<String>,
        task_id: impl Into<String>,
        profile_id: impl Into<String>,
        repetition: usize,
        status: CompletionStatus,
        adapter: impl Into<String>,
        started_at_ms: u64,
        ended_at_ms: Option<u64>,
        completed_at_unix: u64,
    ) -> Result<Self, CompletedRunStoreError> {
        let record = Self {
            run_id: run_id.into(),
            task_id: task_id.into(),
            profile_id: profile_id.into(),
            repetition,
            status,
            adapter: adapter.into(),
            started_at_ms,
            ended_at_ms,
            completed_at_unix,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), CompletedRunStoreError> {
        validate_completed_run_field("run_id", &self.run_id)?;
        crate::run::validate_task_id(&self.task_id).map_err(|error| {
            CompletedRunStoreError::Validation {
                field: error.field,
                message: error.message,
            }
        })?;
        comparison_profile(&self.profile_id).map_err(|error| match error {
            RunPlanError::InvalidProfile { profile_id } => CompletedRunStoreError::Validation {
                field: "profile_id",
                message: format!("invalid comparison profile `{profile_id}`"),
            },
            _ => CompletedRunStoreError::Validation {
                field: "profile_id",
                message: error.to_string(),
            },
        })?;
        if self.repetition == 0 {
            return Err(CompletedRunStoreError::Validation {
                field: "repetition",
                message: "must be greater than zero".to_owned(),
            });
        }
        validate_completed_run_field("adapter", &self.adapter)?;
        if self.started_at_ms == 0 {
            return Err(CompletedRunStoreError::Validation {
                field: "started_at_ms",
                message: "must be greater than zero".to_owned(),
            });
        }
        if let Some(ended_at_ms) = self.ended_at_ms {
            if ended_at_ms < self.started_at_ms {
                return Err(CompletedRunStoreError::Validation {
                    field: "ended_at_ms",
                    message: "must be greater than or equal to started_at_ms".to_owned(),
                });
            }
        }
        if self.completed_at_unix == 0 {
            return Err(CompletedRunStoreError::Validation {
                field: "completed_at_unix",
                message: "must be greater than zero".to_owned(),
            });
        }

        Ok(())
    }

    pub fn is_completed(&self) -> bool {
        self.status.is_completed()
    }

    pub fn to_completed_run(&self) -> Option<CompletedRun> {
        self.is_completed()
            .then(|| CompletedRun::new(&self.task_id, &self.profile_id, self.repetition))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunPlanContext<'a> {
    pub manifest: &'a TaskManifest,
    pub completed_runs: &'a [CompletedRun],
    pub repetitions: usize,
    pub started_at_ms: u64,
    pub run_counter: u64,
    pub adapter: &'a str,
}

impl<'a> RunPlanContext<'a> {
    pub fn new(manifest: &'a TaskManifest) -> Self {
        Self {
            manifest,
            completed_runs: &[],
            repetitions: 1,
            started_at_ms: 1,
            run_counter: 1,
            adapter: DEFAULT_RUN_ADAPTER,
        }
    }

    pub fn with_completed_runs(mut self, completed_runs: &'a [CompletedRun]) -> Self {
        self.completed_runs = completed_runs;
        self
    }

    pub const fn with_repetitions(mut self, repetitions: usize) -> Self {
        self.repetitions = repetitions;
        self
    }

    pub const fn with_run_identity(mut self, started_at_ms: u64, run_counter: u64) -> Self {
        self.started_at_ms = started_at_ms;
        self.run_counter = run_counter;
        self
    }

    pub const fn with_adapter(mut self, adapter: &'a str) -> Self {
        self.adapter = adapter;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunPlan {
    pub selection: RunSelection,
    pub mode_state: ModeState,
    pub run_metadata: RunMetadata,
    pub manifest_task_id: String,
    pub profile_id: String,
    pub output: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunPlanError {
    MissingValue {
        flag: &'static str,
    },
    DuplicateFlag {
        flag: &'static str,
    },
    UnknownArg {
        arg: String,
    },
    MissingArg {
        name: &'static str,
    },
    ConflictingArgs {
        message: String,
    },
    InvalidProfile {
        profile_id: String,
    },
    UnknownTask {
        task_id: String,
    },
    InvalidRunMetadata {
        field: &'static str,
        message: String,
    },
    NoPendingRuns,
}

impl fmt::Display for RunPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue { flag } => write!(formatter, "{flag} requires a value"),
            Self::DuplicateFlag { flag } => write!(formatter, "{flag} was provided more than once"),
            Self::UnknownArg { arg } => write!(formatter, "unsupported run argument `{arg}`"),
            Self::MissingArg { name } => write!(formatter, "missing required {name}"),
            Self::ConflictingArgs { message } => formatter.write_str(message),
            Self::InvalidProfile { profile_id } => write!(
                formatter,
                "invalid profile `{profile_id}`; expected `baseline` or `treatment`"
            ),
            Self::UnknownTask { task_id } => {
                write!(formatter, "task `{task_id}` is not present in the manifest")
            }
            Self::InvalidRunMetadata { field, message } => {
                write!(formatter, "{field}: {message}")
            }
            Self::NoPendingRuns => formatter.write_str("no pending task/profile runs remain"),
        }
    }
}

impl Error for RunPlanError {}

#[derive(Debug)]
pub enum CompletedRunStoreError {
    Io(io::Error),
    InvalidHeader {
        found: String,
    },
    MalformedLine {
        line: usize,
        message: String,
    },
    Validation {
        field: &'static str,
        message: String,
    },
}

impl fmt::Display for CompletedRunStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "completed run store I/O error: {error}"),
            Self::InvalidHeader { found } => {
                write!(formatter, "invalid completed run store header `{found}`")
            }
            Self::MalformedLine { line, message } => {
                write!(
                    formatter,
                    "line {line}: malformed completed run record: {message}"
                )
            }
            Self::Validation { field, message } => write!(formatter, "{field}: {message}"),
        }
    }
}

impl Error for CompletedRunStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for CompletedRunStoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn completed_runs_path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(COMPLETED_RUNS_FILE_NAME)
}

pub fn completed_run_record_from_completion(
    plan: &RunPlan,
    completion: &CompletionRecord,
    ended_at_ms: u64,
) -> Result<CompletedRunRecord, CompletedRunStoreError> {
    let run_id = plan.run_metadata.run_id.to_string();
    if completion.run_id != run_id {
        return Err(CompletedRunStoreError::Validation {
            field: "run_id",
            message: "completion record run_id must match the run plan".to_owned(),
        });
    }
    if completion.task_id != plan.manifest_task_id {
        return Err(CompletedRunStoreError::Validation {
            field: "task_id",
            message: "completion record task_id must match the run plan".to_owned(),
        });
    }

    let repetition = match plan.selection {
        RunSelection::Explicit => 1,
        RunSelection::Next { repetition, .. } => repetition,
    };

    CompletedRunRecord::new(
        run_id,
        plan.manifest_task_id.clone(),
        plan.profile_id.clone(),
        repetition,
        completion.status,
        plan.run_metadata.adapter.clone(),
        plan.run_metadata.started_at_ms,
        Some(ended_at_ms),
        completion.timestamp,
    )
}

pub fn completed_runs_for_scheduler(records: &[CompletedRunRecord]) -> Vec<CompletedRun> {
    records
        .iter()
        .filter_map(CompletedRunRecord::to_completed_run)
        .collect()
}

pub fn read_completed_run_records(
    path: impl AsRef<Path>,
) -> Result<Vec<CompletedRunRecord>, CompletedRunStoreError> {
    let path = path.as_ref();
    match fs::read_to_string(path) {
        Ok(contents) => parse_completed_run_records(&contents),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn write_completed_run_records(
    path: impl AsRef<Path>,
    records: &[CompletedRunRecord],
) -> Result<(), CompletedRunStoreError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serialize_completed_run_records(records))?;
    Ok(())
}

pub fn append_completed_run_record(
    path: impl AsRef<Path>,
    record: &CompletedRunRecord,
) -> Result<(), CompletedRunStoreError> {
    record.validate()?;
    let path = path.as_ref();
    if !path.exists() {
        return write_completed_run_records(path, std::slice::from_ref(record));
    }

    let mut file = OpenOptions::new().append(true).open(path)?;
    writeln!(file, "{}", serialize_completed_run_record(record))?;
    Ok(())
}

pub fn serialize_completed_run_records(records: &[CompletedRunRecord]) -> String {
    let mut out = String::new();
    out.push_str(COMPLETED_RUNS_HEADER);
    out.push('\n');
    for record in records {
        out.push_str(&serialize_completed_run_record(record));
        out.push('\n');
    }
    out
}

pub fn parse_run_args<I, S>(args: I) -> Result<RunCliArgs, RunPlanError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut parsed = RunCliArgs::default();
    let mut args = args.into_iter().map(Into::into);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => {
                if parsed.profile_id.is_some() {
                    return Err(RunPlanError::DuplicateFlag { flag: "--profile" });
                }
                parsed.profile_id = Some(next_value(&mut args, "--profile")?);
            }
            "--task" => {
                if parsed.task_id.is_some() {
                    return Err(RunPlanError::DuplicateFlag { flag: "--task" });
                }
                parsed.task_id = Some(next_value(&mut args, "--task")?);
            }
            "--next" => {
                if parsed.next {
                    return Err(RunPlanError::DuplicateFlag { flag: "--next" });
                }
                parsed.next = true;
            }
            _ => return Err(RunPlanError::UnknownArg { arg }),
        }
    }

    Ok(parsed)
}

pub fn plan_run<I, S>(args: I, context: &RunPlanContext<'_>) -> Result<RunPlan, RunPlanError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = parse_run_args(args)?;
    plan_from_parsed_args(&args, context)
}

pub fn plan_from_parsed_args(
    args: &RunCliArgs,
    context: &RunPlanContext<'_>,
) -> Result<RunPlan, RunPlanError> {
    if args.next {
        if args.task_id.is_some() || args.profile_id.is_some() {
            return Err(RunPlanError::ConflictingArgs {
                message: "`--next` cannot be combined with `--task` or `--profile`".to_owned(),
            });
        }
        return plan_next_run(context);
    }

    let task_id = args
        .task_id
        .as_deref()
        .ok_or(RunPlanError::MissingArg { name: "--task" })?;
    let profile_id = args
        .profile_id
        .as_deref()
        .ok_or(RunPlanError::MissingArg { name: "--profile" })?;

    let task = manifest_task(context.manifest, task_id)?;
    let profile = comparison_profile(profile_id)?;

    build_plan(
        RunSelection::Explicit,
        task,
        profile,
        None,
        context.started_at_ms,
        context.run_counter,
        context.adapter,
    )
}

pub fn plan_next_run(context: &RunPlanContext<'_>) -> Result<RunPlan, RunPlanError> {
    let task_ids = context
        .manifest
        .tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    let mut scheduler = RunMatrixScheduler::with_repetitions(task_ids, context.repetitions);

    for completed in context.completed_runs {
        let profile = scheduler_profile(&completed.profile_id)?;
        scheduler
            .transition(
                &RunKey::new(completed.task_id.clone(), profile, completed.repetition),
                RunState::Completed,
            )
            .map_err(|_| RunPlanError::UnknownTask {
                task_id: completed.task_id.clone(),
            })?;
    }

    let pending_before_selection = scheduler.pending_count();
    let entry = scheduler
        .next_pending()
        .ok_or(RunPlanError::NoPendingRuns)?;
    let task = manifest_task(context.manifest, &entry.task_id)?;
    let profile = comparison_profile(entry.profile.as_str())?;

    build_plan(
        RunSelection::Next {
            repetition: entry.repetition,
            pending_before_selection,
        },
        task,
        profile,
        Some(entry),
        context.started_at_ms,
        context.run_counter,
        context.adapter,
    )
}

fn build_plan(
    selection: RunSelection,
    task: &Task,
    profile: RunProfile,
    matrix_entry: Option<&RunMatrixEntry>,
    started_at_ms: u64,
    run_counter: u64,
    adapter: &str,
) -> Result<RunPlan, RunPlanError> {
    let profile_id = profile.as_str();
    let mode_state = ModeState::task(&task.id, profile_id).map_err(|error| {
        RunPlanError::InvalidRunMetadata {
            field: error.field,
            message: error.message,
        }
    })?;
    let run_metadata =
        RunMetadata::for_task(started_at_ms, run_counter, &task.id, profile, adapter).map_err(
            |error| RunPlanError::InvalidRunMetadata {
                field: error.field,
                message: error.message,
            },
        )?;
    let output = run_plan_output(&mode_state, &run_metadata, task, matrix_entry);

    Ok(RunPlan {
        selection,
        mode_state,
        run_metadata,
        manifest_task_id: task.id.clone(),
        profile_id: profile_id.to_owned(),
        output,
    })
}

fn run_plan_output(
    mode_state: &ModeState,
    run_metadata: &RunMetadata,
    task: &Task,
    matrix_entry: Option<&RunMatrixEntry>,
) -> String {
    let mut out = String::new();
    out.push_str("tokmeter run plan\n");
    out.push_str(&format!("mode: {}\n", mode_state.mode.as_str()));
    out.push_str(&format!("task: {} - {}\n", task.id, task.title));
    out.push_str(&format!("profile: {}\n", mode_state.profile_id));
    out.push_str(&format!("run_id: {}\n", run_metadata.run_id));
    if let Some(entry) = matrix_entry {
        out.push_str(&format!("repetition: {}\n", entry.repetition));
        out.push_str("selection: next pending\n");
    } else {
        out.push_str("selection: explicit\n");
    }
    out.push_str("capture: task mode\n");
    out
}

fn next_value<I>(args: &mut I, flag: &'static str) -> Result<String, RunPlanError>
where
    I: Iterator<Item = String>,
{
    match args.next() {
        Some(value) if !value.starts_with("--") => Ok(value),
        _ => Err(RunPlanError::MissingValue { flag }),
    }
}

fn manifest_task<'a>(manifest: &'a TaskManifest, task_id: &str) -> Result<&'a Task, RunPlanError> {
    crate::run::validate_task_id(task_id).map_err(|error| RunPlanError::InvalidRunMetadata {
        field: error.field,
        message: error.message,
    })?;

    manifest
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| RunPlanError::UnknownTask {
            task_id: task_id.to_owned(),
        })
}

fn comparison_profile(profile_id: &str) -> Result<RunProfile, RunPlanError> {
    let profile = RunProfile::parse(profile_id).map_err(|_| RunPlanError::InvalidProfile {
        profile_id: profile_id.to_owned(),
    })?;

    if profile.is_comparison() {
        Ok(profile)
    } else {
        Err(RunPlanError::InvalidProfile {
            profile_id: profile_id.to_owned(),
        })
    }
}

fn scheduler_profile(profile_id: &str) -> Result<SchedulerProfile, RunPlanError> {
    match profile_id {
        "baseline" => Ok(SchedulerProfile::Baseline),
        "treatment" => Ok(SchedulerProfile::Treatment),
        _ => Err(RunPlanError::InvalidProfile {
            profile_id: profile_id.to_owned(),
        }),
    }
}

fn parse_completed_run_records(
    contents: &str,
) -> Result<Vec<CompletedRunRecord>, CompletedRunStoreError> {
    let mut lines = contents.lines();
    let header = lines.next().unwrap_or_default();
    if header != COMPLETED_RUNS_HEADER {
        return Err(CompletedRunStoreError::InvalidHeader {
            found: header.to_owned(),
        });
    }

    lines
        .enumerate()
        .filter_map(|(offset, line)| {
            let line_number = offset + 2;
            (!line.trim().is_empty()).then(|| parse_completed_run_record_line(line_number, line))
        })
        .collect()
}

fn parse_completed_run_record_line(
    line_number: usize,
    line: &str,
) -> Result<CompletedRunRecord, CompletedRunStoreError> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != COMPLETED_RUNS_FIELD_COUNT {
        return Err(CompletedRunStoreError::MalformedLine {
            line: line_number,
            message: format!(
                "expected {COMPLETED_RUNS_FIELD_COUNT} tab-separated fields, got {}",
                fields.len()
            ),
        });
    }

    let run_id = unescape_store_field(fields[0], line_number)?;
    let task_id = unescape_store_field(fields[1], line_number)?;
    let profile_id = unescape_store_field(fields[2], line_number)?;
    let repetition = parse_usize_field("repetition", fields[3], line_number)?;
    let status = CompletionStatus::parse(fields[4]).map_err(|error| {
        CompletedRunStoreError::MalformedLine {
            line: line_number,
            message: error.to_string(),
        }
    })?;
    let adapter = unescape_store_field(fields[5], line_number)?;
    let started_at_ms = parse_u64_field("started_at_ms", fields[6], line_number)?;
    let ended_at_ms = match fields[7] {
        "-" => None,
        value => Some(parse_u64_field("ended_at_ms", value, line_number)?),
    };
    let completed_at_unix = parse_u64_field("completed_at_unix", fields[8], line_number)?;

    CompletedRunRecord::new(
        run_id,
        task_id,
        profile_id,
        repetition,
        status,
        adapter,
        started_at_ms,
        ended_at_ms,
        completed_at_unix,
    )
}

fn serialize_completed_run_record(record: &CompletedRunRecord) -> String {
    [
        escape_store_field(&record.run_id),
        escape_store_field(&record.task_id),
        escape_store_field(&record.profile_id),
        record.repetition.to_string(),
        record.status.as_str().to_owned(),
        escape_store_field(&record.adapter),
        record.started_at_ms.to_string(),
        record
            .ended_at_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        record.completed_at_unix.to_string(),
    ]
    .join("\t")
}

fn parse_usize_field(
    field: &'static str,
    value: &str,
    line: usize,
) -> Result<usize, CompletedRunStoreError> {
    value
        .parse()
        .map_err(|_| CompletedRunStoreError::MalformedLine {
            line,
            message: format!("{field} is not a positive integer"),
        })
}

fn parse_u64_field(
    field: &'static str,
    value: &str,
    line: usize,
) -> Result<u64, CompletedRunStoreError> {
    value
        .parse()
        .map_err(|_| CompletedRunStoreError::MalformedLine {
            line,
            message: format!("{field} is not an unsigned integer"),
        })
}

fn validate_completed_run_field(
    field: &'static str,
    value: &str,
) -> Result<(), CompletedRunStoreError> {
    if value.trim().is_empty() {
        return Err(CompletedRunStoreError::Validation {
            field,
            message: "must not be empty".to_owned(),
        });
    }

    if value.trim() != value {
        return Err(CompletedRunStoreError::Validation {
            field,
            message: "must not contain leading or trailing whitespace".to_owned(),
        });
    }

    Ok(())
}

fn escape_store_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_store_field(value: &str, line: usize) -> Result<String, CompletedRunStoreError> {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let Some(escaped) = chars.next() else {
            return Err(CompletedRunStoreError::MalformedLine {
                line,
                message: "field ends with an incomplete escape".to_owned(),
            });
        };

        match escaped {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            _ => {
                return Err(CompletedRunStoreError::MalformedLine {
                    line,
                    message: format!("unsupported escape sequence `\\{escaped}`"),
                });
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::CompletionStatus;
    use crate::core::CaptureMode;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn manifest() -> TaskManifest {
        TaskManifest {
            tasks: (1..=5)
                .map(|index| Task {
                    id: format!("task-{index}"),
                    title: format!("Task {index}"),
                    description: String::new(),
                    done: false,
                })
                .collect(),
        }
    }

    fn context<'a>(manifest: &'a TaskManifest) -> RunPlanContext<'a> {
        RunPlanContext::new(manifest)
            .with_repetitions(1)
            .with_run_identity(1_725_000_000_123, 7)
            .with_adapter("codex-cli")
    }

    fn temp_store_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vc-tokmeter-cli-run-{name}-{}-{nonce}.tsv",
            std::process::id()
        ))
    }

    fn completed_record(task_id: &str, profile_id: &str, repetition: usize) -> CompletedRunRecord {
        CompletedRunRecord::new(
            format!("run-{task_id}-{profile_id}-{repetition}"),
            task_id,
            profile_id,
            repetition,
            CompletionStatus::Completed,
            "codex-cli",
            1_725_000_000_123,
            Some(1_725_000_001_123),
            1_725_000_002,
        )
        .unwrap()
    }

    #[test]
    fn explicit_task_and_profile_create_task_mode_plan() {
        let manifest = manifest();
        let plan = plan_run(
            ["--task", "task-3", "--profile", "baseline"],
            &context(&manifest),
        )
        .unwrap();

        assert_eq!(plan.selection, RunSelection::Explicit);
        assert_eq!(plan.manifest_task_id, "task-3");
        assert_eq!(plan.profile_id, "baseline");
        assert_eq!(plan.mode_state.mode, CaptureMode::Task);
        assert_eq!(plan.mode_state.task_id, "task-3");
        assert_eq!(plan.mode_state.profile_id, "baseline");
        assert_eq!(plan.run_metadata.task_id, "task-3");
        assert_eq!(plan.run_metadata.profile, RunProfile::Baseline);
        assert!(plan.output.contains("mode: task"));
        assert!(plan.output.contains("selection: explicit"));
    }

    #[test]
    fn next_selects_interleaved_pending_fixture_run() {
        let manifest = manifest();
        let completed = [
            CompletedRun::new("task-1", "baseline", 1),
            CompletedRun::new("task-1", "treatment", 1),
            CompletedRun::new("task-2", "baseline", 1),
        ];
        let context = context(&manifest).with_completed_runs(&completed);
        let plan = plan_run(["--next"], &context).unwrap();

        assert_eq!(
            plan.selection,
            RunSelection::Next {
                repetition: 1,
                pending_before_selection: 17,
            }
        );
        assert_eq!(plan.mode_state.mode, CaptureMode::Task);
        assert_eq!(plan.manifest_task_id, "task-2");
        assert_eq!(plan.profile_id, "treatment");
        assert!(plan.output.contains("selection: next pending"));
        assert!(plan.output.contains("repetition: 1"));
    }

    #[test]
    fn missing_and_invalid_args_are_reported() {
        let manifest = manifest();
        let context = context(&manifest);

        assert_eq!(
            plan_run(["--task", "task-1"], &context).unwrap_err(),
            RunPlanError::MissingArg { name: "--profile" }
        );
        assert_eq!(
            plan_run(["--profile"], &context).unwrap_err(),
            RunPlanError::MissingValue { flag: "--profile" }
        );
        assert_eq!(
            plan_run(["--task", "task-1", "--profile", "adhoc"], &context).unwrap_err(),
            RunPlanError::InvalidProfile {
                profile_id: "adhoc".to_owned(),
            }
        );
        assert_eq!(
            plan_run(["--task", "missing", "--profile", "baseline"], &context).unwrap_err(),
            RunPlanError::UnknownTask {
                task_id: "missing".to_owned(),
            }
        );
        assert!(matches!(
            plan_run(["--next", "--task", "task-1"], &context).unwrap_err(),
            RunPlanError::ConflictingArgs { .. }
        ));
    }

    #[test]
    fn run_plans_never_use_passive_mode() {
        let manifest = manifest();
        let explicit = plan_run(
            ["--task", "task-1", "--profile", "treatment"],
            &context(&manifest),
        )
        .unwrap();
        let next = plan_run(["--next"], &context(&manifest)).unwrap();

        assert_ne!(explicit.mode_state.mode, CaptureMode::Passive);
        assert_ne!(next.mode_state.mode, CaptureMode::Passive);
        assert_eq!(explicit.mode_state.mode, CaptureMode::Task);
        assert_eq!(next.mode_state.mode, CaptureMode::Task);
    }

    #[test]
    fn completed_run_store_round_trips_public_records() {
        let path = temp_store_path("round-trip");
        let records = vec![
            completed_record("task-1", "baseline", 1),
            CompletedRunRecord::new(
                "run-task-1-treatment-1",
                "task-1",
                "treatment",
                1,
                CompletionStatus::Failed,
                "codex-cli",
                1_725_000_010_000,
                Some(1_725_000_011_000),
                1_725_000_012,
            )
            .unwrap(),
        ];

        write_completed_run_records(&path, &records).unwrap();
        let serialized = fs::read_to_string(&path).unwrap();
        let loaded = read_completed_run_records(&path).unwrap();

        assert!(serialized.starts_with(COMPLETED_RUNS_HEADER));
        assert_eq!(loaded, records);
        assert_eq!(
            completed_runs_for_scheduler(&loaded),
            vec![CompletedRun::new("task-1", "baseline", 1)]
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn append_completed_run_record_keeps_versioned_format() {
        let path = temp_store_path("append");
        let first = completed_record("task-1", "baseline", 1);
        let second = completed_record("task-1", "treatment", 1);

        append_completed_run_record(&path, &first).unwrap();
        append_completed_run_record(&path, &second).unwrap();

        let serialized = fs::read_to_string(&path).unwrap();
        let non_empty_lines = serialized
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();

        assert_eq!(non_empty_lines, 3);
        assert_eq!(
            read_completed_run_records(&path).unwrap(),
            vec![first, second]
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn shareable_completed_run_record_excludes_private_local_notes() {
        let manifest = manifest();
        let plan = plan_run(
            ["--task", "task-1", "--profile", "baseline"],
            &context(&manifest),
        )
        .unwrap();
        let completion = CompletionRecord::new(
            plan.run_metadata.run_id.to_string(),
            "task-1",
            "Done when the observable behavior passes.",
            CompletionStatus::Completed,
            Some("PRIVATE local debugging note"),
            1_725_000_050,
        )
        .unwrap();
        let record =
            completed_run_record_from_completion(&plan, &completion, 1_725_000_051_123).unwrap();
        let serialized = serialize_completed_run_records(&[record.clone()]);

        assert_eq!(record.run_id, plan.run_metadata.run_id.to_string());
        assert_eq!(record.task_id, "task-1");
        assert_eq!(record.profile_id, "baseline");
        assert_eq!(record.repetition, 1);
        assert_eq!(record.status, CompletionStatus::Completed);
        assert_eq!(record.adapter, "codex-cli");
        assert_eq!(record.started_at_ms, 1_725_000_000_123);
        assert_eq!(record.ended_at_ms, Some(1_725_000_051_123));
        assert_eq!(record.completed_at_unix, 1_725_000_050);
        assert!(!serialized.contains("PRIVATE local debugging note"));
        assert!(!serialized.contains("local_note"));
        assert!(!serialized.contains("done_condition"));
    }

    #[test]
    fn next_selection_uses_completed_records_loaded_from_store() {
        let path = temp_store_path("next-selection");
        let records = vec![
            completed_record("task-1", "baseline", 1),
            completed_record("task-1", "treatment", 1),
            completed_record("task-2", "baseline", 1),
            CompletedRunRecord::new(
                "run-task-2-treatment-failed",
                "task-2",
                "treatment",
                1,
                CompletionStatus::Failed,
                "codex-cli",
                1_725_000_020_000,
                Some(1_725_000_021_000),
                1_725_000_022,
            )
            .unwrap(),
        ];
        write_completed_run_records(&path, &records).unwrap();

        let manifest = manifest();
        let loaded_records = read_completed_run_records(&path).unwrap();
        let completed_runs = completed_runs_for_scheduler(&loaded_records);
        let context = context(&manifest).with_completed_runs(&completed_runs);
        let plan = plan_run(["--next"], &context).unwrap();

        assert_eq!(
            plan.selection,
            RunSelection::Next {
                repetition: 1,
                pending_before_selection: 17,
            }
        );
        assert_eq!(plan.manifest_task_id, "task-2");
        assert_eq!(plan.profile_id, "treatment");

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn persisted_records_have_metadata_needed_for_completed_run_report_math() {
        let manifest = manifest();
        let plan = plan_run(["--next"], &context(&manifest)).unwrap();
        let completion = CompletionRecord::new(
            plan.run_metadata.run_id.to_string(),
            plan.manifest_task_id.clone(),
            "Done when the task has a verifiable outcome.",
            CompletionStatus::Completed,
            Option::<String>::None,
            1_725_000_060,
        )
        .unwrap();

        let record =
            completed_run_record_from_completion(&plan, &completion, 1_725_000_061_123).unwrap();

        assert_eq!(record.run_id, plan.run_metadata.run_id.to_string());
        assert_eq!(record.task_id, plan.manifest_task_id);
        assert_eq!(record.profile_id, plan.profile_id);
        assert_eq!(record.repetition, 1);
        assert_eq!(record.status, CompletionStatus::Completed);
        assert_eq!(record.adapter, "codex-cli");
        assert_eq!(record.started_at_ms, plan.run_metadata.started_at_ms);
        assert_eq!(record.ended_at_ms, Some(1_725_000_061_123));
        assert_eq!(record.completed_at_unix, completion.timestamp);
        assert_eq!(
            record.to_completed_run(),
            Some(CompletedRun::new("task-1", "baseline", 1))
        );
    }
}
