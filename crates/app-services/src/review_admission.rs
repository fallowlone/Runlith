//! ADR-0033 admission-time review selection (pure decision stage).
//!
//! This module resolves the canonical owner-managed setting plus the
//! `ScopedReviewed` policy plus an optional trusted review record into
//! one admission decision. Durable reads/writes live in the service
//! layer around it (later TASK-0145 phase); this function owns the
//! decision table only.

/// Owned inputs to the pure admission decision.
#[derive(Clone, Debug)]
pub struct ReviewAdmissionInput {
    pub policy: policy::TestMutationPolicyRevision,
    pub setting: policy::ReviewPathSetting,
    pub path: String,
    pub check: String,
    pub record: Option<evaluation::ReviewRecord>,
    pub task_id: String,
    pub requirement_revision: String,
    pub base_manifest: String,
    pub candidate_manifest: String,
    pub changed_inputs: Vec<String>,
    pub affected_checks: Vec<String>,
    pub now_ms: u64,
    pub max_age_ms: u64,
}

/// Admission outcome. `Deny` is terminal for these inputs; `Blocked*`
/// may resolve when a review lands (B1) or never without the separate
/// B2 prerequisite demonstration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewAdmissionDecision {
    Admit,
    Deny { reason: &'static str },
    BlockedOnReview,
    BlockedOnB2Prerequisite,
}

