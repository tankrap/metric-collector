use std::collections::{HashMap, HashSet};
use std::fmt;

/// Stable digest algorithm label used by this module.
///
/// `fnv1a64` is the 64-bit FNV-1a hash. It is deterministic and fast, but it is
/// not cryptographic and must not be treated as collision-resistant. It is used
/// here as a stdlib-only fallback until the crate can depend on a cryptographic
/// digest implementation.
pub const DIGEST_ALGORITHM: &str = "fnv1a64";

const FNV1A64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Returns a stable, privacy-safe digest string for raw bytes.
///
/// The returned format is `fnv1a64:<16 lowercase hex digits>`.
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("{DIGEST_ALGORITHM}:{:016x}", fnv1a64(bytes))
}

/// Returns a stable, privacy-safe digest string for UTF-8 text.
pub fn digest_str(value: &str) -> String {
    digest_bytes(value.as_bytes())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV1A64_OFFSET_BASIS;

    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }

    hash
}

/// Result of observing one payload with [`RepeatDetector`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatObservation {
    /// Stable digest of the observed payload.
    pub content_digest: String,
    /// Digest this payload repeats within the same run, when it has been seen.
    pub repeat_of: Option<String>,
}

/// Tracks repeated payloads within a run without retaining raw payload content.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RepeatDetector {
    seen_digests_by_run: HashMap<String, HashSet<String>>,
}

impl RepeatDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observes bytes for `run_id` and returns a repeat link when the same
    /// digest has already appeared in that run.
    pub fn observe_bytes(&mut self, run_id: &str, bytes: &[u8]) -> RepeatObservation {
        let content_digest = digest_bytes(bytes);
        self.observe_digest(run_id, content_digest)
    }

    /// Observes UTF-8 text for `run_id` and returns a repeat link when the same
    /// digest has already appeared in that run.
    pub fn observe_str(&mut self, run_id: &str, value: &str) -> RepeatObservation {
        self.observe_bytes(run_id, value.as_bytes())
    }

    /// Observes an already-computed digest for `run_id`.
    pub fn observe_digest(&mut self, run_id: &str, content_digest: String) -> RepeatObservation {
        let seen_digests = self
            .seen_digests_by_run
            .entry(run_id.to_owned())
            .or_default();

        let repeat_of = if seen_digests.insert(content_digest.clone()) {
            None
        } else {
            Some(content_digest.clone())
        };

        RepeatObservation {
            content_digest,
            repeat_of,
        }
    }
}

impl fmt::Debug for RepeatDetector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepeatDetector")
            .field("run_count", &self.seen_digests_by_run.len())
            .field(
                "digest_count",
                &self
                    .seen_digests_by_run
                    .values()
                    .map(HashSet::len)
                    .sum::<usize>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_same_run_payloads_link_to_first_digest() {
        let mut detector = RepeatDetector::new();

        let first = detector.observe_str("run-1", "same payload");
        let second = detector.observe_str("run-1", "same payload");

        assert_eq!(first.repeat_of, None);
        assert_eq!(second.repeat_of, Some(first.content_digest.clone()));
        assert_eq!(second.content_digest, first.content_digest);
    }

    #[test]
    fn different_payloads_in_same_run_do_not_link() {
        let mut detector = RepeatDetector::new();

        let first = detector.observe_str("run-1", "payload one");
        let second = detector.observe_str("run-1", "payload two");

        assert_eq!(first.repeat_of, None);
        assert_eq!(second.repeat_of, None);
        assert_ne!(second.content_digest, first.content_digest);
    }

    #[test]
    fn same_payload_in_different_run_does_not_link() {
        let mut detector = RepeatDetector::new();

        let first = detector.observe_str("run-1", "same payload");
        let second = detector.observe_str("run-2", "same payload");

        assert_eq!(first.repeat_of, None);
        assert_eq!(second.repeat_of, None);
        assert_eq!(second.content_digest, first.content_digest);
    }

    #[test]
    fn digest_is_stable_and_documented() {
        assert_eq!(digest_str(""), "fnv1a64:cbf29ce484222325");
        assert_eq!(digest_str("hello"), "fnv1a64:a430d84680aabd0b");
        assert_eq!(digest_bytes(b"hello"), digest_str("hello"));
        assert_eq!(digest_str("hello"), digest_str("hello"));
    }

    #[test]
    fn detector_state_does_not_store_raw_content() {
        let mut detector = RepeatDetector::new();
        let secret = "raw secret payload must not be retained";

        let observation = detector.observe_str("run-1", secret);
        let state = format!("{detector:?}");

        assert_eq!(observation.content_digest, digest_str(secret));
        assert!(!state.contains(secret));
        assert!(
            detector
                .seen_digests_by_run
                .values()
                .flatten()
                .all(|stored| stored != secret)
        );
    }
}
