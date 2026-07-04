use std::error::Error;
use std::fmt;

pub const DEFAULT_MIN_REPETITIONS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Profile {
    Baseline,
    Treatment,
}

impl Profile {
    pub const ALL: [Self; 2] = [Self::Baseline, Self::Treatment];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Treatment => "treatment",
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RunState {
    Pending,
    Completed,
    Failed,
    Skipped,
}

impl RunState {
    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RunKey {
    pub task_id: String,
    pub profile: Profile,
    pub repetition: usize,
}

impl RunKey {
    pub fn new(task_id: impl Into<String>, profile: Profile, repetition: usize) -> Self {
        Self {
            task_id: task_id.into(),
            profile,
            repetition,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunMatrixEntry {
    pub task_id: String,
    pub profile: Profile,
    pub repetition: usize,
    pub state: RunState,
}

impl RunMatrixEntry {
    pub fn new(task_id: impl Into<String>, profile: Profile, repetition: usize) -> Self {
        Self {
            task_id: task_id.into(),
            profile,
            repetition,
            state: RunState::Pending,
        }
    }

    pub fn key(&self) -> RunKey {
        RunKey::new(self.task_id.clone(), self.profile, self.repetition)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunMatrixScheduler {
    entries: Vec<RunMatrixEntry>,
    repetitions: usize,
}

impl RunMatrixScheduler {
    pub fn new<I, S>(task_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_repetitions(task_ids, DEFAULT_MIN_REPETITIONS)
    }

    pub fn with_repetitions<I, S>(task_ids: I, repetitions: usize) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_repetition_minimum(task_ids, repetitions, DEFAULT_MIN_REPETITIONS)
    }

    pub fn with_repetition_minimum<I, S>(
        task_ids: I,
        repetitions: usize,
        minimum_repetitions: usize,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let task_ids: Vec<String> = task_ids.into_iter().map(Into::into).collect();
        let repetitions = effective_repetitions(repetitions, minimum_repetitions);
        let mut entries = Vec::with_capacity(task_ids.len() * Profile::ALL.len() * repetitions);

        for repetition in 1..=repetitions {
            for task_id in &task_ids {
                for profile in Profile::ALL {
                    entries.push(RunMatrixEntry::new(task_id.clone(), profile, repetition));
                }
            }
        }

        Self {
            entries,
            repetitions,
        }
    }

    pub fn from_entries(entries: Vec<RunMatrixEntry>) -> Self {
        let repetitions = entries
            .iter()
            .map(|entry| entry.repetition)
            .max()
            .unwrap_or(DEFAULT_MIN_REPETITIONS)
            .max(DEFAULT_MIN_REPETITIONS);

        Self {
            entries,
            repetitions,
        }
    }

    pub fn entries(&self) -> &[RunMatrixEntry] {
        &self.entries
    }

    pub fn repetitions(&self) -> usize {
        self.repetitions
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn next_run(&self) -> Option<&RunMatrixEntry> {
        self.entries.iter().find(|entry| entry.state.is_pending())
    }

    pub fn next_pending(&self) -> Option<&RunMatrixEntry> {
        self.next_run()
    }

    pub fn state(&self, key: &RunKey) -> Option<RunState> {
        self.entry(key).map(|entry| entry.state)
    }

    pub fn pending_count(&self) -> usize {
        self.count_by_state(RunState::Pending)
    }

    pub fn completed_count(&self) -> usize {
        self.count_by_state(RunState::Completed)
    }

    pub fn failed_count(&self) -> usize {
        self.count_by_state(RunState::Failed)
    }

    pub fn skipped_count(&self) -> usize {
        self.count_by_state(RunState::Skipped)
    }

    pub fn transition(&mut self, key: &RunKey, state: RunState) -> Result<(), SchedulerError> {
        let entry = self
            .entry_mut(key)
            .ok_or_else(|| SchedulerError::UnknownEntry(key.clone()))?;
        entry.state = state;
        Ok(())
    }

    pub fn mark_completed(&mut self, key: &RunKey) -> Result<(), SchedulerError> {
        self.transition(key, RunState::Completed)
    }

    pub fn mark_failed(&mut self, key: &RunKey) -> Result<(), SchedulerError> {
        self.transition(key, RunState::Failed)
    }

    pub fn mark_skipped(&mut self, key: &RunKey) -> Result<(), SchedulerError> {
        self.transition(key, RunState::Skipped)
    }

    fn count_by_state(&self, state: RunState) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == state)
            .count()
    }

    fn entry(&self, key: &RunKey) -> Option<&RunMatrixEntry> {
        self.entries.iter().find(|entry| entry.key() == *key)
    }

    fn entry_mut(&mut self, key: &RunKey) -> Option<&mut RunMatrixEntry> {
        self.entries.iter_mut().find(|entry| {
            entry.task_id == key.task_id
                && entry.profile == key.profile
                && entry.repetition == key.repetition
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchedulerError {
    UnknownEntry(RunKey),
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEntry(key) => write!(
                formatter,
                "unknown scheduler entry: task_id='{}' profile='{}' repetition={}",
                key.task_id, key.profile, key.repetition
            ),
        }
    }
}

impl Error for SchedulerError {}

pub fn effective_repetitions(repetitions: usize, minimum_repetitions: usize) -> usize {
    repetitions.max(minimum_repetitions.max(DEFAULT_MIN_REPETITIONS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_fixture() -> RunMatrixScheduler {
        RunMatrixScheduler::with_repetitions(["task-a", "task-b"], 2)
    }

    fn compact_entries(scheduler: &RunMatrixScheduler) -> Vec<String> {
        scheduler
            .entries()
            .iter()
            .map(|entry| {
                format!(
                    "{}:{}:{}:{:?}",
                    entry.task_id, entry.profile, entry.repetition, entry.state
                )
            })
            .collect()
    }

    #[test]
    fn orders_runs_by_repetition_then_task_then_interleaved_profile() {
        let scheduler = matrix_fixture();

        assert_eq!(
            compact_entries(&scheduler),
            vec![
                "task-a:baseline:1:Pending",
                "task-a:treatment:1:Pending",
                "task-b:baseline:1:Pending",
                "task-b:treatment:1:Pending",
                "task-a:baseline:2:Pending",
                "task-a:treatment:2:Pending",
                "task-b:baseline:2:Pending",
                "task-b:treatment:2:Pending",
            ]
        );
    }

    #[test]
    fn transitions_pending_entries_to_terminal_states() {
        let mut scheduler = matrix_fixture();
        let completed = RunKey::new("task-a", Profile::Baseline, 1);
        let failed = RunKey::new("task-a", Profile::Treatment, 1);
        let skipped = RunKey::new("task-b", Profile::Baseline, 1);

        scheduler.mark_completed(&completed).unwrap();
        scheduler.mark_failed(&failed).unwrap();
        scheduler.mark_skipped(&skipped).unwrap();

        assert_eq!(scheduler.state(&completed), Some(RunState::Completed));
        assert_eq!(scheduler.state(&failed), Some(RunState::Failed));
        assert_eq!(scheduler.state(&skipped), Some(RunState::Skipped));
        assert_eq!(scheduler.completed_count(), 1);
        assert_eq!(scheduler.failed_count(), 1);
        assert_eq!(scheduler.skipped_count(), 1);
        assert_eq!(scheduler.pending_count(), 5);
    }

    #[test]
    fn next_pending_skips_completed_failed_and_skipped_entries() {
        let mut scheduler = matrix_fixture();

        assert_eq!(
            scheduler.next_run().map(RunMatrixEntry::key),
            Some(RunKey::new("task-a", Profile::Baseline, 1))
        );

        scheduler
            .mark_completed(&RunKey::new("task-a", Profile::Baseline, 1))
            .unwrap();
        scheduler
            .mark_failed(&RunKey::new("task-a", Profile::Treatment, 1))
            .unwrap();
        scheduler
            .mark_skipped(&RunKey::new("task-b", Profile::Baseline, 1))
            .unwrap();

        assert_eq!(
            scheduler.next_pending().map(RunMatrixEntry::key),
            Some(RunKey::new("task-b", Profile::Treatment, 1))
        );

        let remaining_keys: Vec<RunKey> = scheduler
            .entries()
            .iter()
            .filter(|entry| entry.state == RunState::Pending)
            .map(RunMatrixEntry::key)
            .collect();
        for key in remaining_keys {
            scheduler.mark_completed(&key).unwrap();
        }

        assert_eq!(scheduler.next_run(), None);
    }

    #[test]
    fn fixture_matrix_is_deterministic_and_respects_minimum_repetitions() {
        let first = RunMatrixScheduler::with_repetitions(["alpha", "beta", "gamma"], 1);
        let second = RunMatrixScheduler::with_repetitions(["alpha", "beta", "gamma"], 1);

        assert_eq!(first, second);
        assert_eq!(first.repetitions(), DEFAULT_MIN_REPETITIONS);
        assert_eq!(first.len(), 12);
        assert_eq!(
            compact_entries(&first),
            vec![
                "alpha:baseline:1:Pending",
                "alpha:treatment:1:Pending",
                "beta:baseline:1:Pending",
                "beta:treatment:1:Pending",
                "gamma:baseline:1:Pending",
                "gamma:treatment:1:Pending",
                "alpha:baseline:2:Pending",
                "alpha:treatment:2:Pending",
                "beta:baseline:2:Pending",
                "beta:treatment:2:Pending",
                "gamma:baseline:2:Pending",
                "gamma:treatment:2:Pending",
            ]
        );

        let defaulted = RunMatrixScheduler::new(["alpha"]);
        assert_eq!(defaulted.repetitions(), DEFAULT_MIN_REPETITIONS);
        assert_eq!(defaulted.len(), 4);

        let explicit_minimum = RunMatrixScheduler::with_repetition_minimum(["alpha", "beta"], 2, 3);
        assert_eq!(explicit_minimum.repetitions(), 3);
        assert_eq!(explicit_minimum.len(), 12);
        assert_eq!(effective_repetitions(1, 1), DEFAULT_MIN_REPETITIONS);
    }

    #[test]
    fn unknown_transition_reports_key() {
        let mut scheduler = matrix_fixture();
        let key = RunKey::new("missing", Profile::Baseline, 1);

        let error = scheduler.mark_completed(&key).unwrap_err();

        assert_eq!(error, SchedulerError::UnknownEntry(key));
    }
}
