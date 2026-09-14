//! Pure Workspace/Git domain and contract types (TASK-0040, G4).
//!
//! Provider-neutral identities, ownership, observations, failure taxonomy,
//! and the small Git capability port required before any Git mutation code
//! exists. Pure values and validation only: no `std::process`, no Git CLI
//! execution, no `git2`/libgit2, no filesystem mutation, no SQLite, no
//! Tokio, no provider SDK. Adapters implement [`GitPort`]; the domain never
//! instantiates infrastructure (composition root only).
//!
//! Authority: ADR-0008, ADR-0023, `docs/architecture/workspace.md`.
//! Identity conventions follow `task-domain` / `agent-runtime` (trimmed,
//! non-blank, 256-byte bound opaque strings).

mod failure;
mod ids;
mod lease;
mod liveness;
mod port;
mod status;
mod store;
mod trusted;

pub use failure::WorkspaceFailure;
pub use ids::{
    BranchRef, RepositoryRef, Revision, WorkspaceError, WorkspaceId, WorkspaceResult,
    MANAGED_BRANCH_PREFIX,
};
pub use lease::{LeaseLifecycle, WorkspaceLease, WorkspaceLeaseSnapshot};
pub use liveness::OwnerLiveness;
pub use port::{
    DiffChangeKind, DiffEntry, DiffEvidence, GitPort, WorktreeDiff, MAX_DIFF_ENTRIES,
    MAX_DIFF_SUMMARY_CHARS,
};
pub use status::WorktreeStatus;
pub use store::{WorkspaceEventKind, WorkspaceJournalEvidence, WorkspaceStore};
pub use trusted::TrustedCommonGit;

// Identity re-exports so port implementors depend on this contract crate
// plus `agent-runtime` only: ownership stays typed without extra deps.
pub use agent_runtime::{AttemptId, ExecutionId};
pub use task_domain::{ProjectId, TaskId};
