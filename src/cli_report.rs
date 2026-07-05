use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::core::{AttributedTokenEvent, EventLog};
use crate::report_json;
use crate::report_markdown;
use crate::share::{ShareMap, ShareSanitizer, ShareValue, serialize_share_value};

pub const REPORT_JSON_FILE_NAME: &str = "report.json";
pub const REPORT_MARKDOWN_FILE_NAME: &str = "report.md";
pub const REPORT_SHARE_FILE_NAME: &str = "report.share.json";

const PASSIVE_PROFILE_ID: &str = "passive";
const PASSIVE_RUN_ID: &str = "first-local-report";
const FIRST_REPORT_WARNING: &str =
    "Passive self-report fixture: no event log was found; metrics are observational.";
const EVENT_LOG_REPORT_WARNING: &str =
    "Event-log report: Grade O observational metrics; controlled efficiency claims require Mode T.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliReportPaths {
    pub output_dir: PathBuf,
    pub report_json: PathBuf,
    pub report_markdown: PathBuf,
    pub report_share: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliReportSource {
    PassiveSelfReportFixture,
    EventLogAggregation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliReportArtifacts {
    pub paths: CliReportPaths,
    pub source: CliReportSource,
}

pub fn report_output_paths(output_dir: impl AsRef<Path>) -> CliReportPaths {
    let output_dir = output_dir.as_ref().to_path_buf();
    CliReportPaths {
        report_json: output_dir.join(REPORT_JSON_FILE_NAME),
        report_markdown: output_dir.join(REPORT_MARKDOWN_FILE_NAME),
        report_share: output_dir.join(REPORT_SHARE_FILE_NAME),
        output_dir,
    }
}

pub fn render_report_output_paths(paths: &CliReportPaths) -> String {
    format!(
        "report.json: {}\nreport.md: {}",
        paths.report_json.display(),
        paths.report_markdown.display()
    )
}

pub fn create_first_report_artifacts(
    output_dir: impl AsRef<Path>,
    event_log: Option<impl AsRef<Path>>,
) -> io::Result<CliReportArtifacts> {
    let should_use_fixture = match event_log.as_ref() {
        Some(path) => event_log_is_missing_or_empty(path.as_ref())?,
        None => true,
    };

    if !should_use_fixture {
        let event_log = event_log.expect("checked Some");
        return create_event_log_report_artifacts(output_dir, event_log.as_ref());
    }

    let paths = report_output_paths(output_dir);
    fs::create_dir_all(&paths.output_dir)?;
    fs::write(&paths.report_json, passive_self_report_json())?;
    fs::write(&paths.report_markdown, passive_self_report_markdown())?;

    Ok(CliReportArtifacts {
        paths,
        source: CliReportSource::PassiveSelfReportFixture,
    })
}

pub fn create_report_share_artifact(
    artifacts: &CliReportArtifacts,
    salt: &str,
) -> io::Result<PathBuf> {
    let report_json = fs::read_to_string(&artifacts.paths.report_json)?;
    let report_markdown = fs::read_to_string(&artifacts.paths.report_markdown)?;
    let share_value = report_share_value(artifacts, &report_json, &report_markdown);
    let sanitized = ShareSanitizer::new(salt).sanitize(&share_value);
    fs::write(
        &artifacts.paths.report_share,
        serialize_share_value(&sanitized),
    )?;
    Ok(artifacts.paths.report_share.clone())
}

fn event_log_is_missing_or_empty(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() == 0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn create_event_log_report_artifacts(
    output_dir: impl AsRef<Path>,
    event_log: impl AsRef<Path>,
) -> io::Result<CliReportArtifacts> {
    let log = EventLog::read_file(event_log.as_ref()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read event log: {error}"),
        )
    })?;

    if log.events.is_empty() {
        return create_first_report_artifacts(output_dir, Option::<&Path>::None);
    }

    let paths = report_output_paths(output_dir);
    fs::create_dir_all(&paths.output_dir)?;
    fs::write(&paths.report_json, event_log_report_json(&log.events))?;
    fs::write(
        &paths.report_markdown,
        event_log_report_markdown(&log.events),
    )?;

    Ok(CliReportArtifacts {
        paths,
        source: CliReportSource::EventLogAggregation,
    })
}

