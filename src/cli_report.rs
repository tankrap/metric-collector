use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cli_run::{CompletedRunRecord, read_completed_run_records};
use crate::completion::CompletionStatus as RunCompletionStatus;
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
const COMPARE_REPORT_WARNING: &str = "Mode T compare report: token deltas use completed task runs only; review completion rates before interpreting the treatment.";
const BASELINE_PROFILE_ID: &str = "baseline";
const TREATMENT_PROFILE_ID: &str = "treatment";

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
    CompareAggregation,
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

pub fn create_compare_report_artifacts(
    output_dir: impl AsRef<Path>,
    event_log: impl AsRef<Path>,
    completed_runs: impl AsRef<Path>,
) -> io::Result<CliReportArtifacts> {
    let log = EventLog::read_file(event_log.as_ref()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read event log: {error}"),
        )
    })?;
    let completed_runs = read_completed_run_records(completed_runs.as_ref()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read completed runs: {error}"),
        )
    })?;
    let compare =
        CompareReportAggregate::from_events_and_completed_runs(&log.events, &completed_runs)?;

    let paths = report_output_paths(output_dir);
    fs::create_dir_all(&paths.output_dir)?;
    fs::write(&paths.report_json, compare_report_json(&compare))?;
    fs::write(&paths.report_markdown, compare_report_markdown(&compare))?;

    Ok(CliReportArtifacts {
        paths,
        source: CliReportSource::CompareAggregation,
    })
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
        comparison: None,
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

fn compare_report_json(compare: &CompareReportAggregate) -> String {
    let run_profile_totals = compare
        .run_profile_totals
        .iter()
        .map(|item| report_json::RunProfileTotals {
            run_id: item.run_id.as_str(),
            profile_id: item.profile_id.as_str(),
            totals: item.totals,
        })
        .collect::<Vec<_>>();
    let class_shares = compare
        .class_totals
        .iter()
        .map(|(operation_class, tokens)| report_json::ClassShare {
            operation_class: operation_class.as_str(),
            tokens: *tokens,
            share: ratio_or_zero(*tokens, compare.totals.total_tokens),
        })
        .collect::<Vec<_>>();
    let rows = compare.comparison_rows();
    let warnings = [COMPARE_REPORT_WARNING];

    report_json::serialize_report_json(&report_json::ReportJson {
        evidence_grade: report_json::EvidenceGrade::GradeP,
        totals: compare.totals,
        run_profile_totals: &run_profile_totals,
        class_shares: &class_shares,
        completion_rates: compare.completion_rates(),
        comparison: Some(report_json::CompareSummary {
            baseline_profile_id: BASELINE_PROFILE_ID,
            treatment_profile_id: TREATMENT_PROFILE_ID,
            rows: &rows,
            completion_rates: compare.completion_comparison(),
        }),
        repeat_metrics: compare.repeat_metrics(),
        cache_metrics: compare.cache_metrics(),
        calibration: report_json::CalibrationMetadata {
            source: Some("mode-t-event-log"),
            tokenizer: Some("event-log"),
            calibration_id: None,
            sample_count: 0,
            mean_absolute_error: None,
        },
        warnings: &warnings,
    })
}

