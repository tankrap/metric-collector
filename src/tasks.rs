use std::collections::HashMap;
use std::error::Error;
use std::fmt;

/// A single task entry from a task manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub done: bool,
}

/// A parsed task manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskManifest {
    pub tasks: Vec<Task>,
}

/// Errors returned while parsing or validating a task manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    MissingTasksKey,
    UnexpectedTopLevel {
        line: usize,
        content: String,
    },
    UnexpectedLine {
        line: usize,
        content: String,
    },
    FieldBeforeTask {
        line: usize,
        field: String,
    },
    UnknownField {
        line: usize,
        field: String,
    },
    DuplicateField {
        line: usize,
        field: String,
    },
    MissingField {
        task_index: usize,
        field: &'static str,
    },
    EmptyField {
        line: usize,
        field: &'static str,
    },
    InvalidDone {
        line: usize,
        value: String,
    },
    DuplicateId {
        id: String,
        first_task: usize,
        duplicate_task: usize,
    },
    TaskCount {
        count: usize,
        min: usize,
        max: usize,
    },
}

impl TaskManifest {
    pub const MIN_COMPARISON_TASKS: usize = 5;
    pub const MAX_COMPARISON_TASKS: usize = 15;

    /// Parse the starter `tasks.yaml` subset used by comparison manifests.
    ///
    /// This is intentionally not a general YAML parser. It accepts only:
    ///
    /// ```text
    /// tasks:
    ///   - id: task-1
    ///     title: Task title
    ///     description: Optional description
    ///     done: false
    /// ```
    ///
    /// Supported scalars are single-line values, optionally wrapped in matching
    /// single or double quotes. Full-line comments and blank lines are ignored.
    /// Replace this explicit parser with `serde_yaml` once external
    /// dependencies are allowed for this crate.
    pub fn parse(input: &str) -> Result<Self, ManifestError> {
        let mut saw_tasks_key = false;
        let mut current = None;
        let mut builders = Vec::new();

        for (line_offset, raw_line) in input.lines().enumerate() {
            let line = line_offset + 1;
            let trimmed = raw_line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if !saw_tasks_key {
                if trimmed == "tasks:" {
                    saw_tasks_key = true;
                    continue;
                }

                return Err(ManifestError::UnexpectedTopLevel {
                    line,
                    content: trimmed.to_string(),
                });
            }

            if trimmed == "tasks:" {
                return Err(ManifestError::UnexpectedTopLevel {
                    line,
                    content: trimmed.to_string(),
                });
            }

            let leading_trimmed = raw_line.trim_start();

            if raw_line == leading_trimmed {
                return Err(ManifestError::UnexpectedTopLevel {
                    line,
                    content: trimmed.to_string(),
                });
            }

            if let Some(item_body) = leading_trimmed.strip_prefix("- ") {
                if let Some(builder) = current.take() {
                    builders.push(builder);
                }

                let mut builder = TaskBuilder::default();
                let item_body = item_body.trim();
                if !item_body.is_empty() {
                    let (field, value) = parse_field(line, item_body)?;
                    builder.set(line, field, value)?;
                }
                current = Some(builder);
                continue;
            }

            if leading_trimmed == "-" {
                if let Some(builder) = current.take() {
                    builders.push(builder);
                }
                current = Some(TaskBuilder::default());
                continue;
            }

            let (field, value) = parse_field(line, leading_trimmed)?;
            let Some(builder) = current.as_mut() else {
                return Err(ManifestError::FieldBeforeTask { line, field });
            };
            builder.set(line, field, value)?;
        }

        if !saw_tasks_key {
            return Err(ManifestError::MissingTasksKey);
        }

        if let Some(builder) = current {
            builders.push(builder);
        }

        let mut tasks = Vec::with_capacity(builders.len());
        let mut seen_ids: HashMap<String, usize> = HashMap::new();

        for (index, builder) in builders.into_iter().enumerate() {
            let task_index = index + 1;
            let task = builder.build(task_index)?;

            if let Some(first_task) = seen_ids.insert(task.id.clone(), task_index) {
                return Err(ManifestError::DuplicateId {
                    id: task.id,
                    first_task,
                    duplicate_task: task_index,
                });
            }

            tasks.push(task);
        }

        let manifest = Self { tasks };
        manifest.validate_for_comparison()?;
        Ok(manifest)
    }

    pub fn from_yaml(input: &str) -> Result<Self, ManifestError> {
        Self::parse(input)
    }