/// Resolves policy + setting + optional record into one decision.
///
/// Order: scope first (exact path/check), then path selection (B2 is
/// selectable but always blocked — never falls back), then B1 record
/// validation against current values. The expectation carries the live
/// setting/policy revisions, so a switched setting or rotated policy
/// invalidates prior records through ordinary binding mismatch.
pub fn decide_review_admission(input: ReviewAdmissionInput) -> ReviewAdmissionDecision {
    if !input.policy.policy().allows(&input.path, &input.check) {
        return ReviewAdmissionDecision::Deny {
            reason: "path/check outside the authorized scope",
        };
    }
    if *input.setting.path() == policy::ReviewPath::B2 {
        return ReviewAdmissionDecision::BlockedOnB2Prerequisite;
    }
    let Some(record) = input.record.as_ref() else {
        return ReviewAdmissionDecision::BlockedOnReview;
    };
    let expected = evaluation::ReviewExpectation {
        task_id: input.task_id,
        requirement_revision: input.requirement_revision,
        base_manifest: input.base_manifest,
        candidate_manifest: input.candidate_manifest,
        changed_inputs: input.changed_inputs,
        affected_checks: input.affected_checks,
        policy_revision: input.policy.revision(),
        policy_fingerprint: input.policy.fingerprint().to_owned(),
        setting_revision: input.setting.revision(),
        setting_path: evaluation::ReviewPathName::B1,
    };
    match record.validate_current(&expected, input.now_ms, input.max_age_ms) {
        Ok(()) => ReviewAdmissionDecision::Admit,
        Err(evaluation::ReviewError::VerdictNotApproved) => ReviewAdmissionDecision::Deny {
            reason: "review verdict is not approved",
        },
        Err(evaluation::ReviewError::RecordStale) => ReviewAdmissionDecision::Deny {
            reason: "review record is stale",
        },
        Err(
            evaluation::ReviewError::BindingMismatch { .. }
            | evaluation::ReviewError::FingerprintMismatch
            | evaluation::ReviewError::MalformedRecord(_),
        ) => ReviewAdmissionDecision::Deny {
            reason: "review bindings do not match current values",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped_policy() -> policy::TestMutationPolicyRevision {
        let policy = policy::TestMutationPolicy::scoped_reviewed(
            vec!["tests/unit.rs".to_owned()],
            vec!["cargo-test-unit".to_owned()],
        )
        .unwrap();
        policy::TestMutationPolicyRevision::new(7, policy).unwrap()
    }

    fn setting(path: policy::ReviewPath) -> policy::ReviewPathSetting {
        policy::ReviewPathSetting::new(3, path).unwrap()
    }

    fn sha(byte: u8) -> String {
        format!("sha256:{}", (byte as char).to_string().repeat(64))
    }

    fn record_with(
        verdict: evaluation::ReviewVerdictName,
        setting_revision: u64,
    ) -> evaluation::ReviewRecord {
        evaluation::ReviewRecord::new(evaluation::ReviewRecordFields {
            record_id: "review-1".to_owned(),
            task_id: "T-1".to_owned(),
            requirement_revision: "rev-1".to_owned(),
            base_manifest: sha(b'a'),
            candidate_manifest: sha(b'b'),
            changed_inputs: vec!["tests/unit.rs".to_owned()],
            affected_checks: vec!["cargo-test-unit".to_owned()],
            policy_revision: 7,
            policy_fingerprint: scoped_policy().fingerprint().to_owned(),
            setting_revision,
            setting_path: evaluation::ReviewPathName::B1,
            reviewer_nonce: "nonce-1".to_owned(),
            reviewer_input_digest: sha(b'd'),
            reviewer_verdict: verdict,
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

    fn input(
        policy: policy::TestMutationPolicyRevision,
        setting: policy::ReviewPathSetting,
        record: Option<evaluation::ReviewRecord>,
    ) -> ReviewAdmissionInput {
        ReviewAdmissionInput {
            policy,
            setting,
            path: "tests/unit.rs".to_owned(),
            check: "cargo-test-unit".to_owned(),
            record,
            task_id: "T-1".to_owned(),
            requirement_revision: "rev-1".to_owned(),
            base_manifest: sha(b'a'),
            candidate_manifest: sha(b'b'),
            changed_inputs: vec!["tests/unit.rs".to_owned()],
            affected_checks: vec!["cargo-test-unit".to_owned()],
            now_ms: 1_700_000_100_000,
            max_age_ms: 86_400_000,
        }
    }

    #[test]
    fn forbidden_denies_without_consulting_review() {
        let policy =
            policy::TestMutationPolicyRevision::new(1, policy::TestMutationPolicy::Forbidden)
                .unwrap();
        let args = input(
            policy,
            setting(policy::ReviewPath::B1),
            Some(record_with(evaluation::ReviewVerdictName::Approved, 3)),
        );
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::Deny { .. }
        ));
    }

    #[test]
    fn scoped_b1_with_current_approved_record_admits() {
        let args = input(
            scoped_policy(),
            setting(policy::ReviewPath::B1),
            Some(record_with(evaluation::ReviewVerdictName::Approved, 3)),
        );
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::Admit
        ));
    }

    #[test]
    fn scoped_b1_without_record_blocks_on_review() {
        let args = input(scoped_policy(), setting(policy::ReviewPath::B1), None);
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::BlockedOnReview
        ));
    }

    #[test]
    fn scoped_b1_with_rejected_record_denies() {
        let args = input(
            scoped_policy(),
            setting(policy::ReviewPath::B1),
            Some(record_with(evaluation::ReviewVerdictName::Rejected, 3)),
        );
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::Deny { .. }
        ));
    }

    #[test]
    fn scoped_b2_is_selectable_but_blocked_without_fallback() {
        let policy_fp = scoped_policy();
        // Even a current approved B1 record cannot discharge B2: no
        // fallback between paths.
        let args = input(
            policy_fp,
            setting(policy::ReviewPath::B2),
            Some(record_with(evaluation::ReviewVerdictName::Approved, 3)),
        );
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::BlockedOnB2Prerequisite
        ));
        let args = input(scoped_policy(), setting(policy::ReviewPath::B2), None);
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::BlockedOnB2Prerequisite
        ));
    }

    #[test]
    fn scoped_out_of_scope_path_denies() {
        let mut args = input(
            scoped_policy(),
            setting(policy::ReviewPath::B1),
            Some(record_with(evaluation::ReviewVerdictName::Approved, 3)),
        );
        args.path = "tests/other.rs".to_owned();
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::Deny { .. }
        ));
    }

    #[test]
    fn switched_setting_revision_invalidates_prior_record() {
        let args = input(
            scoped_policy(),
            setting(policy::ReviewPath::B1),
            Some(record_with(evaluation::ReviewVerdictName::Approved, 2)),
        );
        assert!(matches!(
            decide_review_admission(args),
            ReviewAdmissionDecision::Deny { .. }
        ));
    }
}
