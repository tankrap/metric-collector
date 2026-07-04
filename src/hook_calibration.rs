pub const UNATTRIBUTED_DELTA_CLASS: &str = "unattributed.delta";
pub const HOOK_ESTIMATE_SOURCE_LABEL: &str = "hook-estimated tool results";
pub const REPORTED_USAGE_SOURCE_LABEL: &str = "api/session reported usage";
pub const UNCALIBRATED_METHOD_LABEL: &str = "uncalibrated hook estimate";
pub const FACTOR_METHOD_LABEL: &str = "reported / estimated calibration factor";
pub const DELTA_BUCKET_LABEL: &str = "unattributed reported-vs-estimated delta";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl TokenTotals {
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

    pub fn total_tokens(self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }

    fn add_assign(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookTokenEstimate<'a> {
    pub operation_class: &'a str,
    pub tokens: TokenTotals,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportedUsage {
    pub tokens: TokenTotals,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalibrationStatus {
    ExactMatch,
    ProportionalFactor,
    HookUnderEstimate,
    HookOverEstimate,
    MixedDelta,
    MissingReportedTotals,
    NoHookEstimate,
}

impl CalibrationStatus {
    pub const fn label(self) -> &'static str {
        match self {
            CalibrationStatus::ExactMatch => "exact match",
            CalibrationStatus::ProportionalFactor => "proportional factor",
            CalibrationStatus::HookUnderEstimate => "hook under-estimate",
            CalibrationStatus::HookOverEstimate => "hook over-estimate",
            CalibrationStatus::MixedDelta => "mixed reported-vs-estimated delta",
            CalibrationStatus::MissingReportedTotals => "missing reported totals",
            CalibrationStatus::NoHookEstimate => "no hook estimate",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalibrationFactors {
    pub input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
    pub cache_read_tokens: Option<f64>,
    pub cache_write_tokens: Option<f64>,
    pub total_tokens: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SignedTokenDelta {
    pub input_tokens: i128,
    pub output_tokens: i128,
    pub cache_read_tokens: i128,
    pub cache_write_tokens: i128,
}

impl SignedTokenDelta {
    pub fn total_tokens(self) -> i128 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }

    pub fn absolute_total_tokens(self) -> u128 {
        self.input_tokens.unsigned_abs()
            + self.output_tokens.unsigned_abs()
            + self.cache_read_tokens.unsigned_abs()
            + self.cache_write_tokens.unsigned_abs()
    }

    pub const fn is_zero(self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_write_tokens == 0
    }

    fn has_positive_and_negative_components(self) -> bool {
        let values = [
            self.input_tokens,
            self.output_tokens,
            self.cache_read_tokens,
            self.cache_write_tokens,
        ];
        values.iter().any(|value| *value > 0) && values.iter().any(|value| *value < 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnattributedDeltaBucket {
    pub operation_class: &'static str,
    pub label: &'static str,
    pub delta: SignedTokenDelta,
    pub absolute_delta_tokens: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationMetadata {
    pub estimate_source_label: &'static str,
    pub reported_source_label: Option<&'static str>,
    pub method_label: &'static str,
    pub factor_label: Option<&'static str>,
    pub delta_bucket_label: Option<&'static str>,
    pub status_label: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HookCalibration {
    pub estimated_totals: TokenTotals,
    pub reported_totals: Option<TokenTotals>,
    pub factors: CalibrationFactors,
    pub unattributed_delta: Option<UnattributedDeltaBucket>,
    pub status: CalibrationStatus,
    pub metadata: CalibrationMetadata,
}

pub fn reconcile_hook_estimates(
    estimates: &[HookTokenEstimate<'_>],
    reported_usage: Option<ReportedUsage>,
) -> HookCalibration {
    let estimated_totals = sum_hook_estimates(estimates);

    let Some(reported_usage) = reported_usage else {
        return HookCalibration {
            estimated_totals,
            reported_totals: None,
            factors: CalibrationFactors::default(),
            unattributed_delta: None,
            status: CalibrationStatus::MissingReportedTotals,
            metadata: metadata(
                CalibrationStatus::MissingReportedTotals,
                false,
                false,
                false,
            ),
        };
    };

    let reported_totals = reported_usage.tokens;
    let delta = token_delta(reported_totals, estimated_totals);
    let factors = calibration_factors(reported_totals, estimated_totals);
    let unattributed_delta = (!delta.is_zero()).then_some(UnattributedDeltaBucket {
        operation_class: UNATTRIBUTED_DELTA_CLASS,
        label: DELTA_BUCKET_LABEL,
        delta,
        absolute_delta_tokens: delta.absolute_total_tokens(),
    });
    let status = calibration_status(estimated_totals, reported_totals, delta, &factors);

    HookCalibration {
        estimated_totals,
        reported_totals: Some(reported_totals),
        factors,
        unattributed_delta,
        status,
        metadata: metadata(
            status,
            true,
            factors.total_tokens.is_some(),
            !delta.is_zero(),
        ),
    }
}

pub fn sum_hook_estimates(estimates: &[HookTokenEstimate<'_>]) -> TokenTotals {
    let mut totals = TokenTotals::default();

    for estimate in estimates {
        totals.add_assign(estimate.tokens);
    }

    totals
}

pub fn calibration_factors(
    reported_totals: TokenTotals,
    estimated_totals: TokenTotals,
) -> CalibrationFactors {
    CalibrationFactors {
        input_tokens: calibration_factor(
            reported_totals.input_tokens,
            estimated_totals.input_tokens,
        ),
        output_tokens: calibration_factor(
            reported_totals.output_tokens,
            estimated_totals.output_tokens,
        ),
        cache_read_tokens: calibration_factor(
            reported_totals.cache_read_tokens,
            estimated_totals.cache_read_tokens,
        ),
        cache_write_tokens: calibration_factor(
            reported_totals.cache_write_tokens,
            estimated_totals.cache_write_tokens,
        ),
        total_tokens: calibration_factor(
            reported_totals.total_tokens(),
            estimated_totals.total_tokens(),
        ),
    }
}

pub fn token_delta(
    reported_totals: TokenTotals,
    estimated_totals: TokenTotals,
) -> SignedTokenDelta {
    SignedTokenDelta {
        input_tokens: signed_delta(reported_totals.input_tokens, estimated_totals.input_tokens),
        output_tokens: signed_delta(
            reported_totals.output_tokens,
            estimated_totals.output_tokens,
        ),
        cache_read_tokens: signed_delta(
            reported_totals.cache_read_tokens,
            estimated_totals.cache_read_tokens,
        ),
        cache_write_tokens: signed_delta(
            reported_totals.cache_write_tokens,
            estimated_totals.cache_write_tokens,
        ),
    }
}

fn calibration_factor(reported: u64, estimated: u64) -> Option<f64> {
    match (reported, estimated) {
        (0, 0) => Some(1.0),
        (_, 0) => None,
        _ => Some(reported as f64 / estimated as f64),
    }
}

fn calibration_status(
    estimated_totals: TokenTotals,
    reported_totals: TokenTotals,
    delta: SignedTokenDelta,
    factors: &CalibrationFactors,
) -> CalibrationStatus {
    if estimated_totals.total_tokens() == 0 {
        return CalibrationStatus::NoHookEstimate;
    }

    if delta.is_zero() {
        return CalibrationStatus::ExactMatch;
    }

    if delta.has_positive_and_negative_components() {
        return CalibrationStatus::MixedDelta;
    }

    if is_single_proportional_factor(factors) {
        return CalibrationStatus::ProportionalFactor;
    }

    if reported_totals.total_tokens() > estimated_totals.total_tokens() {
        CalibrationStatus::HookUnderEstimate
    } else {
        CalibrationStatus::HookOverEstimate
    }
}

fn is_single_proportional_factor(factors: &CalibrationFactors) -> bool {
    let Some(total_factor) = factors.total_tokens else {
        return false;
    };

    if (total_factor - 1.0).abs() < f64::EPSILON {
        return false;
    }

    IntoIterator::into_iter([
        factors.input_tokens,
        factors.output_tokens,
        factors.cache_read_tokens,
        factors.cache_write_tokens,
    ])
    .flatten()
    .all(|factor| (factor - total_factor).abs() < f64::EPSILON)
}

fn signed_delta(reported: u64, estimated: u64) -> i128 {
    i128::from(reported) - i128::from(estimated)
}

fn metadata(
    status: CalibrationStatus,
    has_reported_usage: bool,
    has_factor: bool,
    has_delta_bucket: bool,
) -> CalibrationMetadata {
    CalibrationMetadata {
        estimate_source_label: HOOK_ESTIMATE_SOURCE_LABEL,
        reported_source_label: has_reported_usage.then_some(REPORTED_USAGE_SOURCE_LABEL),
        method_label: if has_reported_usage {
            FACTOR_METHOD_LABEL
        } else {
            UNCALIBRATED_METHOD_LABEL
        },
        factor_label: has_factor.then_some(FACTOR_METHOD_LABEL),
        delta_bucket_label: has_delta_bucket.then_some(DELTA_BUCKET_LABEL),
        status_label: status.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate(operation_class: &'static str, tokens: TokenTotals) -> HookTokenEstimate<'static> {
        HookTokenEstimate {
            operation_class,
            tokens,
        }
    }

    fn reported(tokens: TokenTotals) -> ReportedUsage {
        ReportedUsage { tokens }
    }

    fn assert_float_eq(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("expected calibration factor");
        assert!(
            (actual - expected).abs() < f64::EPSILON,
            "expected {}, got {}",
            expected,
            actual
        );
    }

    #[test]
    fn exact_match_has_factor_without_delta_bucket() {
        let calibration = reconcile_hook_estimates(
            &[
                estimate("file.read", TokenTotals::new(80, 10, 5, 0)),
                estimate("vc.diff", TokenTotals::new(20, 10, 5, 0)),
            ],
            Some(reported(TokenTotals::new(100, 20, 10, 0))),
        );

        assert_eq!(calibration.status, CalibrationStatus::ExactMatch);
        assert_eq!(
            calibration.estimated_totals,
            TokenTotals::new(100, 20, 10, 0)
        );
        assert_eq!(
            calibration.reported_totals,
            Some(TokenTotals::new(100, 20, 10, 0))
        );
        assert_float_eq(calibration.factors.total_tokens, 1.0);
        assert_eq!(calibration.unattributed_delta, None);
    }

    #[test]
    fn proportional_factor_is_computed_but_not_spread_across_events() {
        let calibration = reconcile_hook_estimates(
            &[estimate("file.read", TokenTotals::new(100, 50, 20, 10))],
            Some(reported(TokenTotals::new(150, 75, 30, 15))),
        );

        assert_eq!(calibration.status, CalibrationStatus::ProportionalFactor);
        assert_eq!(
            calibration.estimated_totals,
            TokenTotals::new(100, 50, 20, 10)
        );
        assert_float_eq(calibration.factors.input_tokens, 1.5);
        assert_float_eq(calibration.factors.total_tokens, 1.5);
        assert_eq!(
            calibration.unattributed_delta,
            Some(UnattributedDeltaBucket {
                operation_class: UNATTRIBUTED_DELTA_CLASS,
                label: DELTA_BUCKET_LABEL,
                delta: SignedTokenDelta {
                    input_tokens: 50,
                    output_tokens: 25,
                    cache_read_tokens: 10,
                    cache_write_tokens: 5,
                },
                absolute_delta_tokens: 90,
            })
        );
    }

    #[test]
    fn under_estimate_keeps_positive_unattributed_delta() {
        let calibration = reconcile_hook_estimates(
            &[estimate("test.output", TokenTotals::new(80, 10, 0, 0))],
            Some(reported(TokenTotals::new(100, 20, 0, 0))),
        );

        assert_eq!(calibration.status, CalibrationStatus::HookUnderEstimate);
        assert_eq!(
            calibration.unattributed_delta.map(|bucket| bucket.delta),
            Some(SignedTokenDelta {
                input_tokens: 20,
                output_tokens: 10,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            })
        );
    }

    #[test]
    fn over_estimate_keeps_negative_unattributed_delta() {
        let calibration = reconcile_hook_estimates(
            &[estimate("build.output", TokenTotals::new(120, 60, 0, 0))],
            Some(reported(TokenTotals::new(90, 40, 0, 0))),
        );

        assert_eq!(calibration.status, CalibrationStatus::HookOverEstimate);
        assert_eq!(
            calibration.unattributed_delta.map(|bucket| bucket.delta),
            Some(SignedTokenDelta {
                input_tokens: -30,
                output_tokens: -20,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            })
        );
        assert_eq!(
            calibration
                .unattributed_delta
                .map(|bucket| bucket.absolute_delta_tokens),
            Some(50)
        );
    }

    #[test]
    fn missing_reported_totals_returns_uncalibrated_metadata() {
        let calibration = reconcile_hook_estimates(
            &[estimate("file.search", TokenTotals::new(10, 5, 0, 0))],
            None,
        );

        assert_eq!(calibration.status, CalibrationStatus::MissingReportedTotals);
        assert_eq!(calibration.reported_totals, None);
        assert_eq!(calibration.factors, CalibrationFactors::default());
        assert_eq!(calibration.unattributed_delta, None);
        assert_eq!(calibration.metadata.method_label, UNCALIBRATED_METHOD_LABEL);
        assert_eq!(calibration.metadata.reported_source_label, None);
    }

    #[test]
    fn metadata_labels_are_report_ready() {
        let calibration = reconcile_hook_estimates(
            &[estimate("file.read", TokenTotals::new(10, 0, 0, 0))],
            Some(reported(TokenTotals::new(15, 0, 0, 0))),
        );

        assert_eq!(
            calibration.metadata.estimate_source_label,
            HOOK_ESTIMATE_SOURCE_LABEL
        );
        assert_eq!(
            calibration.metadata.reported_source_label,
            Some(REPORTED_USAGE_SOURCE_LABEL)
        );
        assert_eq!(calibration.metadata.factor_label, Some(FACTOR_METHOD_LABEL));
        assert_eq!(
            calibration.metadata.delta_bucket_label,
            Some(DELTA_BUCKET_LABEL)
        );
        assert_eq!(
            calibration.metadata.status_label,
            CalibrationStatus::HookUnderEstimate.label()
        );
    }
}
