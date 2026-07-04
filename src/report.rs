use std::collections::{BTreeMap, BTreeSet};

use crate::core::CaptureMode;

pub type EventClass = String;
pub type Profile = String;
pub type RunId = String;
pub type TaskId = String;
pub type TokenCount = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus {
    Incomplete,
    Completed,
    Failed,
}

impl CompletionStatus {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Failed, _) | (_, Self::Failed) => Self::Failed,
            (Self::Completed, _) | (_, Self::Completed) => Self::Completed,
            (Self::Incomplete, Self::Incomplete) => Self::Incomplete,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextEvent {
    pub mode: CaptureMode,
    pub class: EventClass,
    pub context_tokens: TokenCount,
    pub generated: bool,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
    pub completion_status: CompletionStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ContextKey {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
    pub class: EventClass,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TaskKey {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompletionCounts {
    pub completed: usize,
    pub failed: usize,
    pub incomplete: usize,
}

impl CompletionCounts {
    pub fn total(&self) -> usize {
        self.completed + self.failed + self.incomplete
    }

    pub fn completion_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            self.completed as f64 / total as f64
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub total_context_tokens_by_key: BTreeMap<ContextKey, TokenCount>,
    pub total_context_tokens: TokenCount,
    pub generated_context_tokens: TokenCount,
    pub generated_file_share: f64,
    pub task_completion: CompletionCounts,
    pub run_completion: CompletionCounts,
    pub tokens_per_completed_task_excluding_failed_runs: Option<f64>,
}

pub fn aggregate_report(events: &[ContextEvent]) -> Report {
    let mut total_context_tokens_by_key = BTreeMap::new();
    let mut total_context_tokens = 0;
    let mut generated_context_tokens = 0;
    let mut task_statuses: BTreeMap<TaskKey, CompletionStatus> = BTreeMap::new();
    let mut task_tokens: BTreeMap<TaskKey, TokenCount> = BTreeMap::new();

    for event in events {
        let context_key = ContextKey {
            run_id: event.run_id.clone(),
            task_id: event.task_id.clone(),
            profile: event.profile.clone(),
            class: event.class.clone(),
        };
        *total_context_tokens_by_key.entry(context_key).or_insert(0) += event.context_tokens;

        let task_key = TaskKey {
            run_id: event.run_id.clone(),
            task_id: event.task_id.clone(),
            profile: event.profile.clone(),
        };
        task_statuses
            .entry(task_key.clone())
            .and_modify(|status| *status = status.merge(event.completion_status))
            .or_insert(event.completion_status);
        if event.mode == CaptureMode::Task {
            *task_tokens.entry(task_key).or_insert(0) += event.context_tokens;
        }

        total_context_tokens += event.context_tokens;
        if event.generated {
            generated_context_tokens += event.context_tokens;
        }
    }

    let mut task_completion = CompletionCounts::default();
    let mut run_statuses: BTreeMap<&str, CompletionStatus> = BTreeMap::new();

    for (task_key, status) in &task_statuses {
        add_completion_count(&mut task_completion, *status);
        run_statuses
            .entry(task_key.run_id.as_str())
            .and_modify(|run_status| *run_status = merge_run_status(*run_status, *status))
            .or_insert(*status);
    }

    let mut run_completion = CompletionCounts::default();
    let mut failed_runs = BTreeSet::new();
    for (run_id, status) in run_statuses {
        add_completion_count(&mut run_completion, status);
        if status == CompletionStatus::Failed {
            failed_runs.insert(run_id);
        }
    }

    let mut completed_task_count = 0;
    let mut completed_task_tokens = 0;
    for (task_key, status) in &task_statuses {
        if *status == CompletionStatus::Completed
            && !failed_runs.contains(task_key.run_id.as_str())
            && task_tokens.contains_key(task_key)
        {
            completed_task_count += 1;
            completed_task_tokens += task_tokens.get(task_key).copied().unwrap_or(0);
        }
    }

    Report {
        total_context_tokens_by_key,
        total_context_tokens,
        generated_context_tokens,
        generated_file_share: share(generated_context_tokens, total_context_tokens),
        task_completion,
        run_completion,
        tokens_per_completed_task_excluding_failed_runs: if completed_task_count == 0 {
            None
        } else {
            Some(completed_task_tokens as f64 / completed_task_count as f64)
        },
    }
}

fn add_completion_count(counts: &mut CompletionCounts, status: CompletionStatus) {
    match status {
        CompletionStatus::Completed => counts.completed += 1,
        CompletionStatus::Failed => counts.failed += 1,
        CompletionStatus::Incomplete => counts.incomplete += 1,
    }
}

fn merge_run_status(current: CompletionStatus, task_status: CompletionStatus) -> CompletionStatus {
    match (current, task_status) {
        (CompletionStatus::Failed, _) | (_, CompletionStatus::Failed) => CompletionStatus::Failed,
        (CompletionStatus::Incomplete, _) | (_, CompletionStatus::Incomplete) => {
            CompletionStatus::Incomplete
        }
        (CompletionStatus::Completed, CompletionStatus::Completed) => CompletionStatus::Completed,
    }
}

fn share(part: TokenCount, total: TokenCount) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        run_id: &str,
        task_id: &str,
        profile: &str,
        class: &str,
        context_tokens: TokenCount,
        generated: bool,
        completion_status: CompletionStatus,
    ) -> ContextEvent {
        ContextEvent {
            mode: CaptureMode::Task,
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            profile: profile.to_string(),
            class: class.to_string(),
            context_tokens,
            generated,
            completion_status,
        }
    }

    fn key(run_id: &str, task_id: &str, profile: &str, class: &str) -> ContextKey {
        ContextKey {
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            profile: profile.to_string(),
            class: class.to_string(),
        }
    }

    fn assert_float_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < f64::EPSILON,
            "expected {}, got {}",
            expected,
            actual
        );
    }

    #[test]
    fn aggregates_context_tokens_by_run_task_profile_and_class() {
        let report = aggregate_report(&[
            event(
                "run-1",
                "task-1",
                "default",
                "prompt",
                100,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-1",
                "default",
                "prompt",
                50,
                true,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-1",
                "default",
                "tool",
                30,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-1",
                "large",
                "prompt",
                25,
                false,
                CompletionStatus::Completed,
            ),
        ]);

        assert_eq!(
            report
                .total_context_tokens_by_key
                .get(&key("run-1", "task-1", "default", "prompt")),
            Some(&150)
        );
        assert_eq!(
            report
                .total_context_tokens_by_key
                .get(&key("run-1", "task-1", "default", "tool")),
            Some(&30)
        );
        assert_eq!(
            report
                .total_context_tokens_by_key
                .get(&key("run-1", "task-1", "large", "prompt")),
            Some(&25)
        );
        assert_eq!(report.total_context_tokens, 205);
    }

    #[test]
    fn reports_generated_file_share_weighted_by_context_tokens() {
        let report = aggregate_report(&[
            event(
                "run-1",
                "task-1",
                "default",
                "prompt",
                60,
                true,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-2",
                "default",
                "prompt",
                140,
                false,
                CompletionStatus::Completed,
            ),
        ]);

        assert_eq!(report.generated_context_tokens, 60);
        assert_float_eq(report.generated_file_share, 0.3);
    }

    #[test]
    fn computes_task_and_run_completion_rates() {
        let report = aggregate_report(&[
            event(
                "run-1",
                "task-1",
                "default",
                "prompt",
                100,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-2",
                "default",
                "prompt",
                100,
                false,
                CompletionStatus::Incomplete,
            ),
            event(
                "run-2",
                "task-3",
                "default",
                "prompt",
                100,
                false,
                CompletionStatus::Failed,
            ),
        ]);

        assert_eq!(
            report.task_completion,
            CompletionCounts {
                completed: 1,
                failed: 1,
                incomplete: 1
            }
        );
        assert_float_eq(report.task_completion.completion_rate(), 1.0 / 3.0);

        assert_eq!(
            report.run_completion,
            CompletionCounts {
                completed: 0,
                failed: 1,
                incomplete: 1
            }
        );
        assert_float_eq(report.run_completion.completion_rate(), 0.0);
    }

    #[test]
    fn completed_status_wins_over_incomplete_but_failed_wins_over_all() {
        let report = aggregate_report(&[
            event(
                "run-1",
                "task-1",
                "default",
                "prompt",
                10,
                false,
                CompletionStatus::Incomplete,
            ),
            event(
                "run-1",
                "task-1",
                "default",
                "tool",
                20,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-2",
                "task-2",
                "default",
                "prompt",
                30,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-2",
                "task-2",
                "default",
                "tool",
                40,
                false,
                CompletionStatus::Failed,
            ),
        ]);

        assert_eq!(
            report.task_completion,
            CompletionCounts {
                completed: 1,
                failed: 1,
                incomplete: 0
            }
        );
    }

    #[test]
    fn computes_tokens_per_completed_task_excluding_failed_runs() {
        let report = aggregate_report(&[
            event(
                "run-1",
                "task-1",
                "default",
                "prompt",
                100,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-1",
                "default",
                "tool",
                20,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-1",
                "task-2",
                "default",
                "prompt",
                80,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-2",
                "task-3",
                "default",
                "prompt",
                1_000,
                false,
                CompletionStatus::Completed,
            ),
            event(
                "run-2",
                "task-4",
                "default",
                "prompt",
                100,
                false,
                CompletionStatus::Failed,
            ),
            event(
                "run-3",
                "task-5",
                "default",
                "prompt",
                500,
                false,
                CompletionStatus::Incomplete,
            ),
        ]);

        assert_eq!(
            report.tokens_per_completed_task_excluding_failed_runs,
            Some(100.0)
        );
    }

    #[test]
    fn passive_events_do_not_affect_tokens_per_completed_task_math() {
        let mut passive = event(
            "passive-run",
            "adhoc",
            "adhoc",
            "file.read",
            10_000,
            false,
            CompletionStatus::Completed,
        );
        passive.mode = CaptureMode::Passive;
        let report = aggregate_report(&[
            passive,
            event(
                "task-run",
                "task-1",
                "baseline",
                "file.read",
                100,
                false,
                CompletionStatus::Completed,
            ),
        ]);

        assert_eq!(
            report.tokens_per_completed_task_excluding_failed_runs,
            Some(100.0)
        );
    }

    #[test]
    fn empty_input_has_zero_rates_and_no_completed_task_average() {
        let report = aggregate_report(&[]);

        assert!(report.total_context_tokens_by_key.is_empty());
        assert_eq!(report.total_context_tokens, 0);
        assert_eq!(report.generated_context_tokens, 0);
        assert_float_eq(report.generated_file_share, 0.0);
        assert_float_eq(report.task_completion.completion_rate(), 0.0);
        assert_float_eq(report.run_completion.completion_rate(), 0.0);
        assert_eq!(report.tokens_per_completed_task_excluding_failed_runs, None);
    }
}