fn event_log_report_json(events: &[AttributedTokenEvent]) -> String {
    let aggregate = EventLogAggregate::from_events(events);
    let run_profile_totals = aggregate
        .run_profile_totals
        .iter()
        .map(|item| report_json::RunProfileTotals {
            run_id: item.run_id.as_str(),
            profile_id: item.profile_id.as_str(),
            totals: item.totals,
        })
        .collect::<Vec<_>>();
    let class_shares = aggregate
        .class_totals
        .iter()
        .map(|(operation_class, tokens)| report_json::ClassShare {
            operation_class: operation_class.as_str(),
            tokens: *tokens,
            share: ratio_or_zero(*tokens, aggregate.totals.total_tokens),
        })
        .collect::<Vec<_>>();
    let warnings = [EVENT_LOG_REPORT_WARNING];

    report_json::serialize_report_json(&report_json::ReportJson {
        evidence_grade: report_json::EvidenceGrade::GradeO,
        totals: aggregate.totals,
        run_profile_totals: &run_profile_totals,
        class_shares: &class_shares,
        completion_rates: aggregate.completion_rates(),
        repeat_metrics: aggregate.repeat_metrics(),
        cache_metrics: aggregate.cache_metrics(),
        calibration: report_json::CalibrationMetadata {
            source: Some("event-log"),
            tokenizer: Some("event-log"),
            calibration_id: None,
            sample_count: 0,
            mean_absolute_error: None,
        },
        warnings: &warnings,
    })
}

fn event_log_report_markdown(events: &[AttributedTokenEvent]) -> String {
    let aggregate = EventLogAggregate::from_events(events);
    let warnings = [EVENT_LOG_REPORT_WARNING];

    report_markdown::serialize_report_markdown(&report_markdown::MarkdownReport {
        title: "vc-tokmeter local event report",
        evidence_grade: report_markdown::EvidenceGrade::GradeO,
        single_profile: Some(report_markdown::ProfileSummary {
            profile_id: aggregate.markdown_profile_id(),
            event_count: aggregate.totals.event_count,
            run_count: aggregate.totals.run_count,
            task_count: aggregate.totals.task_count,
            input_tokens: aggregate.totals.input_tokens,
            output_tokens: aggregate.totals.output_tokens,
            cache_read_tokens: aggregate.totals.cache_read_tokens,
            cache_write_tokens: aggregate.totals.cache_write_tokens,
            total_tokens: aggregate.totals.total_tokens,
            byte_count: aggregate.totals.byte_count,
            completion_rates: report_markdown::CompletionRates {
                tasks: completion_rate_to_markdown(aggregate.completion_rates().tasks),
                runs: completion_rate_to_markdown(aggregate.completion_rates().runs),
            },
            token_distribution: None,
        }),
        comparison: None,
        warnings: &warnings,
    })
}

fn report_share_value(
    artifacts: &CliReportArtifacts,
    report_json: &str,
    report_markdown: &str,
) -> ShareValue {
    let mut root = ShareMap::new();
    root.insert("schema_version".to_owned(), ShareValue::U64(1));
    root.insert(
        "artifact_type".to_owned(),
        ShareValue::String("vc-tokmeter.report-share".to_owned()),
    );
    root.insert(
        "source".to_owned(),
        ShareValue::String(
            match artifacts.source {
                CliReportSource::PassiveSelfReportFixture => "passive-self-report-fixture",
                CliReportSource::EventLogAggregation => "event-log-aggregation",
            }
            .to_owned(),
        ),
    );
    root.insert(
        "reports".to_owned(),
        ShareValue::List(vec![
            report_file_share_value("json", &artifacts.paths.report_json, report_json),
            report_file_share_value(
                "markdown",
                &artifacts.paths.report_markdown,
                report_markdown,
            ),
        ]),
    );
    ShareValue::Map(root)
}

