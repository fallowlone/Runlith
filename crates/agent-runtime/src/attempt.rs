//! One concrete try inside an execution: executor reference, conclusion,
//! and opaque evidence note.
//!
//! The outcome vocabulary is deliberately small and provider-neutral.
//! It distinguishes only what orchestration policy needs (did work
//! evidence arrive, did the try fail, did it time out, was it
//! cancelled); *why* it failed travels as the opaque `detail` string
//! minted by the executing adapter (quota, malformed output, network,
//! policy rejection, …). FakeAgent scenario names are adapter-layer
//! simulation identities and never become domain variants here.

use crate::error::{RuntimeError, RuntimeResult};
use crate::id::{AttemptId, ExecutionId, ProviderRef};
use crate::snapshot::AttemptSnapshot;

/// How one attempt concluded. Terminal and immutable once recorded:
/// concluding an attempt never rewrites it afterwards.
///
/// ```text
/// Attempt success  !=  Task COMPLETED
/// Attempt failure  !=  Task FAILED automatically
/// ```
///
/// Success means “evidence was produced for review/verification” — the
/// Task still completes only through the trusted evaluation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttemptOutcome {
    /// The try produced result evidence for the pipeline.
    Succeeded,
    /// The try failed (crash, malformed output, quota/capacity,
    /// network/runtime failure, policy rejection, … — see `detail`).
    Failed,
    /// The try made no progress within its bound (hang/timeout policy
    /// belongs to orchestration, not to this enum).
    TimedOut,
    /// The try was cancelled before concluding on its own.
    Cancelled,
}

/// One concrete try inside exactly one execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    id: AttemptId,
    execution_id: ExecutionId,
    provider: ProviderRef,
    outcome: Option<AttemptOutcome>,
    detail: Option<String>,
}

impl Attempt {
    /// Begins one open attempt. `provider` names the executor opaquely;
    /// no provider-specific behavior may branch on it.
    pub(crate) fn begin(id: AttemptId, execution_id: ExecutionId, provider: ProviderRef) -> Self {
        Self {
            id,
            execution_id,
            provider,
            outcome: None,
            detail: None,
        }
    }

    /// Attempt identity.
    #[must_use]
    pub fn id(&self) -> &AttemptId {
        &self.id
    }

    /// The single execution this attempt belongs to. Fixed at creation,
    /// never rewritten: attempts cannot migrate between executions.
    #[must_use]
    pub fn execution_id(&self) -> &ExecutionId {
        &self.execution_id
    }

    /// Opaque executor reference recorded at attempt start.
    #[must_use]
    pub fn provider(&self) -> &ProviderRef {
        &self.provider
    }

    /// Whether the attempt is still running (no outcome recorded yet).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.outcome.is_none()
    }

    /// The recorded conclusion, if any. `None` while the attempt is open.
    #[must_use]
    pub fn outcome(&self) -> Option<AttemptOutcome> {
        self.outcome
    }

    /// Opaque evidence note recorded with the outcome (adapter-minted:
    /// never a transcript, never a secret).
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Records the single conclusion of an open attempt. Exactly-once:
    /// a concluded attempt rejects every further conclusion, so failed
    /// history can never be rewritten as success.
    pub(crate) fn conclude(
        &mut self,
        outcome: AttemptOutcome,
        detail: impl Into<String>,
    ) -> RuntimeResult<AttemptOutcome> {
        if self.outcome.is_some() {
            return Err(RuntimeError::AttemptAlreadyConcluded(format!(
                "attempt {} is already concluded",
                self.id
            )));
        }
        let detail = detail.into();
        if detail.trim().is_empty() {
            return Err(RuntimeError::input(
                "attempt conclusion requires a non-blank detail",
            ));
        }
        self.outcome = Some(outcome);
        self.detail = Some(detail.trim().to_owned());
        Ok(outcome)
    }

    /// Exports this attempt as a plain-data snapshot for the outbound
    /// storage port. The snapshot carries no behavior.
    #[must_use]
    pub fn snapshot(&self) -> AttemptSnapshot {
        AttemptSnapshot {
            id: self.id.clone(),
            execution_id: self.execution_id.clone(),
            provider: self.provider.clone(),
            outcome: self.outcome,
            detail: self.detail.clone(),
        }
    }

    /// Rebuilds an attempt from a storage-port snapshot, re-validating
    /// every field. Outcome and detail commit together in the domain, so
    /// a snapshot carrying exactly one of them is corruption, never a
    /// default: open means both absent, concluded means both present
    /// with a non-blank note.
    pub fn restore(snapshot: AttemptSnapshot) -> RuntimeResult<Self> {
        match (snapshot.outcome, snapshot.detail) {
            (None, None) => Ok(Self::begin(
                snapshot.id,
                snapshot.execution_id,
                snapshot.provider,
            )),
            (Some(outcome), Some(detail)) => {
                let mut attempt =
                    Self::begin(snapshot.id, snapshot.execution_id, snapshot.provider);
                attempt.conclude(outcome, detail)?;
                Ok(attempt)
            }
            _ => Err(RuntimeError::input(format!(
                "attempt {} stores an outcome without its evidence note or vice versa",
                snapshot.id
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_attempt(id: &str) -> Attempt {
        Attempt::begin(
            AttemptId::new(id).unwrap(),
            ExecutionId::new("E-1").unwrap(),
            ProviderRef::new("test-executor").unwrap(),
        )
    }

    #[test]
    fn new_attempt_is_open_with_fixed_ownership() {
        let attempt = open_attempt("A-1");
        assert!(attempt.is_open());
        assert_eq!(attempt.outcome(), None);
        assert_eq!(attempt.execution_id().as_str(), "E-1");
        assert_eq!(attempt.provider().as_str(), "test-executor");
    }

    #[test]
    fn conclude_records_outcome_and_detail_once() {
        let mut attempt = open_attempt("A-1");
        assert_eq!(
            attempt
                .conclude(AttemptOutcome::Failed, "process disappeared")
                .unwrap(),
            AttemptOutcome::Failed
        );
        assert!(!attempt.is_open());
        assert_eq!(attempt.outcome(), Some(AttemptOutcome::Failed));
        assert_eq!(attempt.detail(), Some("process disappeared"));
        // History is immutable: no rewrite into success.
        assert!(matches!(
            attempt.conclude(AttemptOutcome::Succeeded, "retry won"),
            Err(RuntimeError::AttemptAlreadyConcluded(_))
        ));
        assert_eq!(attempt.outcome(), Some(AttemptOutcome::Failed));
        assert_eq!(attempt.detail(), Some("process disappeared"));
    }

    #[test]
    fn conclude_rejects_blank_detail_without_mutation() {
        let mut attempt = open_attempt("A-1");
        assert!(attempt.conclude(AttemptOutcome::Succeeded, "   ").is_err());
        assert!(attempt.is_open());
        assert_eq!(attempt.outcome(), None);
    }

    #[test]
    fn outcome_vocabulary_stays_small_and_neutral() {
        // The domain distinguishes orchestration-relevant conclusions only.
        // No per-scenario variants: scenario identity stays adapter-owned.
        let outcomes = [
            AttemptOutcome::Succeeded,
            AttemptOutcome::Failed,
            AttemptOutcome::TimedOut,
            AttemptOutcome::Cancelled,
        ];
        assert_eq!(outcomes.len(), 4);
        for outcome in outcomes {
            assert_eq!(outcome, outcome);
        }
    }
}
