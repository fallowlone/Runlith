//! Minimal provider-neutral verification seam (TASK-0037, G3).
//!
//! This crate defines the evaluation boundary only: the request shape, the
//! two G3 verdicts, and the port trait. It implements no judge, no score
//! system, no test runner, no scanner — those belong to the G8 Evaluation
//! Engine. For G3 the only implementation is the deterministic
//! `fake-evaluator` adapter.
//!
//! Authority (ADR-0018): the evaluator produces a trusted verdict plus
//! evidence; only the trusted Task application path commits the
//! authoritative lifecycle transition on that verdict. The evaluator never
//! touches Task persistence, runtime persistence, or the journal.

mod admission_snapshot;
mod candidate;
mod capture;
mod claim_verification;
mod error;
mod observed_effect;
mod port;
mod request;
mod verdict;

pub use admission_snapshot::{
    AdmissionSnapshot, ClaimMarker, CriterionIntegrity, DischargeMarker, IntegrityMarker,
    VerdictMarker,
};
pub use candidate::{
    candidate_sha256, requirement_revision, CandidateCheckpointPhase, CandidateDeletion,
    CandidateError, CandidateFile, CandidateFreshnessCheckpoint, CandidateIntegrityAssessment,
    CandidateIntegrityOutcome, CandidateManifest, CandidateManifestEntry, CandidateSelector,
    CandidateTestResult, GitObjectFormat, VerifiedCandidate, MAX_CANDIDATE_BLOB_BYTES,
    MAX_CANDIDATE_ENTRIES, MAX_CANDIDATE_TREE_DEPTH,
};
pub use capture::{
    CaptureRequest, CapturedObservation, EffectCapturePort, ProcessExitObservation,
    ProcessExitObservationSink, SubjectMaterial, MAX_CAPTURE_PATH, MAX_CAPTURE_SUBJECT,
};
pub use claim_verification::{
    evaluate_admission, AdmissionContext, AdmissionDecision, AdmissionId, AdmissionReport,
    AgentClaim, ApprovalDischarge, ClaimId, ClaimVerdict, ClaimVerification, ClaimedOutcome,
    ClaimedSubject, CriterionEvidence, CriterionOutcome, CriterionStatus, DischargeId,
    EvidenceRefs, ExemptionId, IntegrityBasis, PolicyExemption, ResolvedEffect, VerificationId,
    MAX_CLAIM_TEXT, MAX_EVIDENCE_REFS, TRUSTED_VERIFIER_IDENTITY,
};
pub use error::{EvaluationError, EvaluationResult};
pub use observed_effect::{
    subject_fingerprint, validate_git_oid, CaptureAuthority, EffectError, EffectId, EffectKind,
    EffectPayload, EffectProvenance, EffectSubjectRef, ObservedEffect, Sha256Digest,
    SubjectFingerprint, MAX_EFFECT_PAYLOAD, MAX_EFFECT_TEXT,
};
pub use port::EvaluationPort;
pub use request::EvaluationRequest;
pub use verdict::EvaluationVerdict;

// Identity re-exports so port implementors depend on this contract crate
// only: request fields stay typed without extra direct dependencies.
pub use agent_runtime::{AttemptId, ExecutionId};
pub use task_domain::TaskId;