fn report_file_share_value(kind: &str, path: &Path, content: &str) -> ShareValue {
    let mut value = ShareMap::new();
    value.insert("kind".to_owned(), ShareValue::String(kind.to_owned()));
    value.insert(
        "path".to_owned(),
        ShareValue::String(path.display().to_string()),
    );
    value.insert("content".to_owned(), ShareValue::String(content.to_owned()));
    ShareValue::Map(value)
}

#[derive(Clone, Debug, PartialEq)]
struct OwnedRunProfileTotals {
    run_id: String,
    profile_id: String,
    totals: report_json::Totals,
}

#[derive(Clone, Debug, PartialEq)]
struct EventLogAggregate {
    totals: report_json::Totals,
    run_profile_totals: Vec<OwnedRunProfileTotals>,
    class_totals: Vec<(String, u64)>,
    profile_label: String,
    repeat_event_count: u64,
    repeat_tokens: u64,
}

impl EventLogAggregate {
    fn from_events(events: &[AttributedTokenEvent]) -> Self {
        let mut run_ids = BTreeSet::new();
        let mut task_ids = BTreeSet::new();
        let mut profile_ids = BTreeSet::new();
        let mut run_profile_totals = BTreeMap::<(String, String), report_json::Totals>::new();
        let mut run_profile_task_ids = BTreeMap::<(String, String), BTreeSet<String>>::new();
        let mut class_totals = BTreeMap::<String, u64>::new();
        let mut totals = report_json::Totals::default();
        let mut repeat_event_count = 0u64;
        let mut repeat_tokens = 0u64;

        for event in events {
            let total_tokens = event.tokens.total().unwrap_or(u64::MAX);
            totals.event_count = totals.event_count.saturating_add(1);
            totals.input_tokens = totals
                .input_tokens
                .saturating_add(event.tokens.input_tokens);
            totals.output_tokens = totals
                .output_tokens
                .saturating_add(event.tokens.output_tokens);
            totals.cache_read_tokens = totals
                .cache_read_tokens
                .saturating_add(event.tokens.cache_read_tokens);
            totals.cache_write_tokens = totals
                .cache_write_tokens
                .saturating_add(event.tokens.cache_write_tokens);
            totals.total_tokens = totals.total_tokens.saturating_add(total_tokens);
            totals.byte_count = totals.byte_count.saturating_add(event.byte_count);

            run_ids.insert(event.run_id.clone());
            task_ids.insert(event.task_id.clone());
            profile_ids.insert(event.profile_id.clone());
            add_to_totals(
                run_profile_totals
                    .entry((event.run_id.clone(), event.profile_id.clone()))
                    .or_default(),
                event,
                total_tokens,
            );
            run_profile_task_ids
                .entry((event.run_id.clone(), event.profile_id.clone()))
                .or_default()
                .insert(event.task_id.clone());

            *class_totals
                .entry(event.operation_class.as_str().to_owned())
                .or_insert(0) += total_tokens;

            if event.repeat_of.is_some() {
                repeat_event_count = repeat_event_count.saturating_add(1);
                repeat_tokens = repeat_tokens.saturating_add(total_tokens);
            }
        }

        totals.run_count = run_ids.len() as u64;
        totals.task_count = task_ids.len() as u64;
        for (key, task_ids) in run_profile_task_ids {
            if let Some(totals) = run_profile_totals.get_mut(&key) {
                totals.task_count = task_ids.len() as u64;
            }
        }
        let profile_label = if profile_ids.len() == 1 {
            profile_ids
                .into_iter()
                .next()
                .unwrap_or_else(|| "unknown".to_owned())
        } else {
            "all".to_owned()
        };

        Self {
            totals,
            run_profile_totals: run_profile_totals
                .into_iter()
                .map(|((run_id, profile_id), totals)| OwnedRunProfileTotals {
                    run_id,
                    profile_id,
                    totals,
                })
                .collect(),
            class_totals: class_totals.into_iter().collect(),
            profile_label,
            repeat_event_count,
            repeat_tokens,
        }
    }

    fn markdown_profile_id(&self) -> &str {
        &self.profile_label
    }

    fn completion_rates(&self) -> report_json::CompletionRates {
        report_json::CompletionRates {
            tasks: report_json::CompletionRate {
                completed: 0,
                failed: 0,
                incomplete: self.totals.task_count,
                rate: 0.0,
            },
            runs: report_json::CompletionRate {
                completed: 0,
                failed: 0,
                incomplete: self.totals.run_count,
                rate: 0.0,
            },
        }
    }

