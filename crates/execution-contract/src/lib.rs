//! Shared provider-neutral `ExecutionPort` contract suite (TASK-0049, G5).
//!
//! The canonical provider execution seam is [`scheduler::ExecutionPort`]
//! (ADR-0024): one injected executor per orchestration cycle, durable
//! identities in, canonical evidence or a canonical problem out. Every
//! adapter implementing the seam — FakeAgent today, Claude today, a
//! second provider tomorrow — must satisfy this same contract.
//!
//! Provider implementations consume this contract; they do not define it.
//! Each provider crate invokes [`run_execution_port_contract`] from its own
//! test target with its own executors (deterministic scenarios on the
//! FakeAgent side, fixture executables on the Claude side — never live
//! provider access, network, or subscription quota).
//!
//! What the contract asserts (only behavior the seam promises):
//!
//! ```text
//! success   → Ok canonical evidence (bounded, non-partial) → submittable,
//!             never Task completion (interpretation carries no lifecycle)
//! failure   → Err(Failed) with bounded detail → attempt Failed, never Task
//!             failure at the interpretation level
//! malformed → Err (never Ok) → never submittable as success
//! timeout   → Err(TimedOut) → canonical timeout outcome
//! ref       → opaque stable identity, never embedding work identities
//! ```
//!
//! Deliberately NOT in the contract (not promised by the seam):
//!
//! ```text
//! cancellation  → out-of-band per provider (CancelFlag / ExecutionControl),
//!                 no `cancel` method exists on `ExecutionPort`
//! cwd/workdir   → construction-time binding, not a `WorkItem` field;
//!                 proven provider-side (lease-binding tests), not here
//! partial       → not universally produced; interpretation maps it to
//!                 Failed, but the seam does not require producing it
//! Task mutation → structural: the seam takes `&WorkItem` and returns
//!                 evidence; adapters have no storage port (dependency
//!                 direction + orchestration completion-authority tests)
//! streaming     → no streaming shape exists on the seam
//! usage/cost    → provider-side observation, not seam behavior
//! ```
//!
//! Adding a second provider (`provider-codex`) validates mechanically:
//!
//! ```text
//! provider-codex implements scheduler::ExecutionPort
//!   → its tests dev-depend on this crate
//!   → they build success/failure/malformed/timeout executors
//!   → they call run_execution_port_contract
//!   → composition root selects the implementation
//! ```
//!
//! No Task, Storage, Workspace, or orchestration change is involved.

use agent_runtime::{AttemptId, ExecutionId};
use scheduler::{interpret, AttemptInterpretation, ExecutionPort, ExecutionProblem, WorkItem};
use task_domain::TaskId;

/// Generous contract guardrail for evidence summaries (chars). Adapters
/// enforce their own tighter bounds (Claude 1024, FakeAgent short); the
/// contract catches unbounded transcripts, not exact limits.
pub const MAX_CONTRACT_SUMMARY_CHARS: usize = 4096;

/// Generous contract guardrail for evidence references (bytes). Opaque
/// session/provider refs are short; transcripts are not references.
pub const MAX_CONTRACT_REF_BYTES: usize = 1024;

/// Generous contract guardrail for failure detail (chars). Bounded
/// diagnostics only; dumps fail here.
pub const MAX_CONTRACT_DETAIL_CHARS: usize = 8192;

/// The four deterministic behaviors every `ExecutionPort` implementation
/// must exhibit. Supplied by the provider under test; the harness
/// asserts the canonical seam contract on each. All four must terminate
/// promptly and deterministically (no live provider, no network).
pub struct ContractExecutors<'a> {
    /// Behavior that succeeds: must yield canonical evidence.
    pub success: &'a dyn ExecutionPort,
    /// Behavior that fails: must yield canonical `Failed`.
    pub failure: &'a dyn ExecutionPort,
    /// Behavior with unusable provider output: must never become success.
    pub malformed: &'a dyn ExecutionPort,
    /// Behavior that exceeds its bound: must yield canonical `TimedOut`.
    pub timeout: &'a dyn ExecutionPort,
}

/// Runs the full shared contract against one provider's executors.
/// Panics (test failure) on the first violated seam promise.
pub fn run_execution_port_contract(executors: &ContractExecutors<'_>) {
    success_yields_submittable_evidence(executors.success);
    failure_yields_canonical_problem(executors.failure);
    malformed_never_becomes_success(executors.malformed);
    timeout_is_canonical(executors.timeout);
    provider_ref_is_opaque_identity(executors.success);
    provider_ref_is_opaque_identity(executors.failure);
}

