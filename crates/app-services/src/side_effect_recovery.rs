//! ADR-0031 reconcile helpers for the workspace/Git dogfood
//! (TASK-0126): intent identity, dispatch-outcome classification, and
//! the reconcile vocabulary shared by the materialize and cleanup
//! paths. Pure; the service owns all Git and store I/O.

use workspace::{SideEffectKind, WorkspaceFailure};

/// Contract version bound into every workspace intent's policy
/// fingerprint. Workspace kinds are governed by static code contracts
/// (ADR-0023/0025/0031), not a revisioned policy; the constant
/// versions that contract for the audit trail.
pub const WORKSPACE_CONTRACT_FINGERPRINT: &str = "workspace-side-effect/v1";

/// Process-wide intent counter. Intent ids must be unique across
/// service instances sharing one database (parallel daemons, tests):
/// a per-instance counter collides when two instances mint in the
/// same millisecond, so the counter is static like the lease one and
/// the pid distinguishes processes.
static INTENT_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Mints a unique intent id. Uniqueness needs no coordination: the
/// clock instant plus process plus the process-wide counter never
/// repeats within one database.
pub fn mint_intent_id(now_ms: u64) -> String {
    let counter = INTENT_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("se-{now_ms}-{}-{counter}", std::process::id())
}

/// Deterministic idempotency key per (lease, kind): crash retries
/// represent the same identity (SE-4), and explicit successors carry
/// it forward by construction.
pub fn idempotency_key(lease_id: &str, kind: SideEffectKind) -> String {
    format!("idem-{lease_id}-{}", kind.serialize())
}

/// Classified dispatch error: what the §2 machine may record.
/// Success maps to CONFIRMED at the call site (postconditions
/// verified); only errors need classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    /// Pre-dispatch refusal: the target is provably unchanged.
    Failed,
    /// Anything else: unknown, reconcile before any retry.
    Ambiguous,
}

/// Maps a Git dispatch error onto the machine. Only the adapter's
/// documented pre-dispatch refusals (collision preflights, which run
/// before any mutation) prove the target unchanged; every other
/// failure — mid-dispatch backend errors, postcondition
/// contradictions, timeouts — is AMBIGUOUS (SE-2). Never invents
/// certainty the error does not carry.
pub fn classify_dispatch_error(error: &WorkspaceFailure) -> DispatchOutcome {
    match error {
        WorkspaceFailure::BranchCollision { .. } | WorkspaceFailure::WorktreeCollision { .. } => {
            DispatchOutcome::Failed
        }
        _ => DispatchOutcome::Ambiguous,
    }
}

/// Canonical pre-effect target reading for materialize: managed
/// branch absent plus target path absent.
pub fn materialize_pre_state(branch_absent: bool, path_absent: bool) -> String {
    format!("branch-absent={branch_absent} path-absent={path_absent}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_2_only_preflight_refusals_prove_unchanged() {
        assert_eq!(
            classify_dispatch_error(&WorkspaceFailure::BranchCollision {
                detail: "x".to_owned()
            }),
            DispatchOutcome::Failed
        );
        assert_eq!(
            classify_dispatch_error(&WorkspaceFailure::WorktreeCollision {
                detail: "x".to_owned()
            }),
            DispatchOutcome::Failed
        );
        for error in [
            WorkspaceFailure::CheckoutFailed {
                detail: "x".to_owned(),
            },
            WorkspaceFailure::GitBackendFailure {
                detail: "timeout".to_owned(),
            },
            WorkspaceFailure::StorageFailure {
                detail: "x".to_owned(),
            },
        ] {
            assert_eq!(
                classify_dispatch_error(&error),
                DispatchOutcome::Ambiguous,
                "unknown outcomes stay ambiguous: {error:?}"
            );
        }
    }

    #[test]
    fn idempotency_key_is_deterministic_per_lease_and_kind() {
        assert_eq!(
            idempotency_key("ws-1", SideEffectKind::WorktreeMaterialize),
            idempotency_key("ws-1", SideEffectKind::WorktreeMaterialize)
        );
        assert_ne!(
            idempotency_key("ws-1", SideEffectKind::WorktreeMaterialize),
            idempotency_key("ws-1", SideEffectKind::WorkspaceCleanup)
        );
        assert_ne!(
            idempotency_key("ws-1", SideEffectKind::WorktreeMaterialize),
            idempotency_key("ws-2", SideEffectKind::WorktreeMaterialize)
        );
    }
}
