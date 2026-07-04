pub const EDIT_ECHO_CLASS: &str = "edit.echo";
pub const OTHER_CLASS: &str = "other";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenBuckets {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl TokenBuckets {
    pub const fn new(
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        }
    }

    pub fn total(self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageSource {
    Hook,
    Proxy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QualityEvent<'a> {
    pub operation_class: &'a str,
    pub tokens: TokenBuckets,
    pub source: UsageSource,
    pub unattributed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheEconomics {
    pub fresh_input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_to_fresh_input_ratio: Option<f64>,
}

pub fn cache_economics(events: &[QualityEvent<'_>]) -> Option<CacheEconomics> {
    let mut economics = CacheEconomics::default();
    let mut has_proxy_usage = false;

    for event in events {
        if event.source != UsageSource::Proxy {
            continue;
        }

        has_proxy_usage = true;
        economics.fresh_input_tokens = economics
            .fresh_input_tokens
            .saturating_add(event.tokens.input_tokens);
        economics.cache_read_tokens = economics
            .cache_read_tokens
            .saturating_add(event.tokens.cache_read_tokens);
        economics.cache_write_tokens = economics
            .cache_write_tokens
            .saturating_add(event.tokens.cache_write_tokens);
    }

    if !has_proxy_usage {
        return None;
    }

    economics.cache_read_to_fresh_input_ratio =
        ratio(economics.cache_read_tokens, economics.fresh_input_tokens);
    Some(economics)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationSample {
    pub estimated_tokens: u64,
    pub reported_tokens: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalibrationSummary {
    pub sample_count: u64,
    pub estimated_tokens: u64,
    pub reported_tokens: u64,
    pub factor: Option<f64>,
}

pub fn summarize_calibration(samples: &[CalibrationSample]) -> CalibrationSummary {
    let mut summary = CalibrationSummary::default();

    for sample in samples {
        summary.sample_count = summary.sample_count.saturating_add(1);
        summary.estimated_tokens = summary
            .estimated_tokens
            .saturating_add(sample.estimated_tokens);
        summary.reported_tokens = summary
            .reported_tokens
            .saturating_add(sample.reported_tokens);
    }

    summary.factor = ratio(summary.reported_tokens, summary.estimated_tokens);
    summary
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnattributedSummary {
    pub event_count: u64,
    pub token_count: u64,
    pub total_tokens: u64,
    pub share: f64,
}

pub fn summarize_unattributed(events: &[QualityEvent<'_>]) -> UnattributedSummary {
    let mut summary = UnattributedSummary::default();

    for event in events {
        let total = event.tokens.total();
        summary.total_tokens = summary.total_tokens.saturating_add(total);

        if event.unattributed || event.operation_class == OTHER_CLASS {
            summary.event_count = summary.event_count.saturating_add(1);
            summary.token_count = summary.token_count.saturating_add(total);
        }
    }

    summary.share = ratio_or_zero(summary.token_count, summary.total_tokens);
    summary
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HookQualityIndicators {
    pub calibration: CalibrationSummary,
    pub unattributed: UnattributedSummary,
}

pub fn hook_quality_indicators(
    events: &[QualityEvent<'_>],
    samples: &[CalibrationSample],
) -> HookQualityIndicators {
    let hook_events: Vec<QualityEvent<'_>> = events
        .iter()
        .copied()
        .filter(|event| event.source == UsageSource::Hook)
        .collect();

    HookQualityIndicators {
        calibration: summarize_calibration(samples),
        unattributed: summarize_unattributed(&hook_events),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MismatchKind {
    Model,
    Setting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mismatch<'a> {
    pub kind: MismatchKind,
    pub name: &'a str,
    pub baseline: &'a str,
    pub treatment: &'a str,
}

pub fn render_mismatch_warning(mismatches: &[Mismatch<'_>]) -> Option<String> {
    if mismatches.is_empty() {
        return None;
    }

    let mut out = String::from("WARNING: comparison metadata mismatch\n");
    out.push_str("Token comparisons may not be apples-to-apples.\n");

    for mismatch in mismatches {
        out.push_str("- ");
        match mismatch.kind {
            MismatchKind::Model => out.push_str("model"),
            MismatchKind::Setting => {
                out.push_str("setting ");
                out.push_str(mismatch.name);
            }
        }
        out.push_str(": baseline='");
        out.push_str(mismatch.baseline);
        out.push_str("', treatment='");
        out.push_str(mismatch.treatment);
        out.push_str("'\n");
    }

    Some(out)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditEchoPolicy {
    PresentSeparately,
    IncludeInFileInteraction,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FileInteractionTotals {
    pub total_tokens: u64,
    pub file_interaction_tokens: u64,
    pub file_interaction_share: f64,
    pub edit_echo_tokens: u64,
    pub edit_echo_presented_separately: bool,
}

pub fn file_interaction_totals(
    events: &[QualityEvent<'_>],
    policy: EditEchoPolicy,
) -> FileInteractionTotals {
    let mut totals = FileInteractionTotals {
        edit_echo_presented_separately: policy == EditEchoPolicy::PresentSeparately,
        ..FileInteractionTotals::default()
    };

    for event in events {
        let event_tokens = event.tokens.total();
        totals.total_tokens = totals.total_tokens.saturating_add(event_tokens);

        if event.operation_class == EDIT_ECHO_CLASS {
            totals.edit_echo_tokens = totals.edit_echo_tokens.saturating_add(event_tokens);
            if policy == EditEchoPolicy::IncludeInFileInteraction {
                totals.file_interaction_tokens =
                    totals.file_interaction_tokens.saturating_add(event_tokens);
            }
        } else if is_file_interaction_class(event.operation_class) {
            totals.file_interaction_tokens =
                totals.file_interaction_tokens.saturating_add(event_tokens);
        }
    }

    totals.file_interaction_share =
        ratio_or_zero(totals.file_interaction_tokens, totals.total_tokens);
    totals
}

fn is_file_interaction_class(operation_class: &str) -> bool {
    operation_class.starts_with("file.") || operation_class.starts_with("vc.")
}

fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

fn ratio_or_zero(numerator: u64, denominator: u64) -> f64 {
    ratio(numerator, denominator).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        operation_class: &str,
        tokens: TokenBuckets,
        source: UsageSource,
        unattributed: bool,
    ) -> QualityEvent<'_> {
        QualityEvent {
            operation_class,
            tokens,
            source,
            unattributed,
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
    fn proxy_backed_cache_economics_reports_cache_read_ratio() {
        let events = [
            event(
                "file.read",
                TokenBuckets::new(100, 20, 300, 10),
                UsageSource::Proxy,
                false,
            ),
            event(
                "vc.diff",
                TokenBuckets::new(50, 5, 150, 0),
                UsageSource::Proxy,
                false,
            ),
            event(
                "file.search",
                TokenBuckets::new(999, 0, 999, 0),
                UsageSource::Hook,
                false,
            ),
        ];

        let economics = cache_economics(&events).expect("proxy usage should produce economics");

        assert_eq!(economics.fresh_input_tokens, 150);
        assert_eq!(economics.cache_read_tokens, 450);
        assert_eq!(economics.cache_write_tokens, 10);
        assert_eq!(economics.cache_read_to_fresh_input_ratio, Some(3.0));
    }

    #[test]
    fn hook_calibration_and_coverage_indicators_summarize_factor_and_unattributed_bucket() {
        let events = [
            event(
                "file.read",
                TokenBuckets::new(80, 0, 0, 0),
                UsageSource::Hook,
                false,
            ),
            event(
                OTHER_CLASS,
                TokenBuckets::new(20, 0, 0, 0),
                UsageSource::Hook,
                false,
            ),
            event(
                "vc.diff",
                TokenBuckets::new(200, 0, 0, 0),
                UsageSource::Proxy,
                true,
            ),
        ];
        let samples = [
            CalibrationSample {
                estimated_tokens: 100,
                reported_tokens: 125,
            },
            CalibrationSample {
                estimated_tokens: 300,
                reported_tokens: 375,
            },
        ];

        let indicators = hook_quality_indicators(&events, &samples);

        assert_eq!(indicators.calibration.sample_count, 2);
        assert_eq!(indicators.calibration.estimated_tokens, 400);
        assert_eq!(indicators.calibration.reported_tokens, 500);
        assert_eq!(indicators.calibration.factor, Some(1.25));
        assert_eq!(indicators.unattributed.event_count, 1);
        assert_eq!(indicators.unattributed.token_count, 20);
        assert_eq!(indicators.unattributed.total_tokens, 100);
        assert_float_eq(indicators.unattributed.share, 0.2);
    }

    #[test]
    fn mismatch_warning_output_is_prominent_and_specific() {
        let warning = render_mismatch_warning(&[
            Mismatch {
                kind: MismatchKind::Model,
                name: "model",
                baseline: "gpt-5",
                treatment: "gpt-5-mini",
            },
            Mismatch {
                kind: MismatchKind::Setting,
                name: "reasoning_effort",
                baseline: "low",
                treatment: "high",
            },
        ])
        .expect("mismatches should render a warning");

        assert_eq!(
            warning,
            "WARNING: comparison metadata mismatch\n\
Token comparisons may not be apples-to-apples.\n\
- model: baseline='gpt-5', treatment='gpt-5-mini'\n\
- setting reasoning_effort: baseline='low', treatment='high'\n"
        );
        assert_eq!(render_mismatch_warning(&[]), None);
    }

    #[test]
    fn edit_echo_policy_controls_headline_file_interaction_totals() {
        let events = [
            event(
                "file.read",
                TokenBuckets::new(100, 0, 0, 0),
                UsageSource::Hook,
                false,
            ),
            event(
                "vc.diff",
                TokenBuckets::new(40, 0, 0, 0),
                UsageSource::Hook,
                false,
            ),
            event(
                EDIT_ECHO_CLASS,
                TokenBuckets::new(60, 0, 0, 0),
                UsageSource::Hook,
                false,
            ),
            event(
                "test.output",
                TokenBuckets::new(100, 0, 0, 0),
                UsageSource::Hook,
                false,
            ),
        ];

        let separate = file_interaction_totals(&events, EditEchoPolicy::PresentSeparately);
        let included = file_interaction_totals(&events, EditEchoPolicy::IncludeInFileInteraction);

        assert_eq!(separate.total_tokens, 300);
        assert_eq!(separate.file_interaction_tokens, 140);
        assert_eq!(separate.edit_echo_tokens, 60);
        assert!(separate.edit_echo_presented_separately);
        assert_float_eq(separate.file_interaction_share, 140.0 / 300.0);

        assert_eq!(included.total_tokens, 300);
        assert_eq!(included.file_interaction_tokens, 200);
        assert_eq!(included.edit_echo_tokens, 60);
        assert!(!included.edit_echo_presented_separately);
        assert_float_eq(included.file_interaction_share, 200.0 / 300.0);
    }
}
