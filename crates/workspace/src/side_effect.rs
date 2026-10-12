//! ADR-0031 side-effect intent domain (TASK-0126): the §2 state
//! machine, the frozen v1 kind registry, canonical parameter hashing,
//! and the §3 durable record shape. Pure: no I/O, no Git, no clock.
//!
//! Rule IDs (SE-1..SE-8) appear verbatim in implementing test names
//! per ADR-0031 §5.

use sha2::{Digest, Sha256};

/// Failures of intent construction and transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SideEffectError {
    UnknownKind(String),
    UnknownState(String),
    IllegalTransition {
        from: SideEffectState,
        event: TransitionEvent,
    },
    MissingEvidence,
    MalformedIntent(String),
}

impl std::fmt::Display for SideEffectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKind(kind) => write!(f, "unregistered side-effect kind: {kind}"),
            Self::UnknownState(state) => write!(f, "unknown side-effect state: {state}"),
            Self::IllegalTransition { from, event } => {
                write!(f, "illegal transition: {from:?} + {event:?}")
            }
            Self::MissingEvidence => f.write_str("terminal/reconcile transition requires evidence"),
            Self::MalformedIntent(detail) => write!(f, "malformed side-effect intent: {detail}"),
        }
    }
}

impl std::error::Error for SideEffectError {}

/// Frozen v1 operation-kind registry (ADR-0031 §4). Extended only by
/// ADR amendment; anything else fails closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideEffectKind {
    WorktreeMaterialize,
    WorkspaceCleanup,
}

impl SideEffectKind {
    pub fn parse(value: &str) -> Result<Self, SideEffectError> {
        match value {
            "worktree-materialize" => Ok(Self::WorktreeMaterialize),
            "workspace-cleanup" => Ok(Self::WorkspaceCleanup),
            _ => Err(SideEffectError::UnknownKind(value.to_owned())),
        }
    }

    pub fn serialize(&self) -> &'static str {
        match self {
            Self::WorktreeMaterialize => "worktree-materialize",
            Self::WorkspaceCleanup => "workspace-cleanup",
        }
    }

    /// The kind's named continuation operation for SAFE_TO_CONTINUE.
    pub fn continuation(&self) -> ContinuationKind {
        match self {
            Self::WorktreeMaterialize => ContinuationKind::AttachWorktree,
            Self::WorkspaceCleanup => ContinuationKind::CleanupCatchUp,
        }
    }
}

/// Named continuation operations (ADR-0031 §4): deterministic,
/// previously-proven completions, never discretionary choices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuationKind {
    AttachWorktree,
    CleanupCatchUp,
}

/// The §2 machine states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideEffectState {
    Prepared,
    IntentRecorded,
    Executing,
    Confirmed,
    Failed,
    Ambiguous,
    Reconcile,
    SafeToRetry,
    SafeToContinue,
    Escalate,
}

impl SideEffectState {
    pub fn parse(value: &str) -> Result<Self, SideEffectError> {
        match value {
            "PREPARED" => Ok(Self::Prepared),
            "INTENT_RECORDED" => Ok(Self::IntentRecorded),
            "EXECUTING" => Ok(Self::Executing),
            "CONFIRMED" => Ok(Self::Confirmed),
            "FAILED" => Ok(Self::Failed),
            "AMBIGUOUS" => Ok(Self::Ambiguous),
            "RECONCILE" => Ok(Self::Reconcile),
            "SAFE_TO_RETRY" => Ok(Self::SafeToRetry),
            "SAFE_TO_CONTINUE" => Ok(Self::SafeToContinue),
            "ESCALATE" => Ok(Self::Escalate),
            _ => Err(SideEffectError::UnknownState(value.to_owned())),
        }
    }

