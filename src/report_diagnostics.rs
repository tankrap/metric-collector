use std::cmp::Ordering;
use std::collections::BTreeMap;

pub type EventClass = String;
pub type Digest = String;
pub type Profile = String;
pub type RunId = String;
pub type TaskId = String;

pub const DEFAULT_TOP_N: usize = 20;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticEvent {
    pub class: EventClass,
    pub digest: Digest,
    pub tokens: u64,
    pub bytes: u64,
    pub repeat_of: Option<Digest>,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
    pub generated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepeatedPayloadGroup {
    pub digest: Digest,
    pub tokens: u64,
    pub bytes: u64,
    pub event_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpensiveEvent {
    pub class: EventClass,
    pub digest: Digest,
    pub tokens: u64,
    pub bytes: u64,
    pub repeat_of: Option<Digest>,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: Profile,
    pub generated: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReportDiagnostics {
    pub reread_waste_tokens: u64,
    pub repeated_payload_groups: Vec<RepeatedPayloadGroup>,
    pub expensive_events: Vec<ExpensiveEvent>,
}

pub fn report_diagnostics(events: &[DiagnosticEvent], top_n: usize) -> ReportDiagnostics {
    let mut reread_waste_tokens = 0u64;
    let mut repeated_payload_groups = BTreeMap::<Digest, RepeatedPayloadGroup>::new();

    for event in events {
        if let Some(repeat_digest) = &event.repeat_of {
            reread_waste_tokens = reread_waste_tokens.saturating_add(event.tokens);

            let group = repeated_payload_groups
                .entry(repeat_digest.clone())
                .or_insert_with(|| RepeatedPayloadGroup {
                    digest: repeat_digest.clone(),
                    tokens: 0,
                    bytes: 0,
                    event_count: 0,
                });
            group.tokens = group.tokens.saturating_add(event.tokens);
            group.bytes = group.bytes.saturating_add(event.bytes);
            group.event_count = group.event_count.saturating_add(1);
        }
    }

    let mut repeated_payload_groups = repeated_payload_groups
        .into_values()
        .collect::<Vec<RepeatedPayloadGroup>>();
    repeated_payload_groups.sort_by(compare_repeated_payload_groups);
    repeated_payload_groups.truncate(top_n);

    let mut expensive_events = events
        .iter()
        .map(|event| ExpensiveEvent {
            class: event.class.clone(),
            digest: event.digest.clone(),
            tokens: event.tokens,
            bytes: event.bytes,
            repeat_of: event.repeat_of.clone(),
            run_id: event.run_id.clone(),
            task_id: event.task_id.clone(),
            profile: event.profile.clone(),
            generated: event.generated,
        })
        .collect::<Vec<ExpensiveEvent>>();
    expensive_events.sort_by(compare_expensive_events);
    expensive_events.truncate(top_n);

    ReportDiagnostics {
        reread_waste_tokens,
        repeated_payload_groups,
        expensive_events,
    }
}

pub fn report_diagnostics_default(events: &[DiagnosticEvent]) -> ReportDiagnostics {
    report_diagnostics(events, DEFAULT_TOP_N)
}

fn compare_repeated_payload_groups(
    left: &RepeatedPayloadGroup,
    right: &RepeatedPayloadGroup,
) -> Ordering {
    right
        .tokens
        .cmp(&left.tokens)
        .then_with(|| right.bytes.cmp(&left.bytes))
        .then_with(|| left.digest.cmp(&right.digest))
}

fn compare_expensive_events(left: &ExpensiveEvent, right: &ExpensiveEvent) -> Ordering {
    right
        .tokens
        .cmp(&left.tokens)
        .then_with(|| right.bytes.cmp(&left.bytes))
        .then_with(|| left.digest.cmp(&right.digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        class: &str,
        digest: &str,
        tokens: u64,
        bytes: u64,
        repeat_of: Option<&str>,
    ) -> DiagnosticEvent {
        DiagnosticEvent {
            class: class.to_owned(),
            digest: digest.to_owned(),
            tokens,
            bytes,
            repeat_of: repeat_of.map(str::to_owned),
            run_id: "run-1".to_owned(),
            task_id: "task-1".to_owned(),
            profile: "default".to_owned(),
            generated: false,
        }
    }

    #[test]
    fn totals_reread_waste_tokens_for_repeated_events_only() {
        let diagnostics = report_diagnostics(
            &[
                event("file.read", "digest-a", 100, 500, None),
                event("file.read", "digest-b", 40, 200, Some("digest-a")),
                event("file.read", "digest-c", 60, 300, Some("digest-a")),
                event("vc.diff", "digest-d", 80, 400, None),
            ],
            10,
        );

        assert_eq!(diagnostics.reread_waste_tokens, 100);
    }

    #[test]
    fn orders_repeated_payload_groups_by_cost_then_digest() {
        let diagnostics = report_diagnostics(
            &[
                event("file.read", "repeat-a-1", 25, 150, Some("digest-a")),
                event("file.read", "repeat-b-1", 50, 100, Some("digest-b")),
                event("file.read", "repeat-c-1", 50, 200, Some("digest-c")),
                event("file.read", "repeat-a-2", 25, 50, Some("digest-a")),
                event("file.read", "repeat-d-1", 50, 200, Some("digest-d")),
            ],
            10,
        );

        assert_eq!(
            diagnostics.repeated_payload_groups,
            vec![
                RepeatedPayloadGroup {
                    digest: "digest-a".to_owned(),
                    tokens: 50,
                    bytes: 200,
                    event_count: 2,
                },
                RepeatedPayloadGroup {
                    digest: "digest-c".to_owned(),
                    tokens: 50,
                    bytes: 200,
                    event_count: 1,
                },
                RepeatedPayloadGroup {
                    digest: "digest-d".to_owned(),
                    tokens: 50,
                    bytes: 200,
                    event_count: 1,
                },
                RepeatedPayloadGroup {
                    digest: "digest-b".to_owned(),
                    tokens: 50,
                    bytes: 100,
                    event_count: 1,
                },
            ]
        );
    }

    #[test]
    fn orders_expensive_events_by_tokens_bytes_then_digest() {
        let diagnostics = report_diagnostics(
            &[
                event("file.read", "digest-c", 100, 100, None),
                event("file.read", "digest-b", 200, 100, None),
                event("file.read", "digest-a", 200, 500, None),
                event("file.read", "digest-d", 200, 500, None),
            ],
            10,
        );

        assert_eq!(
            diagnostics
                .expensive_events
                .iter()
                .map(|event| event.digest.as_str())
                .collect::<Vec<&str>>(),
            vec!["digest-a", "digest-d", "digest-b", "digest-c"]
        );
    }

    #[test]
    fn diagnostics_use_digests_without_content_or_path_fields() {
        let diagnostics = report_diagnostics(
            &[DiagnosticEvent {
                class: "file.read".to_owned(),
                digest: "fnv1a64:abc123".to_owned(),
                tokens: 10,
                bytes: 20,
                repeat_of: Some("fnv1a64:first".to_owned()),
                run_id: "run-1".to_owned(),
                task_id: "task-1".to_owned(),
                profile: "default".to_owned(),
                generated: true,
            }],
            10,
        );

        let rendered = format!("{diagnostics:?}");

        assert!(rendered.contains("fnv1a64:abc123"));
        assert!(rendered.contains("fnv1a64:first"));
        assert!(!rendered.contains("content"));
        assert!(!rendered.contains("path"));
        assert!(!rendered.contains("secret payload"));
        assert!(!rendered.contains("/Users/alice/private/repo/src/lib.rs"));
    }

    #[test]
    fn applies_top_n_limit_to_ranked_outputs() {
        let diagnostics = report_diagnostics(
            &[
                event("file.read", "digest-a", 100, 100, Some("digest-a")),
                event("file.read", "digest-b", 90, 100, Some("digest-b")),
                event("file.read", "digest-c", 80, 100, Some("digest-c")),
            ],
            2,
        );

        assert_eq!(diagnostics.repeated_payload_groups.len(), 2);
        assert_eq!(diagnostics.expensive_events.len(), 2);
        assert_eq!(
            diagnostics
                .expensive_events
                .iter()
                .map(|event| event.digest.as_str())
                .collect::<Vec<&str>>(),
            vec!["digest-a", "digest-b"]
        );
    }
}