    pub fn validate_for_comparison(&self) -> Result<(), ManifestError> {
        let count = self.tasks.len();
        let min = Self::MIN_COMPARISON_TASKS;
        let max = Self::MAX_COMPARISON_TASKS;

        if !(min..=max).contains(&count) {
            return Err(ManifestError::TaskCount { count, min, max });
        }

        Ok(())
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTasksKey => write!(formatter, "manifest is missing required `tasks:` key"),
            Self::UnexpectedTopLevel { line, content } => {
                write!(
                    formatter,
                    "line {line}: unexpected top-level content `{content}`"
                )
            }
            Self::UnexpectedLine { line, content } => {
                write!(
                    formatter,
                    "line {line}: unsupported manifest line `{content}`"
                )
            }
            Self::FieldBeforeTask { line, field } => {
                write!(
                    formatter,
                    "line {line}: field `{field}` appears before any task"
                )
            }
            Self::UnknownField { line, field } => {
                write!(formatter, "line {line}: unsupported task field `{field}`")
            }
            Self::DuplicateField { line, field } => {
                write!(formatter, "line {line}: duplicate task field `{field}`")
            }
            Self::MissingField { task_index, field } => {
                write!(
                    formatter,
                    "task {task_index}: missing required field `{field}`"
                )
            }
            Self::EmptyField { line, field } => {
                write!(formatter, "line {line}: field `{field}` must not be empty")
            }
            Self::InvalidDone { line, value } => {
                write!(
                    formatter,
                    "line {line}: `done` must be `true` or `false`, got `{value}`"
                )
            }
            Self::DuplicateId {
                id,
                first_task,
                duplicate_task,
            } => write!(
                formatter,
                "duplicate task id `{id}` at tasks {first_task} and {duplicate_task}"
            ),
            Self::TaskCount { count, min, max } => write!(
                formatter,
                "comparison manifests must contain {min}-{max} tasks, got {count}"
            ),
        }
    }
}

impl Error for ManifestError {}

#[derive(Debug, Default)]
struct TaskBuilder {
    id: Option<FieldValue>,
    title: Option<FieldValue>,
    description: Option<FieldValue>,
    done: Option<FieldValue>,
}

impl TaskBuilder {
    fn set(&mut self, line: usize, field: String, value: String) -> Result<(), ManifestError> {
        match field.as_str() {
            "id" => set_once(&mut self.id, line, field, value),
            "title" => set_once(&mut self.title, line, field, value),
            "description" => set_once(&mut self.description, line, field, value),
            "done" => set_once(&mut self.done, line, field, value),
            _ => Err(ManifestError::UnknownField { line, field }),
        }
    }

    fn build(self, task_index: usize) -> Result<Task, ManifestError> {
        let id = required_value(self.id, task_index, "id")?;
        let title = required_value(self.title, task_index, "title")?;
        let done = required_value(self.done, task_index, "done")?;
        let description = self
            .description
            .map(|field| field.value)
            .unwrap_or_default();

        let done = match done.value.as_str() {
            "true" => true,
            "false" => false,
            _ => {
                return Err(ManifestError::InvalidDone {
                    line: done.line,
                    value: done.value,
                });
            }
        };

        Ok(Task {
            id: id.value,
            title: title.value,
            description,
            done,
        })
    }
}

#[derive(Debug)]
struct FieldValue {
    line: usize,
    value: String,
}

fn set_once(
    slot: &mut Option<FieldValue>,
    line: usize,
    field: String,
    value: String,
) -> Result<(), ManifestError> {
    if slot.is_some() {
        return Err(ManifestError::DuplicateField { line, field });
    }

    *slot = Some(FieldValue { line, value });
    Ok(())
}

fn required_value(
    field: Option<FieldValue>,
    task_index: usize,
    field_name: &'static str,
) -> Result<FieldValue, ManifestError> {
    let Some(field) = field else {
        return Err(ManifestError::MissingField {
            task_index,
            field: field_name,
        });
    };

    if field.value.is_empty() {
        return Err(ManifestError::EmptyField {
            line: field.line,
            field: field_name,
        });
    }

    Ok(field)
}

fn parse_field(line: usize, input: &str) -> Result<(String, String), ManifestError> {
    let Some((key, value)) = input.split_once(':') else {
        return Err(ManifestError::UnexpectedLine {
            line,
            content: input.to_string(),
        });
    };

    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return Err(ManifestError::UnexpectedLine {
            line,
            content: input.to_string(),
        });
    }

    Ok((key.to_string(), unquote_scalar(value.trim())))
}

fn unquote_scalar(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];

        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return value[1..value.len() - 1].to_string();
        }
    }

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MANIFEST: &str = r#"
# Full-line comments are ignored.
tasks:
  - id: task-1
    title: First task
    description: Has a colon: still one scalar
    done: false
  - id: task-2
    title: Second task
    description: Done already
    done: true
  - id: task-3
    title: Third task
    description: ""
    done: false
  - id: task-4
    title: Fourth task
    done: false
  - id: task-5
    title: Fifth task
    description: 'single quoted'
    done: true
