use std::error::Error;
use std::fmt;

use crate::mode::ModeState;
use crate::run::{Profile as RunProfile, RunMetadata};
use crate::scheduler::{
    Profile as SchedulerProfile, RunKey, RunMatrixEntry, RunMatrixScheduler, RunState,
};
use crate::tasks::{Task, TaskManifest};

const DEFAULT_RUN_ADAPTER: &str = "tokmeter";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CaptureMode;

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
}
