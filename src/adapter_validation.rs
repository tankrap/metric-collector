#[cfg(adapter_validation_standalone)]
#[path = "core.rs"]
mod core;
#[cfg(adapter_validation_standalone)]
#[path = "hook_calibration.rs"]
mod hook_calibration;

use std::error::Error;
use std::fmt;

#[cfg(adapter_validation_standalone)]
use self::core::OperationClass;
#[cfg(adapter_validation_standalone)]
use self::hook_calibration::{
    reconcile_hook_estimates, HookTokenEstimate, ReportedUsage, TokenTotals,
};
#[cfg(not(adapter_validation_standalone))]
use crate::core::OperationClass;
#[cfg(not(adapter_validation_standalone))]
use crate::hook_calibration::{
    reconcile_hook_estimates, HookTokenEstimate, ReportedUsage, TokenTotals,
};

pub const CALIBRATED_ACCURACY_TOLERANCE: f64 = 0.10;
pub const VALIDATION_SOURCE: &str = "offline static adapter validation fixtures";

const DEFAULT_FIXTURES: [AdapterValidationFixture; 5] = [
    AdapterValidationFixture::new(
        "git status porcelain",
        OperationClass::VcStatus,
        TokenTotals::new(72, 18, 0, 0),
        TokenTotals::new(80, 20, 0, 0),
    ),
    AdapterValidationFixture::new(
        "git diff compact patch",
        OperationClass::VcDiff,
        TokenTotals::new(360, 90, 0, 0),
        TokenTotals::new(400, 100, 0, 0),
    ),
    AdapterValidationFixture::new(
        "read source file",
        OperationClass::FileRead,
        TokenTotals::new(648, 72, 0, 0),
        TokenTotals::new(720, 80, 0, 0),
    ),
    AdapterValidationFixture::new(
        "ripgrep search results",
        OperationClass::FileSearch,
        TokenTotals::new(252, 27, 0, 0),
        TokenTotals::new(280, 30, 0, 0),
    ),
    AdapterValidationFixture::new(
        "cargo test failure output",
        OperationClass::TestOutput,
        TokenTotals::new(810, 270, 0, 0),
        TokenTotals::new(900, 300, 0, 0),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterValidationFixture {
    pub name: &'static str,
    pub operation_class: OperationClass,
    pub hook_estimate: TokenTotals,
    pub proxy_ground_truth: TokenTotals,
}

impl AdapterValidationFixture {
    pub const fn new(
        name: &'static str,
        operation_class: OperationClass,
        hook_estimate: TokenTotals,
        proxy_ground_truth: TokenTotals,
    ) -> Self {
        Self {
            name,
            operation_class,
            hook_estimate,
            proxy_ground_truth,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProxyCalibration {
    pub factor: f64,
    pub hook_total_tokens: u64,
    pub proxy_total_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineDependencyPolicy {
    pub fixture_source: &'static str,
    pub uses_live_provider: bool,
    pub uses_network: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterValidationReport {
    pub calibration: ProxyCalibration,
    pub checked: Vec<FixtureAccuracy>,
    pub failures: Vec<AdapterValidationFailure>,
}

impl AdapterValidationReport {
    pub fn is_passing(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn failure_summary(&self) -> String {
        self.failures
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixtureAccuracy {
    pub fixture_name: &'static str,
    pub operation_class: OperationClass,
    pub hook_total_tokens: u64,
    pub proxy_total_tokens: u64,
    pub calibrated_total_tokens: u64,
    pub relative_error: f64,
}

impl FixtureAccuracy {
    pub fn within_tolerance(&self) -> bool {
        self.relative_error <= CALIBRATED_ACCURACY_TOLERANCE
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdapterValidationFailure {
    pub fixture_name: &'static str,
    pub operation_class: OperationClass,
    pub calibrated_total_tokens: u64,
    pub proxy_total_tokens: u64,
    pub relative_error: f64,
    pub tolerance: f64,
}

impl fmt::Display for AdapterValidationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "fixture '{}' ({}) calibrated total {} vs proxy ground truth {}: {:.2}% error exceeds {:.2}% tolerance",
            self.fixture_name,
            self.operation_class,
            self.calibrated_total_tokens,
            self.proxy_total_tokens,
            self.relative_error * 100.0,
            self.tolerance * 100.0
        )
    }
}

impl Error for AdapterValidationFailure {}

pub fn default_validation_fixtures() -> &'static [AdapterValidationFixture] {
    &DEFAULT_FIXTURES
}

pub const fn offline_dependency_policy() -> OfflineDependencyPolicy {
    OfflineDependencyPolicy {
        fixture_source: VALIDATION_SOURCE,
        uses_live_provider: false,
        uses_network: false,
    }
}

pub fn validate_default_fixtures() -> AdapterValidationReport {
    validate_fixtures(default_validation_fixtures())
}

pub fn validate_fixtures(fixtures: &[AdapterValidationFixture]) -> AdapterValidationReport {
    let calibration = derive_proxy_calibration(fixtures);
    validate_fixtures_with_calibration(fixtures, calibration)
}

pub fn validate_fixtures_with_calibration(
    fixtures: &[AdapterValidationFixture],
    calibration: ProxyCalibration,
) -> AdapterValidationReport {
    let mut checked = Vec::with_capacity(fixtures.len());
    let mut failures = Vec::new();

    for fixture in fixtures {
        let accuracy = fixture_accuracy(*fixture, calibration);

        if !accuracy.within_tolerance() {
            failures.push(AdapterValidationFailure {
                fixture_name: accuracy.fixture_name,
                operation_class: accuracy.operation_class,
                calibrated_total_tokens: accuracy.calibrated_total_tokens,
                proxy_total_tokens: accuracy.proxy_total_tokens,
                relative_error: accuracy.relative_error,
                tolerance: CALIBRATED_ACCURACY_TOLERANCE,
            });
        }

        checked.push(accuracy);
    }

    AdapterValidationReport {
        calibration,
        checked,
        failures,
    }
}

pub fn derive_proxy_calibration(fixtures: &[AdapterValidationFixture]) -> ProxyCalibration {
    let hook_estimates = fixtures
        .iter()
        .map(|fixture| HookTokenEstimate {
            operation_class: fixture.operation_class.as_str(),
            tokens: fixture.hook_estimate,
        })
        .collect::<Vec<_>>();
    let proxy_ground_truth = fixtures
        .iter()
        .fold(TokenTotals::default(), add_proxy_totals);
    let hook_total_tokens = hook_estimates
        .iter()
        .map(|estimate| estimate.tokens.total_tokens())
        .sum::<u64>();
    let proxy_total_tokens = proxy_ground_truth.total_tokens();
    let calibration = reconcile_hook_estimates(
        &hook_estimates,
        Some(ReportedUsage {
            tokens: proxy_ground_truth,
        }),
    );

    ProxyCalibration {
        factor: calibration.factors.total_tokens.unwrap_or(1.0),
        hook_total_tokens,
        proxy_total_tokens,
    }
}

fn fixture_accuracy(
    fixture: AdapterValidationFixture,
    calibration: ProxyCalibration,
) -> FixtureAccuracy {
    let hook_total_tokens = fixture.hook_estimate.total_tokens();
    let proxy_total_tokens = fixture.proxy_ground_truth.total_tokens();
    let calibrated_total_tokens = calibrated_total(hook_total_tokens, calibration.factor);
    let relative_error = relative_error(calibrated_total_tokens, proxy_total_tokens);

    FixtureAccuracy {
        fixture_name: fixture.name,
        operation_class: fixture.operation_class,
        hook_total_tokens,
        proxy_total_tokens,
        calibrated_total_tokens,
        relative_error,
    }
}

fn add_proxy_totals(mut totals: TokenTotals, fixture: &AdapterValidationFixture) -> TokenTotals {
    totals.input_tokens = totals
        .input_tokens
        .saturating_add(fixture.proxy_ground_truth.input_tokens);
    totals.output_tokens = totals
        .output_tokens
        .saturating_add(fixture.proxy_ground_truth.output_tokens);
    totals.cache_read_tokens = totals
        .cache_read_tokens
        .saturating_add(fixture.proxy_ground_truth.cache_read_tokens);
    totals.cache_write_tokens = totals
        .cache_write_tokens
        .saturating_add(fixture.proxy_ground_truth.cache_write_tokens);
    totals
}

fn calibrated_total(hook_total_tokens: u64, factor: f64) -> u64 {
    (hook_total_tokens as f64 * factor).round() as u64
}

fn relative_error(calibrated_total_tokens: u64, proxy_total_tokens: u64) -> f64 {
    if proxy_total_tokens == 0 {
        return if calibrated_total_tokens == 0 {
            0.0
        } else {
            f64::INFINITY
        };
    }

    let delta = calibrated_total_tokens.abs_diff(proxy_total_tokens);
    delta as f64 / proxy_total_tokens as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fixtures_pass_calibrated_accuracy_threshold() {
        let report = validate_default_fixtures();

        assert!(
            report.is_passing(),
            "expected passing offline adapter validation:\n{}",
            report.failure_summary()
        );
        assert_eq!(report.checked.len(), 5);
        assert_eq!(report.calibration.hook_total_tokens, 2619);
        assert_eq!(report.calibration.proxy_total_tokens, 2910);
    }

    #[test]
    fn failing_fixture_reports_name_and_operation_class() {
        let calibration = derive_proxy_calibration(default_validation_fixtures());
        let failing_fixture = AdapterValidationFixture::new(
            "under-counted test output regression",
            OperationClass::TestOutput,
            TokenTotals::new(100, 20, 0, 0),
            TokenTotals::new(900, 300, 0, 0),
        );

        let report = validate_fixtures_with_calibration(&[failing_fixture], calibration);

        assert!(!report.is_passing());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            report.failures[0].fixture_name,
            "under-counted test output regression"
        );
        assert_eq!(
            report.failures[0].operation_class,
            OperationClass::TestOutput
        );
        let diagnostic = report.failure_summary();
        assert!(diagnostic.contains("under-counted test output regression"));
        assert!(diagnostic.contains("test.output"));
    }

    #[test]
    fn operation_class_attribution_covers_required_representative_classes() {
        let classes = default_validation_fixtures()
            .iter()
            .map(|fixture| fixture.operation_class)
            .collect::<Vec<_>>();

        assert!(classes.contains(&OperationClass::VcStatus));
        assert!(classes.contains(&OperationClass::VcDiff));
        assert!(classes.contains(&OperationClass::FileRead));
        assert!(classes.contains(&OperationClass::FileSearch));
        assert!(classes.contains(&OperationClass::TestOutput));
    }

    #[test]
    fn validation_declares_no_live_provider_or_network_dependency() {
        let policy = offline_dependency_policy();

        assert_eq!(policy.fixture_source, VALIDATION_SOURCE);
        assert!(!policy.uses_live_provider);
        assert!(!policy.uses_network);
    }
}