/// Successful execution yields canonical evidence that is submittable
/// for independent verification — and nothing more. The interpretation
/// carries evidence plus an outcome, never Task completion authority.
pub fn success_yields_submittable_evidence(executor: &dyn ExecutionPort) {
    let work = work();
    let evidence = executor
        .execute(&work)
        .expect("success behavior must return Ok evidence");
    assert!(
        !evidence.summary.trim().is_empty(),
        "evidence summary must be non-blank"
    );
    assert!(
        evidence.summary.chars().count() <= MAX_CONTRACT_SUMMARY_CHARS,
        "evidence summary must be bounded operational text, got {} chars",
        evidence.summary.chars().count()
    );
    assert!(
        !evidence.evidence_ref.trim().is_empty(),
        "evidence ref must be non-blank"
    );
    assert!(
        evidence.evidence_ref.len() <= MAX_CONTRACT_REF_BYTES,
        "evidence ref must be an opaque reference, got {} bytes",
        evidence.evidence_ref.len()
    );
    assert!(
        !evidence.partial,
        "contract success behavior must be complete evidence"
    );
    match interpret(Ok(evidence), &work) {
        AttemptInterpretation::Submittable {
            summary,
            evidence_ref,
            ..
        } => {
            assert!(!summary.trim().is_empty());
            assert!(!evidence_ref.trim().is_empty());
        }
        AttemptInterpretation::NotSubmittable { .. } => {
            panic!("complete evidence must be submittable, never completion and never failure")
        }
    }
}

/// Provider failure yields the canonical `Failed` problem with bounded
/// detail, interpreted as attempt `Failed` — never Task failure, never
/// silent success.
pub fn failure_yields_canonical_problem(executor: &dyn ExecutionPort) {
    let work = work();
    let error = executor
        .execute(&work)
        .expect_err("failure behavior must return Err");
    match &error {
        ExecutionProblem::Failed { detail } => {
            assert!(
                !detail.trim().is_empty(),
                "failure detail must be non-blank"
            );
            assert!(
                detail.chars().count() <= MAX_CONTRACT_DETAIL_CHARS,
                "failure detail must be bounded, got {} chars",
                detail.chars().count()
            );
        }
        other => panic!("failure behavior must be Failed, got {other:?}"),
    }
    match interpret(Err(error), &work) {
        AttemptInterpretation::NotSubmittable {
            outcome: agent_runtime::AttemptOutcome::Failed,
            ..
        } => {}
        other => panic!("failure must interpret as attempt Failed, got {other:?}"),
    }
}

/// Malformed provider output can never become success: the seam returns
/// `Err`, and interpretation is never submittable.
pub fn malformed_never_becomes_success(executor: &dyn ExecutionPort) {
    let work = work();
    let behavior = executor.execute(&work);
    assert!(
        behavior.is_err(),
        "malformed provider result must never become success, got {behavior:?}"
    );
    match interpret(behavior, &work) {
        AttemptInterpretation::NotSubmittable { .. } => {}
        AttemptInterpretation::Submittable { .. } => {
            panic!("malformed output must never be submittable as success")
        }
    }
}

/// An exceeded bound yields the canonical `TimedOut` problem — the only
/// orchestration-relevant split the seam keeps beyond `Failed`.
pub fn timeout_is_canonical(executor: &dyn ExecutionPort) {
    let work = work();
    let error = executor
        .execute(&work)
        .expect_err("timeout behavior must return Err");
    assert!(
        matches!(error, ExecutionProblem::TimedOut { .. }),
        "timeout behavior must be TimedOut, got {error:?}"
    );
    match interpret(Err(error), &work) {
        AttemptInterpretation::NotSubmittable {
            outcome: agent_runtime::AttemptOutcome::TimedOut,
            ..
        } => {}
        other => panic!("timeout must interpret as attempt TimedOut, got {other:?}"),
    }
}

/// The provider reference is an opaque stable identity: non-blank,
/// bounded, identical across calls, and never embedding the ephemeral
/// work identities it executes for.
pub fn provider_ref_is_opaque_identity(executor: &dyn ExecutionPort) {
    let first = executor.provider_ref();
    let second = executor.provider_ref();
    assert_eq!(
        first.as_str(),
        second.as_str(),
        "provider ref must be stable across calls"
    );
    assert!(
        !first.as_str().trim().is_empty(),
        "provider ref must be non-blank"
    );
    assert!(
        first.as_str().len() <= 256,
        "provider ref must be a bounded identity, got {:?}",
        first.as_str()
    );
    for work_id in ["T-CONTRACT", "E-CONTRACT", "A-CONTRACT"] {
        assert!(
            !first.as_str().contains(work_id),
            "provider ref must stay opaque and never embed work identities, got {:?}",
            first.as_str()
        );
    }
}

/// Distinctive deterministic work identities for contract calls. Never
/// persisted, never executed by orchestration — the harness calls
/// `execute` directly on the seam.
fn work() -> WorkItem {
    WorkItem {
        task_id: TaskId::new("T-CONTRACT").unwrap(),
        execution_id: ExecutionId::new("E-CONTRACT").unwrap(),
        attempt_id: AttemptId::new("A-CONTRACT").unwrap(),
    }
}