fn compare_report_markdown(compare: &CompareReportAggregate) -> String {
    let json_rows = compare.comparison_rows();
    let rows = json_rows
        .iter()
        .map(|row| report_markdown::CompareRow {
            metric: row.metric,
            baseline: report_markdown::MetricValue {
                value: row.baseline.value,
                median: row.baseline.median,
                iqr: row.baseline.iqr,
            },
            treatment: report_markdown::MetricValue {
                value: row.treatment.value,
                median: row.treatment.median,
                iqr: row.treatment.iqr,
            },
        })
        .collect::<Vec<_>>();
    let completion = compare.completion_comparison();
    let warnings = [COMPARE_REPORT_WARNING];

    report_markdown::serialize_report_markdown(&report_markdown::MarkdownReport {
        title: "vc-tokmeter Mode T compare report",
        evidence_grade: report_markdown::EvidenceGrade::GradeP,
        single_profile: None,
        comparison: Some(report_markdown::CompareSummary {
            baseline_profile_id: BASELINE_PROFILE_ID,
            treatment_profile_id: TREATMENT_PROFILE_ID,
            rows: &rows,
            completion_rates: report_markdown::CompletionRateComparison {
                tasks: report_markdown::CompletionRatePair {
                    baseline: completion_rate_to_markdown(completion.tasks.baseline),
                    treatment: completion_rate_to_markdown(completion.tasks.treatment),
                },
                runs: report_markdown::CompletionRatePair {
                    baseline: completion_rate_to_markdown(completion.runs.baseline),
                    treatment: completion_rate_to_markdown(completion.runs.treatment),
                },
            },
        }),
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
                CliReportSource::CompareAggregation => "compare-aggregation",
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

#[derive(Clone, Debug, Default, PartialEq)]
struct CompareReportAggregate {
    totals: report_json::Totals,
    run_profile_totals: Vec<OwnedRunProfileTotals>,
    class_totals: Vec<(String, u64)>,
    completion: CompareCompletionAggregate,
    completed_task_tokens: BTreeMap<String, Vec<u64>>,
    repeat_event_count: u64,
    repeat_tokens: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CompareCompletionAggregate {
    task_counts: BTreeMap<String, report_json::CompletionRate>,
    run_counts: BTreeMap<String, report_json::CompletionRate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MetricMode {
    Total,
    Average,
}

impl CompareReportAggregate {
    fn from_events_and_completed_runs(
        events: &[AttributedTokenEvent],
        completed_runs: &[CompletedRunRecord],
    ) -> io::Result<Self> {
        let completion = CompareCompletionAggregate::from_completed_runs(completed_runs);
        if !completion.has_profile(BASELINE_PROFILE_ID)
            || !completion.has_profile(TREATMENT_PROFILE_ID)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Grade P compare reports require baseline and treatment completed-run records",
            ));
        }

        let mut run_ids = BTreeSet::new();
        let mut task_ids = BTreeSet::new();
        let mut run_profile_totals = BTreeMap::<(String, String), report_json::Totals>::new();
        let mut run_profile_task_ids = BTreeMap::<(String, String), BTreeSet<String>>::new();
        let mut class_totals = BTreeMap::<String, u64>::new();
        let mut totals = report_json::Totals::default();
        let mut repeat_event_count = 0u64;
        let mut repeat_tokens = 0u64;
        let completed_keys = completed_runs
            .iter()
            .filter(|record| record.status == RunCompletionStatus::Completed)
            .map(|record| {
                (
                    record.run_id.as_str(),
                    record.task_id.as_str(),
                    record.profile_id.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        let mut completed_token_totals = BTreeMap::<(String, String, String), u64>::new();

        for event in events {
            if !is_compare_profile(&event.profile_id) {
                continue;
            }
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

            if completed_keys.contains(&(
                event.run_id.as_str(),
                event.task_id.as_str(),
                event.profile_id.as_str(),
            )) {
                *completed_token_totals
                    .entry((
                        event.run_id.clone(),
                        event.task_id.clone(),
                        event.profile_id.clone(),
                    ))
                    .or_insert(0) += total_tokens;
            }
        }

        totals.run_count = run_ids.len() as u64;
        totals.task_count = task_ids.len() as u64;
        for (key, task_ids) in run_profile_task_ids {
            if let Some(totals) = run_profile_totals.get_mut(&key) {
                totals.task_count = task_ids.len() as u64;
            }
        }

        let mut completed_task_tokens = BTreeMap::<String, Vec<u64>>::new();
        for record in completed_runs
            .iter()
            .filter(|record| record.status == RunCompletionStatus::Completed)
            .filter(|record| is_compare_profile(&record.profile_id))
        {
            let tokens = completed_token_totals
                .get(&(
                    record.run_id.clone(),
                    record.task_id.clone(),
                    record.profile_id.clone(),
                ))
                .copied()
                .unwrap_or(0);
            completed_task_tokens
                .entry(record.profile_id.clone())
                .or_default()
                .push(tokens);
        }

        Ok(Self {
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
            completion,
            completed_task_tokens,
            repeat_event_count,
            repeat_tokens,
        })
    }

    fn comparison_rows(&self) -> Vec<report_json::CompareRow<'static>> {
        vec![
            report_json::CompareRow {
                metric: "Completed task tokens",
                baseline: self.completed_task_metric(BASELINE_PROFILE_ID, MetricMode::Total),
                treatment: self.completed_task_metric(TREATMENT_PROFILE_ID, MetricMode::Total),
            },
            report_json::CompareRow {
                metric: "Tokens per completed task",
                baseline: self.completed_task_metric(BASELINE_PROFILE_ID, MetricMode::Average),
                treatment: self.completed_task_metric(TREATMENT_PROFILE_ID, MetricMode::Average),
            },
        ]
    }

    fn completed_task_metric(
        &self,
        profile_id: &str,
        mode: MetricMode,
    ) -> report_json::MetricValue {
        let tokens = self
            .completed_task_tokens
            .get(profile_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let total = tokens.iter().copied().sum::<u64>() as f64;
        let value = match mode {
            MetricMode::Total => total,
            MetricMode::Average => {
                if tokens.is_empty() {
                    0.0
                } else {
                    total / tokens.len() as f64
                }
            }
        };
        let samples = tokens.iter().map(|value| *value as f64).collect::<Vec<_>>();
        report_json::MetricValue {
            value,
            median: median(&samples),
            iqr: iqr(&samples),
        }
    }

    fn completion_rates(&self) -> report_json::CompletionRates {
        report_json::CompletionRates {
            tasks: combine_rates(self.completion.task_counts.values().copied()),
            runs: combine_rates(self.completion.run_counts.values().copied()),
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

    fn completion_comparison(&self) -> report_json::CompletionRateComparison {
        report_json::CompletionRateComparison {
            tasks: report_json::CompletionRatePair {
                baseline: self.completion.rate_for_task_profile(BASELINE_PROFILE_ID),
                treatment: self.completion.rate_for_task_profile(TREATMENT_PROFILE_ID),
            },
            runs: report_json::CompletionRatePair {
                baseline: self.completion.rate_for_run_profile(BASELINE_PROFILE_ID),
                treatment: self.completion.rate_for_run_profile(TREATMENT_PROFILE_ID),
            },
        }
    }
}

impl CompareCompletionAggregate {
    fn from_completed_runs(records: &[CompletedRunRecord]) -> Self {
        let mut task_counts = BTreeMap::<String, report_json::CompletionRate>::new();
        let mut run_statuses = BTreeMap::<(String, String), RunCompletionStatus>::new();

        for record in records
            .iter()
            .filter(|record| is_compare_profile(&record.profile_id))
        {
            add_run_completion_count(
                task_counts.entry(record.profile_id.clone()).or_default(),
                record.status,
            );
            run_statuses
                .entry((record.profile_id.clone(), record.run_id.clone()))
                .and_modify(|status| *status = merge_run_completion_status(*status, record.status))
                .or_insert(record.status);
        }

        for rate in task_counts.values_mut() {
            rate.rate = completion_ratio(*rate);
        }

        let mut run_counts = BTreeMap::<String, report_json::CompletionRate>::new();
        for ((profile_id, _run_id), status) in run_statuses {
            add_run_completion_count(run_counts.entry(profile_id).or_default(), status);
        }
        for rate in run_counts.values_mut() {
            rate.rate = completion_ratio(*rate);
        }

        Self {
            task_counts,
            run_counts,
        }
    }

    fn has_profile(&self, profile_id: &str) -> bool {
        self.task_counts
            .get(profile_id)
            .is_some_and(|rate| rate.total() > 0)
    }

    fn rate_for_task_profile(&self, profile_id: &str) -> report_json::CompletionRate {
        self.task_counts
            .get(profile_id)
            .copied()
            .unwrap_or_default()
    }

    fn rate_for_run_profile(&self, profile_id: &str) -> report_json::CompletionRate {
        self.run_counts.get(profile_id).copied().unwrap_or_default()
    }
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

fn is_compare_profile(profile_id: &str) -> bool {
    profile_id == BASELINE_PROFILE_ID || profile_id == TREATMENT_PROFILE_ID
}

fn add_run_completion_count(counts: &mut report_json::CompletionRate, status: RunCompletionStatus) {
    match status {
        RunCompletionStatus::Completed => counts.completed = counts.completed.saturating_add(1),
        RunCompletionStatus::Failed => counts.failed = counts.failed.saturating_add(1),
        RunCompletionStatus::Aborted => counts.incomplete = counts.incomplete.saturating_add(1),
    }
    counts.rate = completion_ratio(*counts);
}

fn merge_run_completion_status(
    current: RunCompletionStatus,
    next: RunCompletionStatus,
) -> RunCompletionStatus {
    match (current, next) {
        (RunCompletionStatus::Failed, _) | (_, RunCompletionStatus::Failed) => {
            RunCompletionStatus::Failed
        }
        (RunCompletionStatus::Aborted, _) | (_, RunCompletionStatus::Aborted) => {
            RunCompletionStatus::Aborted
        }
        (RunCompletionStatus::Completed, RunCompletionStatus::Completed) => {
            RunCompletionStatus::Completed
        }
    }
}

fn combine_rates(
    rates: impl IntoIterator<Item = report_json::CompletionRate>,
) -> report_json::CompletionRate {
    let mut combined = report_json::CompletionRate::default();
    for rate in rates {
        combined.completed = combined.completed.saturating_add(rate.completed);
        combined.failed = combined.failed.saturating_add(rate.failed);
        combined.incomplete = combined.incomplete.saturating_add(rate.incomplete);
    }
    combined.rate = completion_ratio(combined);
    combined
}

fn completion_ratio(rate: report_json::CompletionRate) -> f64 {
    if rate.total() == 0 {
        0.0
    } else {
        rate.completed as f64 / rate.total() as f64
    }
}

fn median(values: &[f64]) -> Option<f64> {
    let values = sorted_finite_values(values);
    if values.is_empty() {
        None
    } else {
        Some(median_of_sorted_slice(&values))
    }
}

fn iqr(values: &[f64]) -> Option<f64> {
    let values = sorted_finite_values(values);
    if values.len() < 2 {
        return None;
    }
    let midpoint = values.len() / 2;
    let lower = &values[..midpoint];
    let upper = if values.len() % 2 == 0 {
        &values[midpoint..]
    } else {
        &values[midpoint + 1..]
    };
    if lower.is_empty() || upper.is_empty() {
        None
    } else {
        Some(median_of_sorted_slice(upper) - median_of_sorted_slice(lower))
    }
}

fn sorted_finite_values(values: &[f64]) -> Vec<f64> {
    let mut values = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.total_cmp(right));
    values
}

fn median_of_sorted_slice(values: &[f64]) -> f64 {
    let midpoint = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[midpoint - 1] + values[midpoint]) / 2.0
    } else {
        values[midpoint]
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
        comparison: None,
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
    use crate::cli_run::{CompletedRunRecord, write_completed_run_records};
    use crate::completion::CompletionStatus;
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
    fn creates_grade_p_compare_artifacts_when_baseline_and_treatment_exist() {
        let output_dir = temp_output_dir("compare-report");
        fs::create_dir_all(&output_dir).unwrap();
        let event_log = output_dir.join("events.jsonl");
        let completed_runs = output_dir.join("completed-runs.tsv");
        write_event_log(
            &event_log,
            &[
                event(
                    "baseline-ok",
                    "task-1",
                    "baseline",
                    OperationClass::VcStatus,
                    TokenCounts::new(100, 0, 0, 0),
                    64,
                    None,
                ),
                event(
                    "baseline-fail",
                    "task-2",
                    "baseline",
                    OperationClass::VcDiff,
                    TokenCounts::new(900, 0, 0, 0),
                    128,
                    None,
                ),
                event(
                    "treatment-ok",
                    "task-1",
                    "treatment",
                    OperationClass::VcStatus,
                    TokenCounts::new(70, 0, 0, 0),
                    64,
                    None,
                ),
                event(
                    "treatment-abort",
                    "task-2",
                    "treatment",
                    OperationClass::VcDiff,
                    TokenCounts::new(100, 0, 0, 0),
                    128,
                    None,
                ),
            ],
        );
        write_completed_run_records(
            &completed_runs,
            &[
                completed_record(
                    "baseline-ok",
                    "task-1",
                    "baseline",
                    CompletionStatus::Completed,
                ),
                completed_record(
                    "baseline-fail",
                    "task-2",
                    "baseline",
                    CompletionStatus::Failed,
                ),
                completed_record(
                    "treatment-ok",
                    "task-1",
                    "treatment",
                    CompletionStatus::Completed,
                ),
                completed_record(
                    "treatment-abort",
                    "task-2",
                    "treatment",
                    CompletionStatus::Aborted,
                ),
            ],
        )
        .unwrap();

        let artifacts =
            create_compare_report_artifacts(&output_dir, &event_log, &completed_runs).unwrap();

        assert_eq!(artifacts.source, CliReportSource::CompareAggregation);
        let json = fs::read_to_string(&artifacts.paths.report_json).unwrap();
        let markdown = fs::read_to_string(&artifacts.paths.report_markdown).unwrap();
        assert!(json.contains("\"evidence_grade\": \"Grade P\""));
        assert!(json.contains("\"comparison\": {"));
        assert!(json.contains("\"baseline_profile_id\": \"baseline\""));
        assert!(json.contains("\"treatment_profile_id\": \"treatment\""));
        assert!(json.contains("\"metric\": \"Tokens per completed task\""));
        assert!(markdown.contains("# vc-tokmeter Mode T compare report"));
        assert!(markdown.contains("**Evidence:** Grade P"));
        assert!(markdown.contains("Baseline: **baseline**"));
        assert!(markdown.contains("Treatment: **treatment**"));
    }

    #[test]
    fn compare_report_uses_completed_runs_only_for_token_math() {
        let events = [
            event(
                "baseline-ok",
                "task-1",
                "baseline",
                OperationClass::VcStatus,
                TokenCounts::new(100, 0, 0, 0),
                64,
                None,
            ),
            event(
                "baseline-fail",
                "task-2",
                "baseline",
                OperationClass::VcDiff,
                TokenCounts::new(900, 0, 0, 0),
                128,
                None,
            ),
            event(
                "treatment-ok",
                "task-1",
                "treatment",
                OperationClass::VcStatus,
                TokenCounts::new(70, 0, 0, 0),
                64,
                None,
            ),
        ];
        let completed_runs = [
            completed_record(
                "baseline-ok",
                "task-1",
                "baseline",
                CompletionStatus::Completed,
            ),
            completed_record(
                "baseline-fail",
                "task-2",
                "baseline",
                CompletionStatus::Failed,
            ),
            completed_record(
                "treatment-ok",
                "task-1",
                "treatment",
                CompletionStatus::Completed,
            ),
        ];

        let aggregate =
            CompareReportAggregate::from_events_and_completed_runs(&events, &completed_runs)
                .unwrap();
        let rows = aggregate.comparison_rows();

        assert_eq!(rows[0].metric, "Completed task tokens");
        assert_eq!(rows[0].baseline.value, 100.0);
        assert_eq!(rows[0].treatment.value, 70.0);
        assert_eq!(rows[1].metric, "Tokens per completed task");
        assert_eq!(rows[1].baseline.value, 100.0);
        assert_eq!(rows[1].treatment.value, 70.0);
        assert_eq!(aggregate.totals.total_tokens, 1_070);
    }

    #[test]
    fn compare_report_shows_side_by_side_completion_rates() {
        let output_dir = temp_output_dir("compare-completion-rates");
        fs::create_dir_all(&output_dir).unwrap();
        let event_log = output_dir.join("events.jsonl");
        let completed_runs = output_dir.join("completed-runs.tsv");
        write_event_log(
            &event_log,
            &[
                event(
                    "baseline-ok",
                    "task-1",
                    "baseline",
                    OperationClass::VcStatus,
                    TokenCounts::new(100, 0, 0, 0),
                    64,
                    None,
                ),
                event(
                    "treatment-ok",
                    "task-1",
                    "treatment",
                    OperationClass::VcStatus,
                    TokenCounts::new(70, 0, 0, 0),
                    64,
                    None,
                ),
            ],
        );
        write_completed_run_records(
            &completed_runs,
            &[
                completed_record(
                    "baseline-ok",
                    "task-1",
                    "baseline",
                    CompletionStatus::Completed,
                ),
                completed_record(
                    "baseline-fail",
                    "task-2",
                    "baseline",
                    CompletionStatus::Failed,
                ),
                completed_record(
                    "treatment-ok",
                    "task-1",
                    "treatment",
                    CompletionStatus::Completed,
                ),
                completed_record(
                    "treatment-abort",
                    "task-2",
                    "treatment",
                    CompletionStatus::Aborted,
                ),
            ],
        )
        .unwrap();

        let artifacts =
            create_compare_report_artifacts(&output_dir, &event_log, &completed_runs).unwrap();
        let markdown = fs::read_to_string(&artifacts.paths.report_markdown).unwrap();

        assert!(markdown.contains("## Completion rate changes"));
        assert!(markdown.contains("| Tasks | 50.0% (1/2) | 50.0% (1/2) | 0.0 pp |"));
        assert!(markdown.contains("| Runs | 50.0% (1/2) | 50.0% (1/2) | 0.0 pp |"));
    }

    #[test]
    fn compare_report_does_not_use_observational_delta_language() {
        let events = [
            event(
                "baseline-ok",
                "task-1",
                "baseline",
                OperationClass::VcStatus,
                TokenCounts::new(100, 0, 0, 0),
                64,
                None,
            ),
            event(
                "treatment-ok",
                "task-1",
                "treatment",
                OperationClass::VcStatus,
                TokenCounts::new(70, 0, 0, 0),
                64,
                None,
            ),
        ];
        let completed_runs = [
            completed_record(
                "baseline-ok",
                "task-1",
                "baseline",
                CompletionStatus::Completed,
            ),
            completed_record(
                "treatment-ok",
                "task-1",
                "treatment",
                CompletionStatus::Completed,
            ),
        ];

        let aggregate =
            CompareReportAggregate::from_events_and_completed_runs(&events, &completed_runs)
                .unwrap();
        let markdown = compare_report_markdown(&aggregate);

        assert!(
            markdown.contains("| Tokens per completed task | Grade P | 100 | 70 | 30 (30.0%) |")
        );
        assert!(!markdown.contains("descriptive delta only"));
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

    fn completed_record(
        run_id: &str,
        task_id: &str,
        profile_id: &str,
        status: CompletionStatus,
    ) -> CompletedRunRecord {
        CompletedRunRecord::new(
            run_id,
            task_id,
            profile_id,
            1,
            status,
            "test-adapter",
            1_725_000_000_000,
            Some(1_725_000_001_000),
            1_725_000_001,
        )
        .unwrap()
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
