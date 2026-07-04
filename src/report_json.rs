pub const REPORT_JSON_SCHEMA_VERSION: u32 = 1;
pub const GRADE_O_CAPTION: &str = "Observational: workloads were not controlled; differences may reflect changes in the work itself.";
pub const GRADE_P_CAPTION: &str =
    "Protocol: completed Mode T task runs with controlled task/profile pairing.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceGrade {
    GradeO,
    GradeP,
}

impl EvidenceGrade {
    pub const fn label(self) -> &'static str {
        match self {
            Self::GradeO => "Grade O",
            Self::GradeP => "Grade P",
        }
    }

    pub const fn caption(self) -> &'static str {
        match self {
            Self::GradeO => GRADE_O_CAPTION,
            Self::GradeP => GRADE_P_CAPTION,
        }
    }
}

impl Default for EvidenceGrade {
    fn default() -> Self {
        Self::GradeO
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub event_count: u64,
    pub run_count: u64,
    pub task_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub byte_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunProfileTotals<'a> {
    pub run_id: &'a str,
    pub profile_id: &'a str,
    pub totals: Totals,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassShare<'a> {
    pub operation_class: &'a str,
    pub tokens: u64,
    pub share: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompletionRates {
    pub tasks: CompletionRate,
    pub runs: CompletionRate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompletionRate {
    pub completed: u64,
    pub failed: u64,
    pub incomplete: u64,
    pub rate: f64,
}

impl CompletionRate {
    pub const fn total(self) -> u64 {
        self.completed + self.failed + self.incomplete
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RepeatMetrics {
    pub repeat_event_count: u64,
    pub repeat_tokens: u64,
    pub repeat_token_share: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheMetrics {
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_tokens: u64,
    pub cache_token_share: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalibrationMetadata<'a> {
    pub source: Option<&'a str>,
    pub tokenizer: Option<&'a str>,
    pub calibration_id: Option<&'a str>,
    pub sample_count: u64,
    pub mean_absolute_error: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReportJson<'a> {
    pub evidence_grade: EvidenceGrade,
    pub totals: Totals,
    pub run_profile_totals: &'a [RunProfileTotals<'a>],
    pub class_shares: &'a [ClassShare<'a>],
    pub completion_rates: CompletionRates,
    pub repeat_metrics: RepeatMetrics,
    pub cache_metrics: CacheMetrics,
    pub calibration: CalibrationMetadata<'a>,
    pub warnings: &'a [&'a str],
}

pub fn serialize_report_json(report: &ReportJson<'_>) -> String {
    report.to_json()
}

impl ReportJson<'_> {
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        write_u32_field(
            &mut out,
            1,
            "schema_version",
            REPORT_JSON_SCHEMA_VERSION,
            true,
        );
        write_str_field(
            &mut out,
            1,
            "evidence_grade",
            self.evidence_grade.label(),
            true,
        );
        write_str_field(
            &mut out,
            1,
            "evidence_caption",
            self.evidence_grade.caption(),
            true,
        );

        write_field_name(&mut out, 1, "totals");
        write_totals(&mut out, 1, &self.totals);
        out.push_str(",\n");

        write_run_profile_totals(&mut out, self.run_profile_totals);
        out.push_str(",\n");

        write_class_shares(&mut out, self.class_shares);
        out.push_str(",\n");

        write_completion_rates(&mut out, &self.completion_rates);
        out.push_str(",\n");

        write_repeat_metrics(&mut out, &self.repeat_metrics);
        out.push_str(",\n");

        write_cache_metrics(&mut out, &self.cache_metrics);
        out.push_str(",\n");

        write_calibration(&mut out, &self.calibration);
        out.push_str(",\n");

        write_warnings(&mut out, self.warnings);
        out.push('\n');
        out.push_str("}\n");
        out
    }
}

fn write_totals(out: &mut String, indent: usize, totals: &Totals) {
    out.push_str("{\n");
    write_u64_field(out, indent + 1, "event_count", totals.event_count, true);
    write_u64_field(out, indent + 1, "run_count", totals.run_count, true);
    write_u64_field(out, indent + 1, "task_count", totals.task_count, true);
    write_u64_field(out, indent + 1, "input_tokens", totals.input_tokens, true);
    write_u64_field(out, indent + 1, "output_tokens", totals.output_tokens, true);
    write_u64_field(
        out,
        indent + 1,
        "cache_read_tokens",
        totals.cache_read_tokens,
        true,
    );
    write_u64_field(
        out,
        indent + 1,
        "cache_write_tokens",
        totals.cache_write_tokens,
        true,
    );
    write_u64_field(out, indent + 1, "total_tokens", totals.total_tokens, true);
    write_u64_field(out, indent + 1, "byte_count", totals.byte_count, false);
    write_indent(out, indent);
    out.push('}');
}

fn write_run_profile_totals(out: &mut String, totals: &[RunProfileTotals<'_>]) {
    write_field_name(out, 1, "run_profile_totals");
    out.push_str("[\n");
    for (index, item) in totals.iter().enumerate() {
        write_indent(out, 2);
        out.push_str("{\n");
        write_str_field(out, 3, "run_id", item.run_id, true);
        write_str_field(out, 3, "profile_id", item.profile_id, true);
        write_field_name(out, 3, "totals");
        write_totals(out, 3, &item.totals);
        out.push('\n');
        write_indent(out, 2);
        out.push('}');
        if index + 1 != totals.len() {
            out.push(',');
        }
        out.push('\n');
    }
    write_indent(out, 1);
    out.push(']');
}

fn write_class_shares(out: &mut String, shares: &[ClassShare<'_>]) {
    write_field_name(out, 1, "class_shares");
    out.push_str("[\n");
    for (index, share) in shares.iter().enumerate() {
        write_indent(out, 2);
        out.push_str("{\n");
        write_str_field(out, 3, "operation_class", share.operation_class, true);
        write_u64_field(out, 3, "tokens", share.tokens, true);
        write_f64_field(out, 3, "share", share.share, false);
        write_indent(out, 2);
        out.push('}');
        if index + 1 != shares.len() {
            out.push(',');
        }
        out.push('\n');
    }
    write_indent(out, 1);
    out.push(']');
}

fn write_completion_rates(out: &mut String, completion_rates: &CompletionRates) {
    write_field_name(out, 1, "completion_rates");
    out.push_str("{\n");
    write_completion_rate(out, 2, "tasks", &completion_rates.tasks, true);
    write_completion_rate(out, 2, "runs", &completion_rates.runs, false);
    write_indent(out, 1);
    out.push('}');
}

fn write_completion_rate(
    out: &mut String,
    indent: usize,
    name: &str,
    completion_rate: &CompletionRate,
    comma: bool,
) {
    write_field_name(out, indent, name);
    out.push_str("{\n");
    write_u64_field(
        out,
        indent + 1,
        "completed",
        completion_rate.completed,
        true,
    );
    write_u64_field(out, indent + 1, "failed", completion_rate.failed, true);
    write_u64_field(
        out,
        indent + 1,
        "incomplete",
        completion_rate.incomplete,
        true,
    );
    write_u64_field(out, indent + 1, "total", completion_rate.total(), true);
    write_f64_field(out, indent + 1, "rate", completion_rate.rate, false);
    write_indent(out, indent);
    out.push('}');
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn write_repeat_metrics(out: &mut String, repeat_metrics: &RepeatMetrics) {
    write_field_name(out, 1, "repeat_metrics");
    out.push_str("{\n");
    write_u64_field(
        out,
        2,
        "repeat_event_count",
        repeat_metrics.repeat_event_count,
        true,
    );
    write_u64_field(out, 2, "repeat_tokens", repeat_metrics.repeat_tokens, true);
    write_f64_field(
        out,
        2,
        "repeat_token_share",
        repeat_metrics.repeat_token_share,
        false,
    );
    write_indent(out, 1);
    out.push('}');
}

fn write_cache_metrics(out: &mut String, cache_metrics: &CacheMetrics) {
    write_field_name(out, 1, "cache_metrics");
    out.push_str("{\n");
    write_u64_field(
        out,
        2,
        "cache_read_tokens",
        cache_metrics.cache_read_tokens,
        true,
    );
    write_u64_field(
        out,
        2,
        "cache_write_tokens",
        cache_metrics.cache_write_tokens,
        true,
    );
    write_u64_field(out, 2, "cache_tokens", cache_metrics.cache_tokens, true);
    write_f64_field(
        out,
        2,
        "cache_token_share",
        cache_metrics.cache_token_share,
        false,
    );
    write_indent(out, 1);
    out.push('}');
}

fn write_calibration(out: &mut String, calibration: &CalibrationMetadata<'_>) {
    write_field_name(out, 1, "calibration");
    out.push_str("{\n");
    write_optional_str_field(out, 2, "source", calibration.source, true);
    write_optional_str_field(out, 2, "tokenizer", calibration.tokenizer, true);
    write_optional_str_field(out, 2, "calibration_id", calibration.calibration_id, true);
    write_u64_field(out, 2, "sample_count", calibration.sample_count, true);
    write_optional_f64_field(
        out,
        2,
        "mean_absolute_error",
        calibration.mean_absolute_error,
        false,
    );
    write_indent(out, 1);
    out.push('}');
}

fn write_warnings(out: &mut String, warnings: &[&str]) {
    write_field_name(out, 1, "warnings");
    out.push_str("[\n");
    for (index, warning) in warnings.iter().enumerate() {
        write_indent(out, 2);
        write_json_string(out, warning);
        if index + 1 != warnings.len() {
            out.push(',');
        }
        out.push('\n');
    }
    write_indent(out, 1);
    out.push(']');
}

fn write_u32_field(out: &mut String, indent: usize, name: &str, value: u32, comma: bool) {
    write_field_name(out, indent, name);
    out.push_str(&value.to_string());
    finish_field(out, comma);
}

fn write_u64_field(out: &mut String, indent: usize, name: &str, value: u64, comma: bool) {
    write_field_name(out, indent, name);
    out.push_str(&value.to_string());
    finish_field(out, comma);
}

fn write_f64_field(out: &mut String, indent: usize, name: &str, value: f64, comma: bool) {
    write_field_name(out, indent, name);
    out.push_str(&json_number(value));
    finish_field(out, comma);
}

fn write_optional_f64_field(
    out: &mut String,
    indent: usize,
    name: &str,
    value: Option<f64>,
    comma: bool,
) {
    write_field_name(out, indent, name);
    match value {
        Some(value) => out.push_str(&json_number(value)),
        None => out.push_str("null"),
    }
    finish_field(out, comma);
}

fn write_str_field(out: &mut String, indent: usize, name: &str, value: &str, comma: bool) {
    write_field_name(out, indent, name);
    write_json_string(out, value);
    finish_field(out, comma);
}

fn write_optional_str_field(
    out: &mut String,
    indent: usize,
    name: &str,
    value: Option<&str>,
    comma: bool,
) {
    write_field_name(out, indent, name);
    match value {
        Some(value) => write_json_string(out, value),
        None => out.push_str("null"),
    }
    finish_field(out, comma);
}

fn write_field_name(out: &mut String, indent: usize, name: &str) {
    write_indent(out, indent);
    write_json_string(out, name);
    out.push_str(": ");
}

fn finish_field(out: &mut String, comma: bool) {
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn json_number(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_owned();
    }

    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            character if character <= '\u{1f}' => {
                out.push_str("\\u00");
                out.push(hex_digit((character as u32 >> 4) & 0x0f));
                out.push(hex_digit(character as u32 & 0x0f));
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

fn write_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn hex_digit(value: u32) -> char {
    match value {
        0..=9 => char::from(b'0' + value as u8),
        10..=15 => char::from(b'a' + (value as u8 - 10)),
        _ => unreachable!("hex digit out of range"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_stable_schema_order() {
        let run_profile_totals = [
            RunProfileTotals {
                run_id: "run-a",
                profile_id: "default",
                totals: Totals {
                    event_count: 2,
                    run_count: 1,
                    task_count: 1,
                    input_tokens: 100,
                    output_tokens: 20,
                    cache_read_tokens: 30,
                    cache_write_tokens: 5,
                    total_tokens: 155,
                    byte_count: 2048,
                },
            },
            RunProfileTotals {
                run_id: "run-b",
                profile_id: "review",
                totals: Totals {
                    event_count: 1,
                    run_count: 1,
                    task_count: 1,
                    input_tokens: 50,
                    output_tokens: 10,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    total_tokens: 60,
                    byte_count: 512,
                },
            },
        ];
        let class_shares = [
            ClassShare {
                operation_class: "file.read",
                tokens: 90,
                share: 0.42,
            },
            ClassShare {
                operation_class: "vc.diff",
                tokens: 70,
                share: 0.33,
            },
        ];
        let warnings = ["missing calibration for adapter alpha"];
        let report = ReportJson {
            evidence_grade: EvidenceGrade::GradeP,
            totals: Totals {
                event_count: 3,
                run_count: 2,
                task_count: 2,
                input_tokens: 150,
                output_tokens: 30,
                cache_read_tokens: 30,
                cache_write_tokens: 5,
                total_tokens: 215,
                byte_count: 2560,
            },
            run_profile_totals: &run_profile_totals,
            class_shares: &class_shares,
            completion_rates: CompletionRates {
                tasks: CompletionRate {
                    completed: 1,
                    failed: 1,
                    incomplete: 0,
                    rate: 0.5,
                },
                runs: CompletionRate {
                    completed: 1,
                    failed: 0,
                    incomplete: 1,
                    rate: 0.5,
                },
            },
            repeat_metrics: RepeatMetrics {
                repeat_event_count: 1,
                repeat_tokens: 15,
                repeat_token_share: 0.07,
            },
            cache_metrics: CacheMetrics {
                cache_read_tokens: 30,
                cache_write_tokens: 5,
                cache_tokens: 35,
                cache_token_share: 0.16,
            },
            calibration: CalibrationMetadata {
                source: Some("fixture"),
                tokenizer: Some("tokmeter-test"),
                calibration_id: Some("cal-001"),
                sample_count: 12,
                mean_absolute_error: Some(0.125),
            },
            warnings: &warnings,
        };

        let json = serialize_report_json(&report);

        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"evidence_grade\": \"Grade P\""));
        assert!(json.contains("\"evidence_caption\": \"Protocol: completed Mode T task runs with controlled task/profile pairing.\""));
        assert!(json.contains("\"total_tokens\": 215"));
        assert!(json.contains("\"operation_class\": \"vc.diff\""));
        assert!(json.contains("\"missing calibration for adapter alpha\""));
    }

    #[test]
    fn escapes_json_strings_in_snapshot_output() {
        let run_profile_totals = [RunProfileTotals {
            run_id: "run\"quoted",
            profile_id: "profile\\windows",
            totals: Totals::default(),
        }];
        let class_shares = [ClassShare {
            operation_class: "line\nclass",
            tokens: 0,
            share: f64::NAN,
        }];
        let warnings = ["tab\tnewline\nbell\u{07}"];
        let report = ReportJson {
            evidence_grade: EvidenceGrade::GradeO,
            totals: Totals::default(),
            run_profile_totals: &run_profile_totals,
            class_shares: &class_shares,
            completion_rates: CompletionRates::default(),
            repeat_metrics: RepeatMetrics {
                repeat_event_count: 0,
                repeat_tokens: 0,
                repeat_token_share: f64::NEG_INFINITY,
            },
            cache_metrics: CacheMetrics::default(),
            calibration: CalibrationMetadata {
                source: Some("source\rname"),
                tokenizer: None,
                calibration_id: Some("id\u{08}\u{0c}"),
                sample_count: 0,
                mean_absolute_error: None,
            },
            warnings: &warnings,
        };

        let json = report.to_json();

        assert!(json.contains("\"evidence_grade\": \"Grade O\""));
        assert!(json.contains("\"run_id\": \"run\\\"quoted\""));
        assert!(json.contains("\"profile_id\": \"profile\\\\windows\""));
        assert!(json.contains("\"operation_class\": \"line\\nclass\""));
        assert!(json.contains("\"repeat_token_share\": null"));
        assert!(json.contains("\"tab\\tnewline\\nbell\\u0007\""));
    }
}