"#;

    #[test]
    fn parses_valid_manifest() {
        let manifest = TaskManifest::parse(VALID_MANIFEST).expect("valid manifest parses");

        assert_eq!(manifest.tasks.len(), 5);
        assert_eq!(manifest.tasks[0].id, "task-1");
        assert_eq!(manifest.tasks[0].title, "First task");
        assert_eq!(
            manifest.tasks[0].description,
            "Has a colon: still one scalar"
        );
        assert!(!manifest.tasks[0].done);
        assert!(manifest.tasks[1].done);
        assert_eq!(manifest.tasks[2].description, "");
        assert_eq!(manifest.tasks[3].description, "");
        assert_eq!(manifest.tasks[4].description, "single quoted");
    }

    #[test]
    fn from_yaml_alias_uses_same_parser() {
        assert_eq!(
            TaskManifest::from_yaml(VALID_MANIFEST),
            TaskManifest::parse(VALID_MANIFEST)
        );
    }

    #[test]
    fn rejects_missing_tasks_key() {
        let error = TaskManifest::parse("").unwrap_err();

        assert_eq!(error, ManifestError::MissingTasksKey);
    }

    #[test]
    fn rejects_too_few_tasks() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id: only
    title: Only task
    done: false
"#,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ManifestError::TaskCount {
                count: 1,
                min: 5,
                max: 15
            }
        ));
    }

    #[test]
    fn rejects_too_many_tasks() {
        let mut manifest = String::from("tasks:\n");
        for index in 1..=16 {
            manifest.push_str(&format!(
                "  - id: task-{index}\n    title: Task {index}\n    done: false\n"
            ));
        }

        let error = TaskManifest::parse(&manifest).unwrap_err();

        assert!(matches!(
            error,
            ManifestError::TaskCount {
                count: 16,
                min: 5,
                max: 15
            }
        ));
    }

    #[test]
    fn rejects_missing_required_field() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id: task-1
    title: Task 1
    done: false
  - id: task-2
    title: Task 2
    done: false
  - id: task-3
    title: Task 3
    done: false
  - id: task-4
    title: Task 4
    done: false
  - id: task-5
    title: Task 5
"#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ManifestError::MissingField {
                task_index: 5,
                field: "done"
            }
        );
    }

    #[test]
    fn rejects_empty_required_field() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id:
    title: Task 1
    done: false
  - id: task-2
    title: Task 2
    done: false
  - id: task-3
    title: Task 3
    done: false
  - id: task-4
    title: Task 4
    done: false
  - id: task-5
    title: Task 5
    done: false
"#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ManifestError::EmptyField {
                line: 3,
                field: "id"
            }
        );
    }

    #[test]
    fn rejects_duplicate_ids() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id: task-1
    title: Task 1
    done: false
  - id: task-2
    title: Task 2
    done: false
  - id: task-3
    title: Task 3
    done: false
  - id: task-1
    title: Task 4
    done: false
  - id: task-5
    title: Task 5
    done: false
"#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ManifestError::DuplicateId {
                id: "task-1".to_string(),
                first_task: 1,
                duplicate_task: 4
            }
        );
    }

    #[test]
    fn rejects_invalid_done_value() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id: task-1
    title: Task 1
    done: no
  - id: task-2
    title: Task 2
    done: false
  - id: task-3
    title: Task 3
    done: false
  - id: task-4
    title: Task 4
    done: false
  - id: task-5
    title: Task 5
    done: false
"#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ManifestError::InvalidDone {
                line: 5,
                value: "no".to_string()
            }
        );
    }

    #[test]
    fn rejects_duplicate_fields() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id: task-1
    id: task-1-again
    title: Task 1
    done: false
  - id: task-2
    title: Task 2
    done: false
  - id: task-3
    title: Task 3
    done: false
  - id: task-4
    title: Task 4
    done: false
  - id: task-5
    title: Task 5
    done: false
"#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ManifestError::DuplicateField {
                line: 4,
                field: "id".to_string()
            }
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = TaskManifest::parse(
            r#"
tasks:
  - id: task-1
    title: Task 1
    owner: somebody
    done: false
  - id: task-2
    title: Task 2
    done: false
  - id: task-3
    title: Task 3
    done: false
  - id: task-4
    title: Task 4
    done: false
  - id: task-5
    title: Task 5
    done: false
"#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            ManifestError::UnknownField {
                line: 5,
                field: "owner".to_string()
            }
        );
    }
}
