//! Sealed admission snapshot (TASK-0161, strict rule-5).
//!
//! [`AdmissionSnapshot`] is the canonical encoding of every durable input
//! a completion-admission report was built from. The trusted boundary
//! captures it from the same reads as the report; `admit_completion`
//! re-reads the same rows inside the committing transaction, rebuilds the
//! snapshot through this exact constructor, and refuses with `Stale` on
//! any drift. No admission logic lives here — only the sealed byte
//! identity of the inputs.

use crate::{
    requirement_revision, AgentClaim, ApprovalDischarge, AttemptId, ClaimVerification,
    EvidenceRefs, ExecutionId, IntegrityBasis, PolicyExemption, ResolvedEffect, SubjectFingerprint,
    TaskId,
};

/// Canonical sealed identity of one admission report's durable inputs.
/// Every list is sorted; equality is byte equality of the read set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionSnapshot {
    /// Task under admission.
    pub task_id: String,
    /// Sorted acceptance criteria.
    pub acceptance: Vec<String>,
    /// `(criterion_id, requirement_revision)` sorted by criterion.
    pub requirements: Vec<(String, String)>,
    /// Task risk text bound by CV-8 discharges.
    pub task_risk: Option<String>,
    /// Current trusted subject fingerprint at report time.
    pub current_fingerprint: Option<String>,
    /// Full per-task claim set: `(claim_id, criterion_id, outcome,
    /// subject_ref, subject_fingerprint, receipts, effects, sessions,
    /// asserted_by, asserted_at)`, sorted by claim id.
    pub claims: Vec<ClaimMarker>,
    /// Full per-task verdict set, sorted by verification id.
    pub verdicts: Vec<VerdictMarker>,
    /// Full per-task exemption set:
    /// `(exemption_id, criterion_id, subject, policy, created_at)`,
    /// sorted by exemption id.
    pub exemptions: Vec<(String, Option<String>, String, String, String)>,
    /// Full per-task discharge set:
    /// `(discharge_id, criterion_id, subject, risk, approval_ref,
    /// not_valid_before, not_valid_after, consumed_by, created_at)`,
    /// sorted by discharge id.
    pub discharges: Vec<DischargeMarker>,
    /// Union of resolved basis effects: `(effect_id, subject_fingerprint)`.
    pub resolved_effects: Vec<(String, String)>,
    /// Relied executions with their owning task: `(execution_id, task_id)`.
    pub relied_executions: Vec<(String, String)>,
    /// Per-criterion integrity basis with its relied keys:
    /// `(criterion_id, execution_id, attempt_id, clean,
    /// requirement_revision, policy_fingerprint, evaluated_base)`.
    pub integrity: Vec<IntegrityMarker>,
    /// Current policy fingerprint the report evaluated under (CV-7).
    pub current_policy: Option<String>,
    /// Report timestamp sealing the discharge validity windows.
    pub now: String,
}

/// Sealed marker of one durable claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimMarker {
    /// Durable claim identity (sort key).
    pub claim_id: String,
    /// Acceptance criterion the claim is about.
    pub criterion_id: String,
    /// What the agent asserted.
    pub claimed_outcome: String,
    /// Opaque subject reference.
    pub subject_ref: String,
    /// Fingerprint of the claimed subject state.
    pub subject_fingerprint: String,
    /// Canonical receipt references encoding.
    pub receipts: String,
    /// Canonical effect references encoding.
    pub effects: String,
    /// Canonical session references encoding.
    pub sessions: String,
    /// Agent/provider identity that asserted the claim.
    pub asserted_by: String,
    /// Assertion timestamp.
    pub asserted_at: String,
}

/// Sealed marker of one durable verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerdictMarker {
    /// Durable verdict identity (sort key).
    pub verification_id: String,
    /// The claim this verdict judges.
    pub claim_id: String,
    /// Verifier identity.
    pub verifier: String,
    /// Requirement kind the verdict was produced against.
    pub requirement_kind: String,
    /// Fingerprint of the exact subject state bound.
    pub subject_fingerprint: String,
    /// Exact requirement revision judged.
    pub requirement_revision: String,
    /// Policy fingerprint echoed from the requirement.
    pub policy_fingerprint: Option<String>,
    /// The verdict.
    pub verdict: String,
    /// Canonical receipt references encoding.
    pub receipts: String,
    /// Canonical effect references encoding.
    pub effects: String,
    /// Canonical session references encoding.
    pub sessions: String,
    /// Superseding verdict, if superseded.
    pub superseded_by: Option<String>,
    /// Decision timestamp.
    pub decided_at: String,
}

