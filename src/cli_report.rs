use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::report_json;
use crate::report_markdown;

pub const REPORT_JSON_FILE_NAME: &str = "report.json";
pub const REPORT_MARKDOWN_FILE_NAME: &str = "report.md";

const PASSIVE_PROFILE_ID: &str = "passive";
const PASSIVE_RUN_ID: &str = "first-local-report";
const FIRST_REPORT_WARNING: &str =
    "Passive self-report fixture: no event log was found; metrics are observational.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliReportPaths {
    pub output_dir: PathBuf,
    pub report_json: PathBuf,
    pub report_markdown: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliReportSource {
    PassiveSelfReportFixture,
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
    let should_use_fixture = match event_log {
        Some(path) => event_log_is_missing_or_empty(path.as_ref())?,
        None => true,
    };

    if !should_use_fixture {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "event log exists; use the full report aggregation path",
        ));
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

fn event_log_is_missing_or_empty(path: &Path) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() == 0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
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
    fn non_empty_event_log_is_left_for_full_report_path() {
        let output_dir = temp_output_dir("existing-log");
        fs::create_dir_all(&output_dir).unwrap();
        let event_log = output_dir.join("events.jsonl");
        fs::write(&event_log, "{\"schema_version\":2}\n").unwrap();

        let error = create_first_report_artifacts(&output_dir, Some(&event_log)).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!output_dir.join(REPORT_JSON_FILE_NAME).exists());
        assert!(!output_dir.join(REPORT_MARKDOWN_FILE_NAME).exists());
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
}
