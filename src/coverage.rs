use std::collections::{BTreeMap, BTreeSet};

pub const OTHER_CLASS: &str = "other";

pub type Adapter = String;
pub type OperationClass = String;
pub type Profile = String;
pub type RulesVersion = String;
pub type RunId = String;
pub type TaskId = String;
pub type TokenCount = u64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageEvent {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
    pub operation_class: OperationClass,
    pub generated: bool,
    pub context_tokens: TokenCount,
    pub adapter: Adapter,
    pub rules_version: RulesVersion,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClassCoverage {
    pub events: usize,
    pub context_tokens: TokenCount,
    pub generated_context_tokens: TokenCount,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CoverageKey {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverageScopeSummary {
    pub total_context_tokens: TokenCount,
    pub other_context_tokens: TokenCount,
    pub other_share: f64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoverageMetadata {
    pub adapters: Vec<Adapter>,
    pub rules_versions: Vec<RulesVersion>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverageSummary {
    pub class_totals: BTreeMap<OperationClass, ClassCoverage>,
    pub scopes: BTreeMap<CoverageKey, CoverageScopeSummary>,
    pub total_context_tokens: TokenCount,
    pub other_context_tokens: TokenCount,
    pub other_share: f64,
    pub generated_context_tokens: TokenCount,
    pub generated_file_share: f64,
    pub metadata: CoverageMetadata,
}

pub fn summarize_coverage(events: &[CoverageEvent]) -> CoverageSummary {
    let mut summary = CoverageSummary::default();
    let mut adapters = BTreeSet::new();
    let mut rules_versions = BTreeSet::new();

    for event in events {
        let class_total = summary
            .class_totals
            .entry(event.operation_class.clone())
            .or_default();
        class_total.events += 1;
        class_total.context_tokens += event.context_tokens;

        if event.generated {
            class_total.generated_context_tokens += event.context_tokens;
            summary.generated_context_tokens += event.context_tokens;
        }

        let is_other = event.operation_class == OTHER_CLASS;
        if is_other {
            summary.other_context_tokens += event.context_tokens;
        }

        let scope = summary
            .scopes
            .entry(CoverageKey {
                run_id: event.run_id.clone(),
                task_id: event.task_id.clone(),
                profile: event.profile.clone(),
            })
            .or_default();
        scope.total_context_tokens += event.context_tokens;
        if is_other {
            scope.other_context_tokens += event.context_tokens;
        }

        summary.total_context_tokens += event.context_tokens;
        adapters.insert(event.adapter.clone());
        rules_versions.insert(event.rules_version.clone());
    }

    summary.other_share = share(summary.other_context_tokens, summary.total_context_tokens);
    summary.generated_file_share = share(
        summary.generated_context_tokens,
        summary.total_context_tokens,
    );

    for scope in summary.scopes.values_mut() {
        scope.other_share = share(scope.other_context_tokens, scope.total_context_tokens);
    }

    summary.metadata = CoverageMetadata {
        adapters: adapters.into_iter().collect(),
        rules_versions: rules_versions.into_iter().collect(),
    };

    summary
}

fn share(part: TokenCount, total: TokenCount) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        run_id: &str,
        task_id: &str,
        profile: &str,
        operation_class: &str,
        context_tokens: TokenCount,
        generated: bool,
        adapter: &str,
        rules_version: &str,
    ) -> CoverageEvent {
        CoverageEvent {
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            profile: profile.to_string(),
            operation_class: operation_class.to_string(),
            generated,
            context_tokens,
            adapter: adapter.to_string(),
            rules_version: rules_version.to_string(),
        }
    }

    fn scope_key(run_id: &str, task_id: &str, profile: &str) -> CoverageKey {
        CoverageKey {
            run_id: run_id.to_string(),
            task_id: task_id.to_string(),
            profile: profile.to_string(),
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
    fn aggregates_class_totals() {
        let summary = summarize_coverage(&[
            event(
                "run-1",
                "task-1",
                "baseline",
                "file_read",
                100,
                false,
                "claude-code",
                "rules-1",
            ),
            event(
                "run-1",
                "task-1",
                "baseline",
                "file_read",
                25,
                true,
                "claude-code",
                "rules-1",
            ),
            event(
                "run-1",
                "task-1",
                "baseline",
                "git_status",
                10,
                false,
                "claude-code",
                "rules-1",
            ),
        ]);

        assert_eq!(summary.total_context_tokens, 135);
        assert_eq!(
            summary.class_totals.get("file_read"),
            Some(&ClassCoverage {
                events: 2,
                context_tokens: 125,
                generated_context_tokens: 25
            })
        );
        assert_eq!(
            summary.class_totals.get("git_status"),
            Some(&ClassCoverage {
                events: 1,
                context_tokens: 10,
                generated_context_tokens: 0
            })
        );
    }

    #[test]
    fn computes_other_share_globally_and_by_scope() {
        let summary = summarize_coverage(&[
            event(
                "run-1",
                "task-1",
                "baseline",
                OTHER_CLASS,
                40,
                false,
                "claude-code",
                "rules-1",
            ),
            event(
                "run-1",
                "task-1",
                "baseline",
                "file_read",
                60,
                false,
                "claude-code",
                "rules-1",
            ),
            event(
                "run-1",
                "task-2",
                "baseline",
                OTHER_CLASS,
                10,
                false,
                "claude-code",
                "rules-1",
            ),
            event(
                "run-1",
                "task-2",
                "baseline",
                "git_diff",
                90,
                false,
                "claude-code",
                "rules-1",
            ),
        ]);

        assert_eq!(summary.other_context_tokens, 50);
        assert_float_eq(summary.other_share, 0.25);
        assert_eq!(
            summary
                .scopes
                .get(&scope_key("run-1", "task-1", "baseline")),
            Some(&CoverageScopeSummary {
                total_context_tokens: 100,
                other_context_tokens: 40,
                other_share: 0.4
            })
        );
        assert_eq!(
            summary
                .scopes
                .get(&scope_key("run-1", "task-2", "baseline")),
            Some(&CoverageScopeSummary {
                total_context_tokens: 100,
                other_context_tokens: 10,
                other_share: 0.1
            })
        );
    }

    #[test]
    fn computes_generated_file_share_as_distinct_metric() {
        let summary = summarize_coverage(&[
            event(
                "run-1",
                "task-1",
                "baseline",
                "file_read",
                30,
                true,
                "claude-code",
                "rules-1",
            ),
            event(
                "run-1",
                "task-1",
                "baseline",
                "git_diff",
                70,
                false,
                "claude-code",
                "rules-1",
            ),
        ]);

        assert_eq!(summary.generated_context_tokens, 30);
        assert_float_eq(summary.generated_file_share, 0.3);
        assert_eq!(summary.other_context_tokens, 0);
        assert_float_eq(summary.other_share, 0.0);
    }

    #[test]
    fn collects_adapter_and_rules_metadata_once_in_sorted_order() {
        let summary = summarize_coverage(&[
            event(
                "run-1",
                "task-1",
                "baseline",
                "file_read",
                1,
                false,
                "z-adapter",
                "rules-2",
            ),
            event(
                "run-2",
                "task-2",
                "treatment",
                "git_status",
                1,
                false,
                "a-adapter",
                "rules-1",
            ),
            event(
                "run-3",
                "task-3",
                "treatment",
                "git_diff",
                1,
                false,
                "z-adapter",
                "rules-2",
            ),
        ]);

        assert_eq!(
            summary.metadata,
            CoverageMetadata {
                adapters: vec!["a-adapter".to_string(), "z-adapter".to_string()],
                rules_versions: vec!["rules-1".to_string(), "rules-2".to_string()],
            }
        );
    }

    #[test]
    fn empty_input_has_zero_shares_and_empty_metadata() {
        let summary = summarize_coverage(&[]);

        assert!(summary.class_totals.is_empty());
        assert!(summary.scopes.is_empty());
        assert_eq!(summary.total_context_tokens, 0);
        assert_float_eq(summary.other_share, 0.0);
        assert_float_eq(summary.generated_file_share, 0.0);
        assert_eq!(summary.metadata, CoverageMetadata::default());
    }
}