    pub fn serialize(&self) -> &'static str {
        match self {
            Self::Prepared => "PREPARED",
            Self::IntentRecorded => "INTENT_RECORDED",
            Self::Executing => "EXECUTING",
            Self::Confirmed => "CONFIRMED",
            Self::Failed => "FAILED",
            Self::Ambiguous => "AMBIGUOUS",
            Self::Reconcile => "RECONCILE",
            Self::SafeToRetry => "SAFE_TO_RETRY",
            Self::SafeToContinue => "SAFE_TO_CONTINUE",
            Self::Escalate => "ESCALATE",
        }
    }

    /// Terminal for every transition (autonomous or otherwise).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Confirmed | Self::Failed)
    }
}

/// Transition events. Exactly the §2 edges exist; everything else is
/// [`SideEffectError::IllegalTransition`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionEvent {
    RecordIntent,
    Dispatch,
    Confirm,
    Fail,
    MarkAmbiguous,
    BeginReconcile,
    ReconciledToConfirmed,
    ReconciledToRetry,
    ReconciledToContinue,
    Escalate,
    Redispatch,
    DispatchContinuation,
}

/// The normative §2 transition table.
pub fn transition(
    from: SideEffectState,
    event: TransitionEvent,
) -> Result<SideEffectState, SideEffectError> {
    let next = match (from, event) {
        (SideEffectState::Prepared, TransitionEvent::RecordIntent) => {
            SideEffectState::IntentRecorded
        }
        (SideEffectState::IntentRecorded, TransitionEvent::Dispatch) => SideEffectState::Executing,
        (SideEffectState::Executing, TransitionEvent::Confirm) => SideEffectState::Confirmed,
        (SideEffectState::Executing, TransitionEvent::Fail) => SideEffectState::Failed,
        (SideEffectState::Executing, TransitionEvent::MarkAmbiguous) => SideEffectState::Ambiguous,
        (SideEffectState::Ambiguous, TransitionEvent::BeginReconcile) => SideEffectState::Reconcile,
        (SideEffectState::Reconcile, TransitionEvent::ReconciledToConfirmed) => {
            SideEffectState::Confirmed
        }
        (SideEffectState::Reconcile, TransitionEvent::ReconciledToRetry) => {
            SideEffectState::SafeToRetry
        }
        (SideEffectState::Reconcile, TransitionEvent::ReconciledToContinue) => {
            SideEffectState::SafeToContinue
        }
        (SideEffectState::Reconcile, TransitionEvent::Escalate) => SideEffectState::Escalate,
        (SideEffectState::SafeToRetry, TransitionEvent::Redispatch) => SideEffectState::Executing,
        (SideEffectState::SafeToContinue, TransitionEvent::DispatchContinuation) => {
            SideEffectState::Executing
        }
        _ => {
            return Err(SideEffectError::IllegalTransition { from, event });
        }
    };
    Ok(next)
}

/// Identity replayed on every re-dispatch (SE-4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayIdentity {
    pub idempotency_key: String,
    pub params_hash: String,
    pub owner: String,
}

/// SE-4 classification: identical key+params+owner is a
/// distinguishable replay; anything else under a known key is a
/// conflict, never a silent reuse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayClass {
    Replay,
    Conflict,
}

pub fn classify_replay(original: &ReplayIdentity, candidate: &ReplayIdentity) -> ReplayClass {
    if original == candidate {
        ReplayClass::Replay
    } else {
        ReplayClass::Conflict
    }
}

fn hash_fields(domain: &[u8], fields: &[&str]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(b"\0");
    for field in fields {
        hash.update(field.as_bytes());
        hash.update(b"\0");
    }
    format!("sha256:{:x}", hash.finalize())
}

/// Canonical `params_hash` for `WORKTREE_MATERIALIZE` (§4 table):
/// managed branch name, target path, exact base revision, owning
/// lease/execution identity.
pub fn materialize_params_hash(
    branch: &str,
    target_path: &str,
    base_revision: &str,
    lease_id: &str,
    execution_id: &str,
) -> String {
    hash_fields(
        b"worktree-materialize-params/v1",
        &[branch, target_path, base_revision, lease_id, execution_id],
    )
}

