use crate::core::{ADHOC_TASK_ID, CaptureMode};

pub const DEFAULT_PASSIVE_PROFILE_ID: &str = "adhoc";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModeState {
    pub mode: CaptureMode,
    pub task_id: String,
    pub profile_id: String,
}

impl ModeState {
    pub fn passive() -> Self {
        Self {
            mode: CaptureMode::Passive,
            task_id: ADHOC_TASK_ID.to_owned(),
            profile_id: DEFAULT_PASSIVE_PROFILE_ID.to_owned(),
        }
    }

    pub fn task(
        task_id: impl Into<String>,
        profile_id: impl Into<String>,
    ) -> Result<Self, ModeStateError> {
        let task_id = task_id.into();
        let profile_id = profile_id.into();

        if task_id.trim().is_empty() || task_id == ADHOC_TASK_ID {
            return Err(ModeStateError::new(
                "task_id",
                "task mode requires a manifest task id",
            ));
        }
        if profile_id.trim().is_empty() {
            return Err(ModeStateError::new("profile_id", "profile is required"));
        }

        Ok(Self {
            mode: CaptureMode::Task,
            task_id,
            profile_id,
        })
    }

    pub fn from_optional_task_state(task_id: Option<&str>, profile_id: Option<&str>) -> Self {
        match (task_id, profile_id) {
            (Some(task_id), Some(profile_id)) => {
                Self::task(task_id, profile_id).unwrap_or_else(|_| Self::passive())
            }
            _ => Self::passive(),
        }
    }

    pub fn status_line(&self, events_today: u64, top_op_class: Option<&str>) -> String {
        format!(
            "mode={} task_id={} profile={} events_today={} top_op_class={}",
            self.mode.as_str(),
            self.task_id,
            self.profile_id,
            events_today,
            top_op_class.unwrap_or("n/a")
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModeStateError {
    pub field: &'static str,
    pub message: String,
}

impl ModeStateError {
    fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_defaults_to_adhoc_task() {
        let state = ModeState::passive();

        assert_eq!(state.mode, CaptureMode::Passive);
        assert_eq!(state.task_id, "adhoc");
        assert_eq!(state.profile_id, "adhoc");
    }

    #[test]
    fn task_mode_requires_manifest_task_id() {
        let error = ModeState::task("adhoc", "baseline").unwrap_err();

        assert_eq!(error.field, "task_id");
    }

    #[test]
    fn status_reports_current_mode_and_today_summary() {
        let state = ModeState::task("fix-login", "baseline").unwrap();

        assert_eq!(
            state.status_line(7, Some("vc.diff")),
            "mode=task task_id=fix-login profile=baseline events_today=7 top_op_class=vc.diff"
        );
    }
}
