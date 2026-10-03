//! Boot-unique operation identity for journal correlation/causation ids
//! (audit finding F4).
//!
//! Correlation/causation ids were `op-{wall_ms}-{per-service counter}`.
//! The counter resets with every restart, so a clock regression (or two
//! daemons starting in the same millisecond) could mint duplicate ids and
//! merge unrelated causal chains. Every id minted here carries a third
//! segment: a per-process instance tag derived once at first use from the
//! process id, wall-clock time, and address entropy (ASLR slide of a
//! process-local static), hashed to 12 hex characters. A duplicate now
//! requires the same tag, the same millisecond, and the same counter
//! value — instead of just the latter two. No new dependencies: the hash
//! reuses the crate's existing `sha2`, and all entropy sources are
//! `std`-only and cross-platform (Windows included).

use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// Length of the instance tag in hex characters (48 bits).
const INSTANCE_TAG_HEX_LEN: usize = 12;

/// Per-process instance tag, computed once and shared by every service in
/// this process. Distinct services still mint distinct ids through their
/// own counters and timestamps; the tag only separates process lifetimes.
fn instance_tag() -> &'static str {
    static TAG: OnceLock<String> = OnceLock::new();
    TAG.get_or_init(|| {
        let pid = std::process::id();
        let wall_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        // ASLR slide entropy: the address of a process-local static.
        // `std` exposes no boot clock, so pid + wall time + slide carry
        // the separation: pid reuse alone is not enough, and a regressed
        // wall clock alone is not enough, to collide.
        static ANCHOR: OnceLock<u8> = OnceLock::new();
        let anchor = ANCHOR.get_or_init(|| 0);
        let address_entropy = std::ptr::from_ref(anchor) as usize;
        let mut hash = Sha256::new();
        hash.update(b"runlith:op-instance:v1\0");
        hash.update(pid.to_le_bytes());
        hash.update(wall_nanos.to_le_bytes());
        hash.update(address_entropy.to_le_bytes());
        let digest = hash.finalize();
        digest
            .iter()
            .take(INSTANCE_TAG_HEX_LEN / 2)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    })
}

/// Mints a journal correlation id: chain the committed fact belongs to.
#[must_use]
pub(crate) fn correlation_id(now_ms: u64, counter: u64) -> String {
    format!("op-{now_ms}-{counter}-{}", instance_tag())
}

/// Mints a journal causation id: the direct request that caused the fact.
#[must_use]
pub(crate) fn causation_id(now_ms: u64, counter: u64) -> String {
    format!("req-{now_ms}-{counter}-{}", instance_tag())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_tag_is_stable_and_hex_shaped() {
        let first = instance_tag();
        let second = instance_tag();
        assert_eq!(first, second, "tag must be computed once per process");
        assert_eq!(first.len(), INSTANCE_TAG_HEX_LEN);
        assert!(
            first.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "tag must be lowercase hex, got {first:?}"
        );
    }

    #[test]
    fn minted_ids_carry_time_counter_and_instance_segments() {
        let correlation = correlation_id(1_700_000_000_000, 7);
        let causation = causation_id(1_700_000_000_000, 7);
        let tag = instance_tag();
        assert_eq!(correlation, format!("op-1700000000000-7-{tag}"));
        assert_eq!(causation, format!("req-1700000000000-7-{tag}"));
        assert_ne!(correlation_id(1, 1), correlation_id(1, 2));
        assert_ne!(correlation_id(1, 1), correlation_id(2, 1));
    }
}