/// Canonical `params_hash` for `WORKSPACE_CLEANUP` (§4 table):
/// worktree path, owning lease/execution identity, branch deletion
/// scope.
pub fn cleanup_params_hash(
    worktree_path: &str,
    lease_id: &str,
    execution_id: &str,
    branch_scope: &str,
) -> String {
    hash_fields(
        b"workspace-cleanup-params/v1",
        &[worktree_path, lease_id, execution_id, branch_scope],
    )
}

/// Constructor fields for [`SideEffectIntent`].
#[derive(Clone, Debug)]
pub struct SideEffectIntentFields {
    pub intent_id: String,
    pub kind: SideEffectKind,
    pub target_ref: String,
    pub params_hash: String,
    pub idempotency_key: String,
    pub task_id: String,
    pub execution_id: String,
    pub lease_id: String,
    pub pre_state_fingerprint: Option<String>,
    pub policy_fingerprint: String,
}

/// The §3 durable record (in-memory shape; persistence lives in the
/// store layer). Starts `Prepared`; transitions append evidence.
#[derive(Clone, Debug)]
pub struct SideEffectIntent {
    intent_id: String,
    kind: SideEffectKind,
    target_ref: String,
    params_hash: String,
    idempotency_key: String,
    task_id: String,
    execution_id: String,
    lease_id: String,
    pre_state_fingerprint: Option<String>,
    outcome_evidence_refs: Vec<String>,
    policy_fingerprint: String,
    state: SideEffectState,
}

impl SideEffectIntent {
    pub fn new(fields: SideEffectIntentFields) -> Result<Self, SideEffectError> {
        let malformed = |detail: &str| SideEffectError::MalformedIntent(detail.to_owned());
        for (value, name) in [
            (&fields.intent_id, "intent id"),
            (&fields.target_ref, "target ref"),
            (&fields.params_hash, "params hash"),
            (&fields.idempotency_key, "idempotency key"),
            (&fields.task_id, "task id"),
            (&fields.execution_id, "execution id"),
            (&fields.lease_id, "lease id"),
            (&fields.policy_fingerprint, "policy fingerprint"),
        ] {
            if value.is_empty() || value.len() > 1024 {
                return Err(malformed(name));
            }
        }
        if fields
            .pre_state_fingerprint
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > 4096)
        {
            return Err(malformed("pre-state fingerprint"));
        }
        Ok(Self {
            intent_id: fields.intent_id,
            kind: fields.kind,
            target_ref: fields.target_ref,
            params_hash: fields.params_hash,
            idempotency_key: fields.idempotency_key,
            task_id: fields.task_id,
            execution_id: fields.execution_id,
            lease_id: fields.lease_id,
            pre_state_fingerprint: fields.pre_state_fingerprint,
            outcome_evidence_refs: Vec::new(),
            policy_fingerprint: fields.policy_fingerprint,
            state: SideEffectState::Prepared,
        })
    }

    /// Applies one transition, enforcing the §2 table. Terminal and
    /// reconcile-outcome events require evidence (SE-5); the evidence
    /// is appended to the record.
    pub fn apply(
        &mut self,
        event: TransitionEvent,
        evidence: &[String],
    ) -> Result<(), SideEffectError> {
        let next = transition(self.state, event)?;
        if requires_evidence(event) && evidence.is_empty() {
            return Err(SideEffectError::MissingEvidence);
        }
        self.outcome_evidence_refs.extend(evidence.iter().cloned());
        self.state = next;
        Ok(())
    }

    pub fn intent_id(&self) -> &str {
        &self.intent_id
    }

    pub fn kind(&self) -> SideEffectKind {
        self.kind
    }

    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }

    pub fn params_hash(&self) -> &str {
        &self.params_hash
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub fn lease_id(&self) -> &str {
        &self.lease_id
    }

    pub fn pre_state_fingerprint(&self) -> Option<&str> {
        self.pre_state_fingerprint.as_deref()
    }

    pub fn outcome_evidence_refs(&self) -> &[String] {
        &self.outcome_evidence_refs
    }

    pub fn policy_fingerprint(&self) -> &str {
        &self.policy_fingerprint
    }

    pub fn state(&self) -> SideEffectState {
        self.state
    }

    pub fn replay_identity(&self) -> ReplayIdentity {
        ReplayIdentity {
            idempotency_key: self.idempotency_key.clone(),
            params_hash: self.params_hash.clone(),
            owner: self.execution_id.clone(),
        }
    }
}