/// Sealed marker of one durable discharge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DischargeMarker {
    /// Durable discharge identity (sort key).
    pub discharge_id: String,
    /// The criterion discharged.
    pub criterion_id: String,
    /// Fingerprint of the exact subject state approved.
    pub subject_fingerprint: String,
    /// Typed risk text the approval covers.
    pub risk: String,
    /// Reference to the durable approval record.
    pub approval_ref: String,
    /// Start of the validity window (inclusive).
    pub not_valid_before: String,
    /// End of the validity window (exclusive).
    pub not_valid_after: String,
    /// Consuming admission, once single-used.
    pub consumed_by: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
}

/// Sealed marker of one relied integrity basis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrityMarker {
    /// Acceptance criterion the basis served (sort key).
    pub criterion_id: String,
    /// Relied execution identity.
    pub execution_id: String,
    /// Relied attempt identity.
    pub attempt_id: String,
    /// No tamper signal on the assessment.
    pub clean: bool,
    /// Requirement revision the assessment evaluated.
    pub requirement_revision: String,
    /// Evaluated base the assessment ran under.
    pub policy_fingerprint: String,
    /// Evaluated base bound to the relied-on execution.
    pub evaluated_base: String,
}

/// One per-criterion integrity input: the criterion, the relied
/// execution/attempt keys, and the basis the report used.
pub struct CriterionIntegrity<'a> {
    /// Acceptance criterion the basis served.
    pub criterion_id: &'a str,
    /// Relied execution identity.
    pub execution_id: &'a ExecutionId,
    /// Relied attempt identity.
    pub attempt_id: &'a AttemptId,
    /// Integrity basis the report used.
    pub basis: &'a IntegrityBasis,
}

fn encode_refs(refs: &EvidenceRefs) -> (String, String, String) {
    (
        refs.encode_receipts(),
        refs.encode_effects(),
        refs.encode_sessions(),
    )
}

