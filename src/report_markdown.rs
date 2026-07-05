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

    pub const fn allows_savings_headline(self) -> bool {
        matches!(self, Self::GradeP)
    }
}

impl Default for EvidenceGrade {
    fn default() -> Self {
        Self::GradeO
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarkdownReport<'a> {
    pub title: &'a str,
    pub evidence_grade: EvidenceGrade,
    pub single_profile: Option<ProfileSummary<'a>>,
    pub comparison: Option<CompareSummary<'a>>,
    pub warnings: &'a [&'a str],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileSummary<'a> {
    pub profile_id: &'a str,
    pub event_count: u64,
    pub run_count: u64,
    pub task_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub byte_count: u64,
    pub completion_rates: CompletionRates,
    pub session_git_share: Option<SessionGitShare<'a>>,
    pub token_distribution: Option<Distribution>,
    pub token_sources: Option<TokenSourceSummary<'a>>,
    pub git_workflow: Option<GitWorkflowSummary<'a>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SessionGitShare<'a> {
    pub total_tokens: u64,
    pub git_tokens: u64,
    pub non_git_tokens: u64,
    pub git_token_share: f64,
    pub fidelity: &'a str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenSourceSummary<'a> {
    pub rows: &'a [TokenSourceRow<'a>],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenSourceRow<'a> {
    pub source: &'a str,
    pub event_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub byte_count: u64,
    pub token_share: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GitWorkflowSummary<'a> {
    pub event_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub byte_count: u64,
    pub token_share: f64,
    pub rows: &'a [GitWorkflowRow<'a>],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GitWorkflowRow<'a> {
    pub action_subtype: &'a str,
    pub direction: &'a str,
    pub operation_class: &'a str,
    pub event_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub byte_count: u64,
    pub token_share: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompareSummary<'a> {
    pub baseline_profile_id: &'a str,
    pub treatment_profile_id: &'a str,
    pub rows: &'a [CompareRow<'a>],
    pub completion_rates: CompletionRateComparison,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompareRow<'a> {
    pub metric: &'a str,
    pub baseline: MetricValue,
    pub treatment: MetricValue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MetricValue {
    pub value: f64,
    pub median: Option<f64>,
    pub iqr: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Distribution {
    pub median: Option<f64>,
    pub iqr: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompletionRates {
    pub tasks: CompletionRate,
    pub runs: CompletionRate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompletionRateComparison {
    pub tasks: CompletionRatePair,
    pub runs: CompletionRatePair,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompletionRatePair {
    pub baseline: CompletionRate,
    pub treatment: CompletionRate,
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

pub fn serialize_report_markdown(report: &MarkdownReport<'_>) -> String {
    render_report_markdown(report)
}

pub fn render_report_markdown(report: &MarkdownReport<'_>) -> String {
    let mut out = String::new();
    let title = if report.title.trim().is_empty() {
        "vc-tokmeter report"
    } else {
        report.title
    };

    out.push_str("# ");
    out.push_str(&escape_inline(title));
    out.push_str("\n\n");
    write_warnings(&mut out, report.warnings);
    write_evidence_grade(&mut out, report.evidence_grade);

    if let Some(summary) = &report.single_profile {
        write_profile_summary(&mut out, summary, report.evidence_grade);
    }

    if let Some(comparison) = &report.comparison {
        write_compare_summary(&mut out, comparison, report.evidence_grade);
    }

    out
}

pub fn render_profile_summary_markdown(summary: &ProfileSummary<'_>, warnings: &[&str]) -> String {
    render_report_markdown(&MarkdownReport {
        single_profile: Some(*summary),
        warnings,
        ..MarkdownReport::default()
    })
}

pub fn render_compare_markdown(comparison: &CompareSummary<'_>, warnings: &[&str]) -> String {
    render_report_markdown(&MarkdownReport {
        comparison: Some(*comparison),
        warnings,
        ..MarkdownReport::default()
    })
}

fn write_warnings(out: &mut String, warnings: &[&str]) {
    if warnings.is_empty() {
        return;
    }

    out.push_str("> **Warning:** ");
    for (index, warning) in warnings.iter().enumerate() {
        if index != 0 {
            out.push_str("\n> **Warning:** ");
        }
        out.push_str(&escape_blockquote(warning));
    }
    out.push_str("\n\n");
}

fn write_evidence_grade(out: &mut String, grade: EvidenceGrade) {
    out.push_str("**Evidence:** ");
    out.push_str(grade.label());
    out.push_str(" - ");
    out.push_str(grade.caption());
    out.push_str("\n\n");
}

fn write_profile_summary(out: &mut String, summary: &ProfileSummary<'_>, grade: EvidenceGrade) {
    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Evidence | Value |\n");
    out.push_str("| --- | --- | ---: |\n");
    write_table_row(
        out,
        "Profile",
        grade,
        &escape_table_cell(summary.profile_id),
    );
    write_table_row(out, "Runs", grade, &format_u64(summary.run_count));
    write_table_row(out, "Tasks", grade, &format_u64(summary.task_count));
    write_table_row(out, "Events", grade, &format_u64(summary.event_count));
    write_table_row(
        out,
        "Total tokens",
        grade,
        &format_u64(summary.total_tokens),
    );
    write_table_row(
        out,
        "Input tokens",
        grade,
        &format_u64(summary.input_tokens),
    );
    write_table_row(
        out,
        "Output tokens",
        grade,
        &format_u64(summary.output_tokens),
    );
    write_table_row(
        out,
        "Cache read tokens",
        grade,
        &format_u64(summary.cache_read_tokens),
    );
    write_table_row(
        out,
        "Cache write tokens",
        grade,
        &format_u64(summary.cache_write_tokens),
    );
    write_table_row(out, "Bytes", grade, &format_u64(summary.byte_count));

    if let Some(distribution) = summary.token_distribution {
        if let Some(median) = distribution.median {
            write_table_row(out, "Median tokens", grade, &format_number(median));
        }
        if let Some(iqr) = distribution.iqr {
            write_table_row(out, "IQR tokens", grade, &format_number(iqr));
        }
    }

    out.push('\n');
    if let Some(session_git_share) = summary.session_git_share {
        write_session_git_share(out, &session_git_share, grade);
    }
    if let Some(token_sources) = summary.token_sources {
        write_token_sources(out, &token_sources, grade);
    }
    if let Some(git_workflow) = summary.git_workflow {
        write_git_workflow(out, &git_workflow, grade);
    }
    write_single_completion_rates(out, &summary.completion_rates);
}

fn write_session_git_share(out: &mut String, share: &SessionGitShare<'_>, grade: EvidenceGrade) {
    out.push_str("## Session git token share\n\n");
    out.push_str("| Metric | Evidence | Value |\n");
    out.push_str("| --- | --- | ---: |\n");
    write_table_row(
        out,
        "Session total tokens",
        grade,
        &format_u64(share.total_tokens),
    );
    write_table_row(out, "Git tokens", grade, &format_u64(share.git_tokens));
    write_table_row(
        out,
        "Non-git tokens",
        grade,
        &format_u64(share.non_git_tokens),
    );
    write_table_row(
        out,
        "Git token share",
        grade,
        &format_percent(share.git_token_share),
    );
    write_table_row(out, "Token fidelity", grade, share.fidelity);
    out.push('\n');
}

fn write_token_sources(
    out: &mut String,
    token_sources: &TokenSourceSummary<'_>,
    grade: EvidenceGrade,
) {
    out.push_str("## Token source breakdown\n\n");
    out.push_str("| Source | Evidence | Events | Total tokens | Input | Output | Cache read | Cache write | Bytes | Share of all tokens |\n");
    out.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for row in token_sources.rows {
        out.push_str("| ");
        out.push_str(&escape_table_cell(row.source));
        out.push_str(" | ");
        out.push_str(grade.label());
        out.push_str(" | ");
        out.push_str(&format_u64(row.event_count));
        out.push_str(" | ");
        out.push_str(&format_u64(row.total_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.input_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.output_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.cache_read_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.cache_write_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.byte_count));
        out.push_str(" | ");
        out.push_str(&format_percent(row.token_share));
        out.push_str(" |\n");
    }
    out.push('\n');
}

fn write_git_workflow(
    out: &mut String,
    git_workflow: &GitWorkflowSummary<'_>,
    grade: EvidenceGrade,
) {
    out.push_str("## Git workflow tokens\n\n");
    out.push_str("| Metric | Evidence | Value |\n");
    out.push_str("| --- | --- | ---: |\n");
    write_table_row(
        out,
        "Git total tokens",
        grade,
        &format_u64(git_workflow.total_tokens),
    );
    write_table_row(
        out,
        "Git token share",
        grade,
        &format_percent(git_workflow.token_share),
    );
    write_table_row(
        out,
        "Git events",
        grade,
        &format_u64(git_workflow.event_count),
    );
    write_table_row(
        out,
        "Git input tokens",
        grade,
        &format_u64(git_workflow.input_tokens),
    );
    write_table_row(
        out,
        "Git output tokens",
        grade,
        &format_u64(git_workflow.output_tokens),
    );
    write_table_row(
        out,
        "Git cache read tokens",
        grade,
        &format_u64(git_workflow.cache_read_tokens),
    );
    write_table_row(
        out,
        "Git cache write tokens",
        grade,
        &format_u64(git_workflow.cache_write_tokens),
    );
    write_table_row(
        out,
        "Git bytes",
        grade,
        &format_u64(git_workflow.byte_count),
    );
    out.push('\n');

    out.push_str("| Action subtype | Direction | Operation class | Evidence | Events | Total tokens | Input | Output | Cache read | Cache write | Bytes | Share of all tokens |\n");
    out.push_str(
        "| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for row in git_workflow.rows {
        out.push_str("| ");
        out.push_str(&escape_table_cell(row.action_subtype));
        out.push_str(" | ");
        out.push_str(&escape_table_cell(row.direction));
        out.push_str(" | ");
        out.push_str(&escape_table_cell(row.operation_class));
        out.push_str(" | ");
        out.push_str(grade.label());
        out.push_str(" | ");
        out.push_str(&format_u64(row.event_count));
        out.push_str(" | ");
        out.push_str(&format_u64(row.total_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.input_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.output_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.cache_read_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.cache_write_tokens));
        out.push_str(" | ");
        out.push_str(&format_u64(row.byte_count));
        out.push_str(" | ");
        out.push_str(&format_percent(row.token_share));
        out.push_str(" |\n");
    }
    out.push('\n');
}

fn write_single_completion_rates(out: &mut String, completion_rates: &CompletionRates) {
    out.push_str("## Completion rates\n\n");
    out.push_str("| Scope | Completed | Failed | Incomplete | Total | Completion rate |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: |\n");
    write_completion_rate_row(out, "Tasks", completion_rates.tasks);
    write_completion_rate_row(out, "Runs", completion_rates.runs);
    out.push('\n');
}

fn write_compare_summary(out: &mut String, comparison: &CompareSummary<'_>, grade: EvidenceGrade) {
    out.push_str("## Baseline vs treatment\n\n");
    out.push_str("Baseline: **");
    out.push_str(&escape_inline(comparison.baseline_profile_id));
    out.push_str("**  \nTreatment: **");
    out.push_str(&escape_inline(comparison.treatment_profile_id));
    out.push_str("**\n\n");

    if !comparison.rows.is_empty() {
        write_compare_rows(out, comparison.rows, grade);
    }
    write_completion_comparison(out, &comparison.completion_rates);
}

fn write_compare_rows(out: &mut String, rows: &[CompareRow<'_>], grade: EvidenceGrade) {
    let has_distribution = rows.iter().any(|row| {
        row.baseline.median.is_some()
            || row.baseline.iqr.is_some()
            || row.treatment.median.is_some()
            || row.treatment.iqr.is_some()
    });

    if has_distribution {
        out.push_str("| Metric | Evidence | Baseline | Treatment | Delta (baseline - treatment) | Baseline median | Baseline IQR | Treatment median | Treatment IQR |\n");
        out.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    } else {
        out.push_str(
            "| Metric | Evidence | Baseline | Treatment | Delta (baseline - treatment) |\n",
        );
        out.push_str("| --- | --- | ---: | ---: | ---: |\n");
    }

    for row in rows {
        out.push_str("| ");
        out.push_str(&escape_table_cell(row.metric));
        out.push_str(" | ");
        out.push_str(grade.label());
        out.push_str(" | ");
        out.push_str(&format_number(row.baseline.value));
        out.push_str(" | ");
        out.push_str(&format_number(row.treatment.value));
        out.push_str(" | ");
        out.push_str(&format_delta(
            row.baseline.value,
            row.treatment.value,
            grade,
        ));
        if has_distribution {
            out.push_str(" | ");
            out.push_str(&format_optional_number(row.baseline.median));
            out.push_str(" | ");
            out.push_str(&format_optional_number(row.baseline.iqr));
            out.push_str(" | ");
            out.push_str(&format_optional_number(row.treatment.median));
            out.push_str(" | ");
            out.push_str(&format_optional_number(row.treatment.iqr));
        }
        out.push_str(" |\n");
    }
    out.push('\n');
}

fn write_completion_comparison(out: &mut String, comparison: &CompletionRateComparison) {
    out.push_str("## Completion rate changes\n\n");
    out.push_str("| Scope | Baseline | Treatment | Change |\n");
    out.push_str("| --- | ---: | ---: | ---: |\n");
    write_completion_comparison_row(out, "Tasks", comparison.tasks);
    write_completion_comparison_row(out, "Runs", comparison.runs);
    out.push('\n');
}

fn write_table_row(out: &mut String, metric: &str, grade: EvidenceGrade, value: &str) {
    out.push_str("| ");
    out.push_str(metric);
    out.push_str(" | ");
    out.push_str(grade.label());
    out.push_str(" | ");
    out.push_str(value);
    out.push_str(" |\n");
}

fn write_completion_rate_row(out: &mut String, scope: &str, rate: CompletionRate) {
    out.push_str("| ");
    out.push_str(scope);
    out.push_str(" | ");
    out.push_str(&format_u64(rate.completed));
    out.push_str(" | ");
    out.push_str(&format_u64(rate.failed));
    out.push_str(" | ");
    out.push_str(&format_u64(rate.incomplete));
    out.push_str(" | ");
    out.push_str(&format_u64(rate.total()));
    out.push_str(" | ");
    out.push_str(&format_rate(rate));
    out.push_str(" |\n");
}

fn write_completion_comparison_row(out: &mut String, scope: &str, pair: CompletionRatePair) {
    out.push_str("| ");
    out.push_str(scope);
    out.push_str(" | ");
    out.push_str(&format_rate_with_counts(pair.baseline));
    out.push_str(" | ");
    out.push_str(&format_rate_with_counts(pair.treatment));
    out.push_str(" | ");
    out.push_str(&format_completion_rate_change(pair));
    out.push_str(" |\n");
}

fn format_rate_with_counts(rate: CompletionRate) -> String {
    format!(
        "{} ({}/{})",
        format_rate(rate),
        format_u64(rate.completed),
        format_u64(rate.total())
    )
}

fn format_rate(rate: CompletionRate) -> String {
    if rate.total() == 0 || !rate.rate.is_finite() {
        "n/a".to_owned()
    } else {
        format_percent(rate.rate)
    }
}

fn format_delta(baseline: f64, treatment: f64, grade: EvidenceGrade) -> String {
    let delta = baseline - treatment;
    if baseline == 0.0 || !baseline.is_finite() || !treatment.is_finite() {
        return format!("{} (n/a)", format_number(delta));
    }

    if !grade.allows_savings_headline() {
        return format!("{} descriptive delta only", format_number(delta));
    }

    format!(
        "{} ({})",
        format_number(delta),
        format_percent(delta / baseline)
    )
}

fn format_completion_rate_change(pair: CompletionRatePair) -> String {
    if pair.baseline.total() == 0 || pair.treatment.total() == 0 {
        return "n/a".to_owned();
    }

    format_percentage_point_change(pair.treatment.rate - pair.baseline.rate)
}

fn format_percentage_point_change(change: f64) -> String {
    if !change.is_finite() {
        return "n/a".to_owned();
    }

    let mut formatted = format!("{:.1} pp", change * 100.0);
    if change > 0.0 {
        formatted.insert(0, '+');
    }
    formatted
}

fn format_percent(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_owned();
    }
    format!("{:.1}%", value * 100.0)
}

fn format_optional_number(value: Option<f64>) -> String {
    value.map(format_number).unwrap_or_else(|| "n/a".to_owned())
}

fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_owned();
    }

    let rounded = value.round();
    if (value - rounded).abs() < f64::EPSILON {
        format_i64(rounded as i64)
    } else {
        format_decimal(value)
    }
}

fn format_u64(value: u64) -> String {
    add_group_separators(&value.to_string())
}

fn format_i64(value: i64) -> String {
    if value < 0 {
        format!("-{}", add_group_separators(&value.abs().to_string()))
    } else {
        add_group_separators(&value.to_string())
    }
}

fn format_decimal(value: f64) -> String {
    let formatted = format!("{:.1}", value.abs());
    match formatted.split_once('.') {
        Some((whole, fraction)) if value < 0.0 => {
            format!("-{}.{}", add_group_separators(whole), fraction)
        }
        Some((whole, fraction)) => format!("{}.{}", add_group_separators(whole), fraction),
        None if value < 0.0 => format!("-{}", add_group_separators(&formatted)),
        None => add_group_separators(&formatted),
    }
}

fn add_group_separators(digits: &str) -> String {
    let mut out = String::new();
    let first_group_len = match digits.len() % 3 {
        0 => 3,
        len => len,
    };

    for (index, digit) in digits.chars().enumerate() {
        if index != 0
            && (index == first_group_len
                || (index > first_group_len && (index - first_group_len) % 3 == 0))
        {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn escape_inline(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => ' ',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect()
}

fn escape_table_cell(value: &str) -> String {
    let mut out = String::new();
    for character in value.chars() {
        match character {
            '|' => out.push_str("\\|"),
            '\n' | '\r' | '\t' => out.push(' '),
            character if character.is_control() => out.push(' '),
            character => out.push(character),
        }
    }
    out
}

fn escape_blockquote(value: &str) -> String {
    let mut out = String::new();
    for character in value.chars() {
        match character {
            '\n' | '\r' => out.push_str("\n> "),
            '\t' => out.push(' '),
            character if character.is_control() => out.push(' '),
            character => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_single_profile_summary_with_distribution_and_warnings() {
        let git_rows = [GitWorkflowRow {
            action_subtype: "status",
            direction: "response",
            operation_class: "vc.status",
            event_count: 2,
            input_tokens: 1_000,
            output_tokens: 250,
            cache_read_tokens: 100,
            cache_write_tokens: 25,
            total_tokens: 1_375,
            byte_count: 4_200,
            token_share: 0.1,
        }];
        let report = MarkdownReport {
            title: "report.md",
            single_profile: Some(ProfileSummary {
                profile_id: "baseline",
                event_count: 12,
                run_count: 2,
                task_count: 5,
                input_tokens: 10_000,
                output_tokens: 2_500,
                cache_read_tokens: 1_000,
                cache_write_tokens: 250,
                total_tokens: 13_750,
                byte_count: 42_000,
                completion_rates: CompletionRates {
                    tasks: CompletionRate {
                        completed: 4,
                        failed: 1,
                        incomplete: 0,
                        rate: 0.8,
                    },
                    runs: CompletionRate {
                        completed: 1,
                        failed: 0,
                        incomplete: 1,
                        rate: 0.5,
                    },
                },
                token_distribution: Some(Distribution {
                    median: Some(2_750.0),
                    iqr: Some(500.5),
                }),
                session_git_share: Some(SessionGitShare {
                    total_tokens: 13_750,
                    git_tokens: 1_375,
                    non_git_tokens: 12_375,
                    git_token_share: 0.1,
                    fidelity: "estimated",
                }),
                token_sources: None,
                git_workflow: Some(GitWorkflowSummary {
                    event_count: 2,
                    input_tokens: 1_000,
                    output_tokens: 250,
                    cache_read_tokens: 100,
                    cache_write_tokens: 25,
                    total_tokens: 1_375,
                    byte_count: 4_200,
                    token_share: 0.1,
                    rows: &git_rows,
                }),
            }),
            warnings: &["metadata mismatch"],
            ..MarkdownReport::default()
        };

        let markdown = serialize_report_markdown(&report);

        assert!(markdown.contains("# report.md"));
        assert!(markdown.contains("> **Warning:** metadata mismatch"));
        assert!(markdown.contains("**Evidence:** Grade O - Observational: workloads were not controlled; differences may reflect changes in the work itself."));
        assert!(markdown.contains("| Metric | Evidence | Value |"));
        assert!(markdown.contains("| Total tokens | Grade O | 13,750 |"));
        assert!(markdown.contains("| IQR tokens | Grade O | 500.5 |"));
        assert!(markdown.contains("## Session git token share"));
        assert!(markdown.contains("| Git tokens | Grade O | 1,375 |"));
        assert!(markdown.contains("| Token fidelity | Grade O | estimated |"));
        assert!(markdown.contains("## Git workflow tokens"));
        assert!(markdown.contains("| Git token share | Grade O | 10.0% |"));
        assert!(markdown.contains(
            "| status | response | vc.status | Grade O | 2 | 1,375 | 1,000 | 250 | 100 | 25 | 4,200 | 10.0% |"
        ));
        assert!(markdown.contains("| Tasks | 4 | 1 | 0 | 5 | 80.0% |"));
    }

    #[test]
    fn renders_compare_rows_separately_from_completion_rate_changes() {
        let rows = [
            CompareRow {
                metric: "Total tokens",
                baseline: MetricValue {
                    value: 12_000.0,
                    median: Some(3_000.0),
                    iqr: Some(900.0),
                },
                treatment: MetricValue {
                    value: 9_000.0,
                    median: Some(2_100.0),
                    iqr: Some(600.0),
                },
            },
            CompareRow {
                metric: "Input tokens",
                baseline: MetricValue {
                    value: 10_000.0,
                    median: None,
                    iqr: None,
                },
                treatment: MetricValue {
                    value: 8_500.0,
                    median: None,
                    iqr: None,
                },
            },
        ];
        let comparison = CompareSummary {
            baseline_profile_id: "baseline",
            treatment_profile_id: "treatment",
            rows: &rows,
            completion_rates: CompletionRateComparison {
                tasks: CompletionRatePair {
                    baseline: CompletionRate {
                        completed: 9,
                        failed: 1,
                        incomplete: 0,
                        rate: 0.9,
                    },
                    treatment: CompletionRate {
                        completed: 8,
                        failed: 1,
                        incomplete: 1,
                        rate: 0.8,
                    },
                },
                runs: CompletionRatePair {
                    baseline: CompletionRate {
                        completed: 3,
                        failed: 0,
                        incomplete: 0,
                        rate: 1.0,
                    },
                    treatment: CompletionRate {
                        completed: 3,
                        failed: 0,
                        incomplete: 0,
                        rate: 1.0,
                    },
                },
            },
        };

        let markdown = render_compare_markdown(&comparison, &[]);

        assert!(markdown.contains("Delta (baseline - treatment)"));
        assert!(markdown.contains(
            "| Total tokens | Grade O | 12,000 | 9,000 | 3,000 descriptive delta only | 3,000 | 900 | 2,100 | 600 |"
        ));
        assert!(markdown.contains("## Completion rate changes"));
        assert!(markdown.contains("| Tasks | 90.0% (9/10) | 80.0% (8/10) | -10.0 pp |"));
        assert!(!markdown.contains("completion savings"));
    }

    #[test]
    fn escapes_table_cells_and_preserves_warning_banner_lines() {
        let summary = ProfileSummary {
            profile_id: "base|line\none",
            completion_rates: CompletionRates::default(),
            ..ProfileSummary::default()
        };

        let markdown = render_profile_summary_markdown(&summary, &["first line\nsecond | line"]);

        assert!(markdown.contains("> **Warning:** first line\n> second | line\n\n"));
        assert!(markdown.contains("| Profile | Grade O | base\\|line one |"));
    }

    #[test]
    fn formats_zero_baselines_and_empty_completion_rates_as_not_available() {
        let rows = [CompareRow {
            metric: "Total tokens",
            baseline: MetricValue {
                value: 0.0,
                ..MetricValue::default()
            },
            treatment: MetricValue {
                value: 100.0,
                ..MetricValue::default()
            },
        }];
        let comparison = CompareSummary {
            rows: &rows,
            completion_rates: CompletionRateComparison::default(),
            ..CompareSummary::default()
        };

        let markdown = render_compare_markdown(&comparison, &[]);

        assert!(markdown.contains("| Total tokens | Grade O | 0 | 100 | -100 (n/a) |"));
        assert!(markdown.contains("| Tasks | n/a (0/0) | n/a (0/0) | n/a |"));
    }

    #[test]
    fn groups_decimal_numbers() {
        let rows = [CompareRow {
            metric: "Total tokens",
            baseline: MetricValue {
                value: 12_345.5,
                ..MetricValue::default()
            },
            treatment: MetricValue {
                value: 10_000.0,
                ..MetricValue::default()
            },
        }];
        let comparison = CompareSummary {
            rows: &rows,
            ..CompareSummary::default()
        };

        let markdown = render_compare_markdown(&comparison, &[]);

        assert!(markdown.contains(
            "| Total tokens | Grade O | 12,345.5 | 10,000 | 2,345.5 descriptive delta only |"
        ));
    }

    #[test]
    fn grade_p_comparisons_can_show_savings_percentages() {
        let rows = [CompareRow {
            metric: "Tokens per completed task",
            baseline: MetricValue {
                value: 12_000.0,
                ..MetricValue::default()
            },
            treatment: MetricValue {
                value: 9_000.0,
                ..MetricValue::default()
            },
        }];
        let report = MarkdownReport {
            evidence_grade: EvidenceGrade::GradeP,
            comparison: Some(CompareSummary {
                rows: &rows,
                ..CompareSummary::default()
            }),
            ..MarkdownReport::default()
        };

        let markdown = render_report_markdown(&report);

        assert!(markdown.contains("**Evidence:** Grade P"));
        assert!(
            markdown.contains(
                "| Tokens per completed task | Grade P | 12,000 | 9,000 | 3,000 (25.0%) |"
            )
        );
    }
}