fn requires_evidence(event: TransitionEvent) -> bool {
    matches!(
        event,
        TransitionEvent::Confirm
            | TransitionEvent::Fail
            | TransitionEvent::MarkAmbiguous
            | TransitionEvent::ReconciledToConfirmed
            | TransitionEvent::ReconciledToRetry
            | TransitionEvent::ReconciledToContinue
            | TransitionEvent::Escalate
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent() -> SideEffectIntent {
        SideEffectIntent::new(SideEffectIntentFields {
            intent_id: "se-1".to_owned(),
            kind: SideEffectKind::WorktreeMaterialize,
            target_ref: "ws-1".to_owned(),
            params_hash: materialize_params_hash("agent/x", "/root/ws-1", "abc123", "ws-1", "e-1"),
            idempotency_key: "idem-1".to_owned(),
            task_id: "t-1".to_owned(),
            execution_id: "e-1".to_owned(),
            lease_id: "ws-1".to_owned(),
            pre_state_fingerprint: Some("branch-absent+path-absent".to_owned()),
            policy_fingerprint: "sha256:00".to_owned(),
        })
        .unwrap()
    }

    #[test]
    fn se_1_machine_requires_intent_recorded_before_executing() {
        let mut intent = intent();
        assert_eq!(intent.state(), SideEffectState::Prepared);
        assert!(intent.apply(TransitionEvent::Dispatch, &[]).is_err());
        intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        assert_eq!(intent.state(), SideEffectState::IntentRecorded);
        intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
        assert_eq!(intent.state(), SideEffectState::Executing);
    }

    #[test]
    fn se_2_ambiguous_is_distinct_from_failed_and_reconciles_only() {
        for (event, state) in [
            (TransitionEvent::Confirm, SideEffectState::Confirmed),
            (TransitionEvent::Fail, SideEffectState::Failed),
            (TransitionEvent::MarkAmbiguous, SideEffectState::Ambiguous),
        ] {
            let mut intent = intent();
            intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
            intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
            intent.apply(event, &["evidence-1".to_owned()]).unwrap();
            assert_eq!(intent.state(), state);
        }
        // Ambiguous leaves only through RECONCILE — never directly to
        // a terminal, never back to EXECUTING.
        let mut intent = intent();
        intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
        intent
            .apply(TransitionEvent::MarkAmbiguous, &["evidence-1".to_owned()])
            .unwrap();
        assert!(intent
            .apply(TransitionEvent::Confirm, &["e".to_owned()])
            .is_err());
        assert!(intent
            .apply(TransitionEvent::Fail, &["e".to_owned()])
            .is_err());
        assert!(intent.apply(TransitionEvent::Dispatch, &[]).is_err());
        intent.apply(TransitionEvent::BeginReconcile, &[]).unwrap();
        assert_eq!(intent.state(), SideEffectState::Reconcile);
    }

    #[test]
    fn se_3_reconcile_outcomes_are_exhaustive_and_typed() {
        for (event, state) in [
            (
                TransitionEvent::ReconciledToConfirmed,
                SideEffectState::Confirmed,
            ),
            (
                TransitionEvent::ReconciledToRetry,
                SideEffectState::SafeToRetry,
            ),
            (
                TransitionEvent::ReconciledToContinue,
                SideEffectState::SafeToContinue,
            ),
            (TransitionEvent::Escalate, SideEffectState::Escalate),
        ] {
            let mut intent = intent();
            intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
            intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
            intent
                .apply(TransitionEvent::MarkAmbiguous, &["evidence-1".to_owned()])
                .unwrap();
            intent.apply(TransitionEvent::BeginReconcile, &[]).unwrap();
            intent
                .apply(event, &["reconcile-evidence".to_owned()])
                .unwrap();
            assert_eq!(intent.state(), state);
        }
        // No other edge leaves RECONCILE.
        let mut intent = intent();
        intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
        intent
            .apply(TransitionEvent::MarkAmbiguous, &["e".to_owned()])
            .unwrap();
        intent.apply(TransitionEvent::BeginReconcile, &[]).unwrap();
        assert!(intent.apply(TransitionEvent::Dispatch, &[]).is_err());
        assert!(intent.apply(TransitionEvent::RecordIntent, &[]).is_err());
    }

    #[test]
    fn se_4_replay_vs_conflict_by_key_params_and_owner() {
        let original = ReplayIdentity {
            idempotency_key: "idem-1".to_owned(),
            params_hash: "sha256:aa".to_owned(),
            owner: "e-1".to_owned(),
        };
        // Identical key + params + owner: distinguishable replay.
        assert_eq!(
            classify_replay(
                &original,
                &ReplayIdentity {
                    idempotency_key: "idem-1".to_owned(),
                    params_hash: "sha256:aa".to_owned(),
                    owner: "e-1".to_owned(),
                },
            ),
            ReplayClass::Replay
        );
        // Same key, different params or owner: conflict, never reuse.
        assert_eq!(
            classify_replay(
                &original,
                &ReplayIdentity {
                    idempotency_key: "idem-1".to_owned(),
                    params_hash: "sha256:bb".to_owned(),
                    owner: "e-1".to_owned(),
                },
            ),
            ReplayClass::Conflict
        );
        assert_eq!(
            classify_replay(
                &original,
                &ReplayIdentity {
                    idempotency_key: "idem-1".to_owned(),
                    params_hash: "sha256:aa".to_owned(),
                    owner: "e-2".to_owned(),
                },
            ),
            ReplayClass::Conflict
        );
    }

    #[test]
    fn se_5_terminal_facts_require_durable_evidence() {
        let mut intent = intent();
        intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
        assert!(matches!(
            intent.apply(TransitionEvent::Confirm, &[]),
            Err(SideEffectError::MissingEvidence)
        ));
        assert!(matches!(
            intent.apply(TransitionEvent::Fail, &[]),
            Err(SideEffectError::MissingEvidence)
        ));
    }

    #[test]
    fn se_7_unregistered_kind_fails_closed() {
        assert_eq!(
            SideEffectKind::parse("worktree-materialize").unwrap(),
            SideEffectKind::WorktreeMaterialize
        );
        assert_eq!(
            SideEffectKind::parse("workspace-cleanup").unwrap(),
            SideEffectKind::WorkspaceCleanup
        );
        // No standalone branch kind in v1 (ADR-0031 §4): unknown kinds
        // refuse and can never enter the machine.
        assert!(matches!(
            SideEffectKind::parse("git-branch-create"),
            Err(SideEffectError::UnknownKind(_))
        ));
        assert!(matches!(
            SideEffectKind::parse(""),
            Err(SideEffectError::UnknownKind(_))
        ));
    }

    #[test]
    fn se_8_escalate_is_terminal_for_autonomous_transitions() {
        let mut intent = intent();
        intent.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        intent.apply(TransitionEvent::Dispatch, &[]).unwrap();
        intent
            .apply(TransitionEvent::MarkAmbiguous, &["e".to_owned()])
            .unwrap();
        intent.apply(TransitionEvent::BeginReconcile, &[]).unwrap();
        intent
            .apply(TransitionEvent::Escalate, &["e".to_owned()])
            .unwrap();
        assert_eq!(intent.state(), SideEffectState::Escalate);
        for event in [
            TransitionEvent::Dispatch,
            TransitionEvent::Confirm,
            TransitionEvent::RecordIntent,
            TransitionEvent::BeginReconcile,
            TransitionEvent::ReconciledToRetry,
        ] {
            assert!(intent.apply(event, &["e".to_owned()]).is_err());
        }
    }

    #[test]
    fn reentry_edges_dispatch_exact_operations_only() {
        // SAFE_TO_RETRY re-dispatches the original operation.
        let mut retry = intent();
        retry.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        retry.apply(TransitionEvent::Dispatch, &[]).unwrap();
        retry
            .apply(TransitionEvent::MarkAmbiguous, &["e".to_owned()])
            .unwrap();
        retry.apply(TransitionEvent::BeginReconcile, &[]).unwrap();
        retry
            .apply(TransitionEvent::ReconciledToRetry, &["e".to_owned()])
            .unwrap();
        assert!(retry
            .apply(TransitionEvent::Confirm, &["e".to_owned()])
            .is_err());
        retry.apply(TransitionEvent::Redispatch, &[]).unwrap();
        assert_eq!(retry.state(), SideEffectState::Executing);
        // SAFE_TO_CONTINUE dispatches the named continuation.
        let mut continued = intent();
        continued.apply(TransitionEvent::RecordIntent, &[]).unwrap();
        continued.apply(TransitionEvent::Dispatch, &[]).unwrap();
        continued
            .apply(TransitionEvent::MarkAmbiguous, &["e".to_owned()])
            .unwrap();
        continued
            .apply(TransitionEvent::BeginReconcile, &[])
            .unwrap();
        continued
            .apply(TransitionEvent::ReconciledToContinue, &["e".to_owned()])
            .unwrap();
        continued
            .apply(TransitionEvent::DispatchContinuation, &[])
            .unwrap();
        assert_eq!(continued.state(), SideEffectState::Executing);
    }

    #[test]
    fn kinds_name_their_continuation_operations() {
        assert_eq!(
            SideEffectKind::WorktreeMaterialize.continuation(),
            ContinuationKind::AttachWorktree
        );
        assert_eq!(
            SideEffectKind::WorkspaceCleanup.continuation(),
            ContinuationKind::CleanupCatchUp
        );
    }

    #[test]
    fn params_hash_is_stable_and_field_sensitive() {
        let base = materialize_params_hash("agent/x", "/root/ws-1", "abc123", "ws-1", "e-1");
        assert_eq!(
            base,
            materialize_params_hash("agent/x", "/root/ws-1", "abc123", "ws-1", "e-1")
        );
        assert!(base.starts_with("sha256:"));
        for variant in [
            materialize_params_hash("agent/y", "/root/ws-1", "abc123", "ws-1", "e-1"),
            materialize_params_hash("agent/x", "/root/ws-2", "abc123", "ws-1", "e-1"),
            materialize_params_hash("agent/x", "/root/ws-1", "def456", "ws-1", "e-1"),
            materialize_params_hash("agent/x", "/root/ws-1", "abc123", "ws-2", "e-1"),
            materialize_params_hash("agent/x", "/root/ws-1", "abc123", "ws-1", "e-2"),
        ] {
            assert_ne!(base, variant);
        }
        let cleanup = cleanup_params_hash("/root/ws-1", "ws-1", "e-1", "managed-branch");
        assert!(cleanup.starts_with("sha256:"));
        assert_ne!(
            cleanup,
            cleanup_params_hash("/root/ws-1", "ws-1", "e-1", "other-scope")
        );
    }

    #[test]
    fn intent_record_validates_all_bindings() {
        let mut fields = SideEffectIntentFields {
            intent_id: "se-1".to_owned(),
            kind: SideEffectKind::WorktreeMaterialize,
            target_ref: "ws-1".to_owned(),
            params_hash: "sha256:aa".to_owned(),
            idempotency_key: "idem-1".to_owned(),
            task_id: "t-1".to_owned(),
            execution_id: "e-1".to_owned(),
            lease_id: "ws-1".to_owned(),
            pre_state_fingerprint: None,
            policy_fingerprint: "sha256:00".to_owned(),
        };
        assert!(SideEffectIntent::new(fields.clone()).is_ok());
        fields.intent_id.clear();
        assert!(SideEffectIntent::new(fields.clone()).is_err());
        fields.intent_id = "se-1".to_owned();
        fields.idempotency_key.clear();
        assert!(SideEffectIntent::new(fields.clone()).is_err());
        fields.idempotency_key = "idem-1".to_owned();
        fields.target_ref.clear();
        assert!(SideEffectIntent::new(fields).is_err());
    }
}