impl AdmissionSnapshot {
    /// Captures the canonical snapshot from the exact objects the report
    /// read. Used both at report time (from the service's locals) and at
    /// commit time (from the transaction's re-reads) — a single
    /// constructor, so the two encodings can only differ on drift.
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        task: &task_domain::Task,
        current_fingerprint: Option<&SubjectFingerprint>,
        claims: &[AgentClaim],
        verdicts: &[ClaimVerification],
        exemptions: &[PolicyExemption],
        discharges: &[ApprovalDischarge],
        resolved_effects: &[ResolvedEffect],
        relied_executions: &[(&ExecutionId, &TaskId)],
        integrity: &[CriterionIntegrity<'_>],
        current_policy: Option<&str>,
        now: &str,
    ) -> Self {
        let mut acceptance = task.acceptance().to_vec();
        acceptance.sort();
        let mut requirements: Vec<(String, String)> = task
            .evidence_requirements()
            .iter()
            .map(|requirement| {
                (
                    requirement.criterion_id.clone(),
                    requirement_revision(requirement),
                )
            })
            .collect();
        requirements.sort();
        let mut claim_markers: Vec<ClaimMarker> = claims
            .iter()
            .map(|claim| {
                let (receipts, effects, sessions) = encode_refs(&claim.claimed_evidence);
                ClaimMarker {
                    claim_id: claim.claim_id.as_str().to_owned(),
                    criterion_id: claim.criterion_id.clone(),
                    claimed_outcome: claim.claimed_outcome.as_str().to_owned(),
                    subject_ref: claim.claimed_subject.subject_ref.clone(),
                    subject_fingerprint: claim
                        .claimed_subject
                        .subject_fingerprint
                        .as_str()
                        .to_owned(),
                    receipts,
                    effects,
                    sessions,
                    asserted_by: claim.asserted_by.clone(),
                    asserted_at: claim.asserted_at.clone(),
                }
            })
            .collect();
        claim_markers.sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
        let mut verdict_markers: Vec<VerdictMarker> = verdicts
            .iter()
            .map(|verdict| {
                let (receipts, effects, sessions) = encode_refs(&verdict.basis);
                VerdictMarker {
                    verification_id: verdict.verification_id.as_str().to_owned(),
                    claim_id: verdict.claim_id.as_str().to_owned(),
                    verifier: verdict.verifier.clone(),
                    requirement_kind: verdict.requirement_kind.as_str().to_owned(),
                    subject_fingerprint: verdict.subject_fingerprint.as_str().to_owned(),
                    requirement_revision: verdict.requirement_revision.clone(),
                    policy_fingerprint: verdict.policy_fingerprint.clone(),
                    verdict: verdict.verdict.as_str().to_owned(),
                    receipts,
                    effects,
                    sessions,
                    superseded_by: verdict
                        .superseded_by
                        .as_ref()
                        .map(|id| id.as_str().to_owned()),
                    decided_at: verdict.decided_at.clone(),
                }
            })
            .collect();
        verdict_markers.sort_by(|a, b| a.verification_id.cmp(&b.verification_id));
        let mut exemption_markers: Vec<(String, Option<String>, String, String, String)> =
            exemptions
                .iter()
                .map(|exemption| {
                    (
                        exemption.exemption_id.as_str().to_owned(),
                        exemption.criterion_id.clone(),
                        exemption.subject_fingerprint.as_str().to_owned(),
                        exemption.policy_fingerprint.clone(),
                        exemption.created_at.clone(),
                    )
                })
                .collect();
        exemption_markers.sort_by(|a, b| a.0.cmp(&b.0));
        let mut discharge_markers: Vec<DischargeMarker> = discharges
            .iter()
            .map(|discharge| DischargeMarker {
                discharge_id: discharge.discharge_id.as_str().to_owned(),
                criterion_id: discharge.criterion_id.clone(),
                subject_fingerprint: discharge.subject_fingerprint.as_str().to_owned(),
                risk: discharge.risk.clone(),
                approval_ref: discharge.approval_ref.clone(),
                not_valid_before: discharge.not_valid_before.clone(),
                not_valid_after: discharge.not_valid_after.clone(),
                consumed_by: discharge
                    .consumed_by
                    .as_ref()
                    .map(|id| id.as_str().to_owned()),
                created_at: discharge.created_at.clone(),
            })
            .collect();
        discharge_markers.sort_by(|a, b| a.discharge_id.cmp(&b.discharge_id));
        let mut resolved: Vec<(String, String)> = resolved_effects
            .iter()
            .map(|resolved| {
                (
                    resolved.effect_id.clone(),
                    resolved.subject_fingerprint.as_str().to_owned(),
                )
            })
            .collect();
        resolved.sort();
        resolved.dedup();
        let mut executions: Vec<(String, String)> = relied_executions
            .iter()
            .map(|(execution_id, task_id)| {
                (
                    execution_id.as_str().to_owned(),
                    task_id.as_str().to_owned(),
                )
            })
            .collect();
        executions.sort();
        executions.dedup();
        let mut integrity_markers: Vec<IntegrityMarker> = integrity
            .iter()
            .map(|entry| IntegrityMarker {
                criterion_id: entry.criterion_id.to_owned(),
                execution_id: entry.execution_id.as_str().to_owned(),
                attempt_id: entry.attempt_id.as_str().to_owned(),
                clean: entry.basis.clean,
                requirement_revision: entry.basis.requirement_revision.clone(),
                policy_fingerprint: entry.basis.policy_fingerprint.clone(),
                evaluated_base: entry.basis.evaluated_base.clone(),
            })
            .collect();
        integrity_markers.sort_by(|a, b| a.criterion_id.cmp(&b.criterion_id));
        Self {
            task_id: task.id().as_str().to_owned(),
            acceptance,
            requirements,
            task_risk: task.snapshot().risk,
            current_fingerprint: current_fingerprint.map(|fp| fp.as_str().to_owned()),
            claims: claim_markers,
            verdicts: verdict_markers,
            exemptions: exemption_markers,
            discharges: discharge_markers,
            resolved_effects: resolved,
            relied_executions: executions,
            integrity: integrity_markers,
            current_policy: current_policy.map(str::to_owned),
            now: now.to_owned(),
        }
    }
}