    fn repeat_metrics(&self) -> report_json::RepeatMetrics {
        report_json::RepeatMetrics {
            repeat_event_count: self.repeat_event_count,
            repeat_tokens: self.repeat_tokens,
            repeat_token_share: ratio_or_zero(self.repeat_tokens, self.totals.total_tokens),
        }
    }

    fn cache_metrics(&self) -> report_json::CacheMetrics {
        let cache_tokens = self
            .totals
            .cache_read_tokens
            .saturating_add(self.totals.cache_write_tokens);

        report_json::CacheMetrics {
            cache_read_tokens: self.totals.cache_read_tokens,
            cache_write_tokens: self.totals.cache_write_tokens,
            cache_tokens,
            cache_token_share: ratio_or_zero(cache_tokens, self.totals.total_tokens),
        }
    }
}

fn add_to_totals(
    totals: &mut report_json::Totals,
    event: &AttributedTokenEvent,
    total_tokens: u64,
) {
    totals.event_count = totals.event_count.saturating_add(1);
    totals.run_count = 1;
    totals.task_count = totals.task_count.saturating_add(1);
    totals.input_tokens = totals
        .input_tokens
        .saturating_add(event.tokens.input_tokens);
    totals.output_tokens = totals
        .output_tokens
        .saturating_add(event.tokens.output_tokens);
    totals.cache_read_tokens = totals
        .cache_read_tokens
        .saturating_add(event.tokens.cache_read_tokens);
    totals.cache_write_tokens = totals
        .cache_write_tokens
        .saturating_add(event.tokens.cache_write_tokens);
    totals.total_tokens = totals.total_tokens.saturating_add(total_tokens);
    totals.byte_count = totals.byte_count.saturating_add(event.byte_count);
}

fn completion_rate_to_markdown(
    rate: report_json::CompletionRate,
) -> report_markdown::CompletionRate {
    report_markdown::CompletionRate {
        completed: rate.completed,
        failed: rate.failed,
        incomplete: rate.incomplete,
        rate: rate.rate,
    }
}

