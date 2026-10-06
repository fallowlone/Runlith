//! ADR-0033 scoped read-only review scope (pure mapping stage).
//!
//! A B1/B2 review observes exactly the captured candidate inputs named
//! by the review record — nothing else. This module maps a validated
//! [`evaluation::ReviewRecord`] to the exact relative scope paths the
//! G6 layer authorizes read-only for the review operation. Enforcement
//! (per-descriptor authorization under the existing G6 boundary) lives
//! in the service layer around it (later TASK-0145 phase).

/// Exact relative scope paths for one review: the record's canonical
/// changed inputs, in canonical order. The record constructor already
/// rejects escapes; the mapping stays total over validated records.
pub fn review_scope(record: &evaluation::ReviewRecord) -> Vec<String> {
    record.changed_inputs().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(byte: u8) -> String {
        format!("sha256:{}", (byte as char).to_string().repeat(64))
    }

    fn record() -> evaluation::ReviewRecord {
        evaluation::ReviewRecord::new(evaluation::ReviewRecordFields {
            record_id: "review-1".to_owned(),
            task_id: "T-1".to_owned(),
            requirement_revision: "rev-1".to_owned(),
            base_manifest: sha(b'a'),
            candidate_manifest: sha(b'b'),
            changed_inputs: vec![
                "tests/b.rs".to_owned(),
                "tests/a.rs".to_owned(),
                "tests/a.rs".to_owned(),
            ],
            affected_checks: vec!["cargo-test-unit".to_owned()],
            policy_revision: 7,
            policy_fingerprint: sha(b'c'),
            setting_revision: 3,
            setting_path: evaluation::ReviewPathName::B1,
            reviewer_nonce: "nonce-1".to_owned(),
            reviewer_input_digest: sha(b'd'),
            reviewer_verdict: evaluation::ReviewVerdictName::Approved,
            implementer_execution_id: "E-1".to_owned(),
            implementer_attempt_id: "A-1".to_owned(),
            implementer_admission_ref: "runner-admission-1".to_owned(),
            rationale: "covers the changed check".to_owned(),
            evidence_refs: vec!["effect-1".to_owned()],
            captured_at_ms: 1_700_000_000_000,
            supersedes: None,
        })
        .unwrap()
    }

    #[test]
    fn review_scope_is_exactly_the_record_changed_inputs() {
        let scope = review_scope(&record());
        assert_eq!(
            scope,
            vec!["tests/a.rs".to_owned(), "tests/b.rs".to_owned()]
        );
    }

    #[test]
    fn review_scope_never_escapes_through_dot_segments() {
        // Changed inputs are record-validated (no ../.  segments), so the
        // scope mapping is total; this guards the mapping against future
        // record-shape drift by asserting the invariant explicitly.
        for path in review_scope(&record()) {
            assert!(!path.starts_with('/'));
            assert!(!path.split('/').any(|part| part == "." || part == ".."));
        }
    }
}
