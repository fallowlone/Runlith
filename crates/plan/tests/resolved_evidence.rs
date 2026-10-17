//! TASK-0175: resolved evidence content bounds, hashing, estimates.

use plan_domain::{
    resolved_estimated_input_tokens, resolved_within_call_bounds, EvidenceClassification,
    EvidenceRef, ResolvedEvidence, ResolvedEvidenceKind, MAX_RESOLVED_EVIDENCE_BYTES_PER_CALL,
    MAX_RESOLVED_EVIDENCE_ITEMS_PER_CALL,
};

fn item(content: &str) -> ResolvedEvidence {
    ResolvedEvidence::new(
        EvidenceRef::new("attempt:A-1").expect("ref"),
        ResolvedEvidenceKind::Attempt,
        EvidenceClassification::Project,
        content.to_owned(),
    )
    .expect("item")
}

#[test]
fn item_binds_hash_to_attached_bytes() {
    let resolved = item("evidence: attempt:A-1\noutcome: failed");
    assert!(resolved.content_hash.starts_with("sha256:"));
    assert_eq!(resolved.kind.as_str(), "attempt");
    assert_eq!(resolved.classification.as_str(), "PROJECT");
}

#[test]
fn blank_content_is_refused() {
    assert!(ResolvedEvidence::new(
        EvidenceRef::new("attempt:A-1").expect("ref"),
        ResolvedEvidenceKind::Attempt,
        EvidenceClassification::Project,
        "  \n ".to_owned(),
    )
    .is_err());
}

#[test]
fn over_long_content_truncates_at_the_cap_with_a_marker() {
    let long = "x".repeat(10_000);
    let resolved = item(&long);
    assert!(resolved.content.len() <= plan_domain::MAX_RESOLVED_EVIDENCE_BYTES_PER_ITEM);
    assert!(resolved.content.ends_with("[truncated]"));
    // Hash binds the truncated bytes, not the source.
    let direct = item(&resolved.content.clone());
    assert_eq!(resolved.content_hash, direct.content_hash);
}

#[test]
fn estimate_grows_with_content_and_items() {
    let small = item("short");
    let large = item(&"y".repeat(400));
    assert!(large.estimated_input_tokens() > small.estimated_input_tokens());
    let expected = small.estimated_input_tokens() + large.estimated_input_tokens();
    let total = resolved_estimated_input_tokens(&[small, large]);
    assert_eq!(total, expected);
}

#[test]
fn call_bounds_cover_item_count_and_total_bytes() {
    let many: Vec<_> = (0..MAX_RESOLVED_EVIDENCE_ITEMS_PER_CALL + 1)
        .map(|_| item("z"))
        .collect();
    assert!(!resolved_within_call_bounds(&many));
    let few: Vec<_> = (0..MAX_RESOLVED_EVIDENCE_ITEMS_PER_CALL)
        .map(|_| item("z"))
        .collect();
    assert!(resolved_within_call_bounds(&few));
    let total_bytes: usize = few.iter().map(|entry| entry.content.len()).sum();
    assert!(total_bytes <= MAX_RESOLVED_EVIDENCE_BYTES_PER_CALL);
}