fn ratio_or_zero(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

fn passive_self_report_json() -> String {
    let totals = passive_totals();
    let run_profile_totals = [report_json::RunProfileTotals {
        run_id: PASSIVE_RUN_ID,
        profile_id: PASSIVE_PROFILE_ID,
        totals,
    }];
    let class_shares = [
        report_json::ClassShare {
            operation_class: "session.meta",
            tokens: 240,
            share: 0.15,
        },
        report_json::ClassShare {
            operation_class: "vc.status",
            tokens: 960,
            share: 0.60,
        },
        report_json::ClassShare {
            operation_class: "test.output",
            tokens: 400,
            share: 0.25,
        },
    ];
    let warnings = [FIRST_REPORT_WARNING];

    report_json::serialize_report_json(&report_json::ReportJson {
        evidence_grade: report_json::EvidenceGrade::GradeO,
        totals,
        run_profile_totals: &run_profile_totals,
        class_shares: &class_shares,
        completion_rates: report_json::CompletionRates {
            tasks: report_json::CompletionRate::default(),
            runs: report_json::CompletionRate {
                completed: 0,
                failed: 0,
                incomplete: 1,
                rate: 0.0,
            },
        },
        repeat_metrics: report_json::RepeatMetrics::default(),
        cache_metrics: report_json::CacheMetrics {
            cache_read_tokens: totals.cache_read_tokens,
            cache_write_tokens: totals.cache_write_tokens,
            cache_tokens: totals.cache_read_tokens + totals.cache_write_tokens,
            cache_token_share: 0.25,
        },
        calibration: report_json::CalibrationMetadata {
            source: Some("passive-self-report-fixture"),
            tokenizer: Some("estimated"),
            calibration_id: None,
            sample_count: 0,
            mean_absolute_error: None,
        },
        warnings: &warnings,
    })
}

fn passive_self_report_markdown() -> String {
    let totals = passive_totals();
    let warnings = [FIRST_REPORT_WARNING];

    report_markdown::serialize_report_markdown(&report_markdown::MarkdownReport {
        title: "vc-tokmeter first local report",
        evidence_grade: report_markdown::EvidenceGrade::GradeO,
        single_profile: Some(report_markdown::ProfileSummary {
            profile_id: PASSIVE_PROFILE_ID,
            event_count: totals.event_count,
            run_count: totals.run_count,
            task_count: totals.task_count,
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            cache_read_tokens: totals.cache_read_tokens,
            cache_write_tokens: totals.cache_write_tokens,
            total_tokens: totals.total_tokens,
            byte_count: totals.byte_count,
            completion_rates: report_markdown::CompletionRates {
                tasks: report_markdown::CompletionRate::default(),
                runs: report_markdown::CompletionRate {
                    completed: 0,
                    failed: 0,
                    incomplete: 1,
                    rate: 0.0,
                },
            },
            token_distribution: None,
        }),
        comparison: None,
        warnings: &warnings,
    })
}

fn passive_totals() -> report_json::Totals {
    report_json::Totals {
        event_count: 3,
        run_count: 1,
        task_count: 0,
        input_tokens: 1_120,
        output_tokens: 320,
        cache_read_tokens: 320,
        cache_write_tokens: 80,
        total_tokens: 1_600,
        byte_count: 4_096,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CaptureMode, OperationClass, TokenCounts};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn renders_report_output_paths() {
        let paths = report_output_paths(Path::new("/tmp/tokmeter-out"));
        let rendered = render_report_output_paths(&paths);

        assert!(rendered.contains("report.json: /tmp/tokmeter-out/report.json"));
        assert!(rendered.contains("report.md: /tmp/tokmeter-out/report.md"));
    }

    #[test]
    fn creates_grade_o_artifacts_without_savings_claims() {
        let output_dir = temp_output_dir("grade-o-artifacts");
        let artifacts = create_first_report_artifacts(&output_dir, Option::<&Path>::None).unwrap();

        assert_eq!(artifacts.source, CliReportSource::PassiveSelfReportFixture);
        assert!(artifacts.paths.report_json.exists());
        assert!(artifacts.paths.report_markdown.exists());

        let json = fs::read_to_string(&artifacts.paths.report_json).unwrap();
        let markdown = fs::read_to_string(&artifacts.paths.report_markdown).unwrap();

        assert!(json.contains("\"evidence_grade\": \"Grade O\""));
        assert!(markdown.contains("**Evidence:** Grade O"));
        assert_no_savings_claim(&json);
        assert_no_savings_claim(&markdown);
    }

    #[test]
    fn creates_artifacts_quickly_when_event_log_is_missing() {
        let output_dir = temp_output_dir("missing-log");
        let missing_log = output_dir.join("events.jsonl");
        let started = std::time::Instant::now();

        let artifacts = create_first_report_artifacts(&output_dir, Some(&missing_log)).unwrap();

        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(
            artifacts.paths.report_json,
            output_dir.join(REPORT_JSON_FILE_NAME)
        );
        assert_eq!(
            artifacts.paths.report_markdown,
            output_dir.join(REPORT_MARKDOWN_FILE_NAME)
        );
    }

    #[test]
    fn empty_event_log_uses_first_report_fixture() {
        let output_dir = temp_output_dir("empty-log");
        fs::create_dir_all(&output_dir).unwrap();
        let empty_log = output_dir.join("events.jsonl");
        fs::write(&empty_log, "").unwrap();

        let artifacts = create_first_report_artifacts(&output_dir, Some(&empty_log)).unwrap();

        let markdown = fs::read_to_string(&artifacts.paths.report_markdown).unwrap();
        assert!(markdown.contains("Passive self-report fixture"));
    }

    #[test]
    fn non_empty_event_log_is_aggregated_into_grade_o_artifacts() {
        let output_dir = temp_output_dir("existing-log");
        fs::create_dir_all(&output_dir).unwrap();
        let event_log = output_dir.join("events.jsonl");
        write_event_log(
            &event_log,
            &[
                event(
                    "run-1",
                    "task-1",
                    "baseline",
                    OperationClass::FileRead,
                    TokenCounts::new(100, 20, 10, 5),
                    512,
                    None,
                ),
                event(
                    "run-1",
                    "task-1",
                    "baseline",
                    OperationClass::VcStatus,
                    TokenCounts::new(50, 10, 0, 0),
                    128,
                    Some("digest-a"),
                ),
            ],
        );

        let artifacts = create_first_report_artifacts(&output_dir, Some(&event_log)).unwrap();

        assert_eq!(artifacts.source, CliReportSource::EventLogAggregation);
        let json = fs::read_to_string(&artifacts.paths.report_json).unwrap();
        let markdown = fs::read_to_string(&artifacts.paths.report_markdown).unwrap();

        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"evidence_grade\": \"Grade O\""));
        assert!(json.contains("\"event_count\": 2"));
        assert!(json.contains("\"total_tokens\": 195"));
        assert!(json.contains("\"operation_class\": \"file.read\""));
        assert!(json.contains("\"repeat_event_count\": 1"));
        assert!(markdown.contains("# vc-tokmeter local event report"));
        assert!(markdown.contains("## Summary"));
        assert!(markdown.contains("**Evidence:** Grade O"));
        assert_no_savings_claim(&json);
        assert_no_savings_claim(&markdown);
    }

    #[test]
    fn share_artifact_redacts_report_paths_repo_names_and_raw_text() {
        let output_dir = temp_output_dir("private-metric-collector");
        let artifacts = create_first_report_artifacts(&output_dir, Option::<&Path>::None).unwrap();
        let raw_markers = [
            "/Users/justin/private/metric-collector/src/main.rs",
            "metric-collector",
            "please inspect the secret billing prompt",
            "fn private_source_marker() {}",
            "tool output contained customer secret",
        ];
        fs::write(
            &artifacts.paths.report_json,
            format!(
                "{{\"schema_version\":1,\"repository\":\"{}\",\"path\":\"{}\",\"prompt\":\"{}\"}}",
                raw_markers[1], raw_markers[0], raw_markers[2]
            ),
        )
        .unwrap();
        fs::write(
            &artifacts.paths.report_markdown,
            format!(
                "# private\n\nsource: {}\n\ntool_output: {}\n",
                raw_markers[3], raw_markers[4]
            ),
        )
        .unwrap();

        let share_path = create_report_share_artifact(&artifacts, "share-test-salt").unwrap();
        let share = fs::read_to_string(share_path).unwrap();

        for marker in raw_markers {
            assert!(
                !share.contains(marker),
                "share output leaked raw marker {:?}: {}",
                marker,
                share
            );
        }
        assert!(share.contains("\"schema_version\":1"));
        assert!(share.contains("path_hash"));
        assert!(share.contains("content_byte_count"));
        assert!(share.contains("content_value_count"));
    }

    fn assert_no_savings_claim(value: &str) {
        let lower = value.to_ascii_lowercase();
        assert!(!lower.contains("savings"));
        assert!(!lower.contains("saved"));
    }

    fn temp_output_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "vc-tokmeter-cli-report-{}-{}-{}",
            name,
            std::process::id(),
            nanos
        ))
    }

    fn write_event_log(path: &Path, events: &[AttributedTokenEvent]) {
        let mut bytes = Vec::new();
        EventLog {
            events: events.to_vec(),
        }
        .write_to(&mut bytes)
        .unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn event(
        run_id: &str,
        task_id: &str,
        profile_id: &str,
        operation_class: OperationClass,
        tokens: TokenCounts,
        byte_count: u64,
        repeat_of: Option<&str>,
    ) -> AttributedTokenEvent {
        AttributedTokenEvent {
            timestamp_ms: 1_725_000_000_000,
            mode: CaptureMode::Task,
            run_id: run_id.to_owned(),
            task_id: task_id.to_owned(),
            profile_id: profile_id.to_owned(),
            adapter: "test-adapter".to_owned(),
            operation_class,
            tool: "test-tool".to_owned(),
            tokens,
            byte_count,
            content_digest: "digest-a".to_owned(),
            repeat_of: repeat_of.map(str::to_owned),
        }
    }
}
