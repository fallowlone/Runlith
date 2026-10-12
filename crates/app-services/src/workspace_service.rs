//! Trusted workspace mutation service: durable lease ownership plus
//! non-destructive Git reconciliation around [`workspace::GitPort`].
//!
//! Authority: ADR-0023, `docs/architecture/workspace.md`. This service
//! coordinates the workspace domain, lease persistence, `GitPort`, journal
//! evidence, and clock/ID minting. It never runs shell commands, never
//! learns git argv, never mutates Task lifecycle or Attempt outcomes,
//! never selects providers, never merges/rebases/commits, and never performs
//! forced or dirty destruction.
//!
//! Error model: methods return [`workspace::WorkspaceFailure`] end to end
//! — the first service built on `GitPort`, whose taxonomy already spans
//! the Git failure domain, extended in TASK-0042 with `StorageFailure`
//! for the SQLite failure domain (INV-145). `ServiceError` is untouched:
//! its wire taxonomy is frozen for `bins/`.
//!
//! Cross-system rule (INV-145): SQLite and Git are separate failure
//! domains, never one ACID transaction. No SQLite transaction spans a Git
//! subprocess: the saga commits intent, runs Git, observes Git, then
//! commits the reconciled state — each SQLite commit short-lived. Partial
//! states (`SQLite ok, Git failed`; `Git ok, SQLite failed`) are
//! first-class recoverable outcomes surfaced with their evidence, never
//! reported as "nothing happened".
//!
//! Cleanup rule (TASK-0043, ADR-0023 §§5/8/9): ordinary cleanup fails
//! closed on any dirty worktree (`WorktreeDirty`), on live owners
//! (`ActiveOwnerRejectsCleanup`), and on ownership disagreement
//! (`OwnershipMismatch`); removal uses only non-forced Git operations and
//! re-validates dirtiness immediately before destruction (TOCTOU). A lease
//! that reaches `CleanupPending` and then observes dirt never implies
//! deletion — it stays `CleanupPending`, preserved, with no journal event.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use storage_sqlite::{SqliteStore, StorageError};
use task_domain::{ProjectId, TaskId};
use workspace::{
    DiffEvidence, GitPort, LeaseLifecycle, RepositoryRef, SideEffectIntent, SideEffectKind,
    SideEffectState, TransitionEvent, TrustedCommonGit, WorkspaceEventKind, WorkspaceFailure,
    WorkspaceId, WorkspaceJournalEvidence, WorkspaceLease, WorkspaceStore, WorktreeStatus,
};

use crate::clock::{format_rfc3339, Clock, SystemClock};
use crate::side_effect_recovery::{
    classify_dispatch_error, idempotency_key, materialize_pre_state, mint_intent_id,
    DispatchOutcome, WORKSPACE_CONTRACT_FINGERPRINT,
};

/// Provenance marker for every workspace fact this service commits. Fixed
/// string — never model narrative, never provider identity.
const PROVENANCE_SERVICE: &str = "workspace-application-service";

/// Process-wide lease-identity counter. Lease ids must be unique across
/// service instances: the daemon opens one `WorkspaceService` per
/// background run, so a per-instance counter collides when two runs
/// allocate in the same millisecond (TASK-0047 found two concurrent
/// executions minting the identical `ws-<millis>-1`). The pid
/// distinguishes daemon processes sharing one database file.
static LEASE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Maps a storage-local failure into the service error model.
fn store_failure(error: StorageError) -> WorkspaceFailure {
    WorkspaceFailure::StorageFailure {
        detail: error.to_string(),
    }
}

/// Maps a domain validation failure into the service error model.
fn domain_failure(error: workspace::WorkspaceError) -> WorkspaceFailure {
    WorkspaceFailure::StorageFailure {
        detail: format!("invalid workspace value: {error}"),
    }
}

use crate::managed_identity::{
    classify_persisted_identity, managed_slug, PersistedManagedIdentityFormat,
};

/// Application-owned managed workspace root: the path-ownership boundary
/// TASK-0041 deliberately left out. Every managed worktree lives at
/// `<root>/<workspace-slug>`; callers cannot turn the service into
/// "create a worktree at an arbitrary path" because no API accepts a
/// target path. Managed ownership is not a process sandbox (G6 owns
/// sandboxes): this bounds where application worktrees live, nothing more.
#[derive(Debug, Clone)]
pub struct ManagedWorkspaceRoot {
    root: PathBuf,
}

impl ManagedWorkspaceRoot {
    /// Validates an application-controlled root: absolute, an existing
    /// directory, canonicalized once so later containment checks cannot be
    /// fooled by symlinked prefixes.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, WorkspaceFailure> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "managed workspace root must be absolute: {}",
                    path.display()
                ),
            });
        }
        let root =
            std::fs::canonicalize(path).map_err(|error| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "managed workspace root unusable ({}): {error}",
                    path.display()
                ),
            })?;
        if !root.is_dir() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "managed workspace root is not a directory: {}",
                    root.display()
                ),
            });
        }
        Ok(Self { root })
    }

    /// The canonical root path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Derives `<root>/ws-<v2-slug>` for one workspace id and proves
    /// containment. Fails closed on any escape (unreachable by
    /// construction — the v2 slug holds no separators — but checked, not
    /// assumed). New allocations only: persisted leases of any version are
    /// validated by [`WorkspaceService::validate_managed_path`], never
    /// rederived here.
    pub fn join_workspace(&self, id: &WorkspaceId) -> Result<PathBuf, WorkspaceFailure> {
        let target = self.root.join(format!("ws-{}", managed_slug(id.as_str())));
        if target.strip_prefix(&self.root).is_err() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "derived workspace path escapes the managed root for {}",
                    id.as_str()
                ),
            });
        }
        Ok(target)
    }
}

/// Caller-supplied inputs for workspace allocation. The caller provides
/// execution ownership, repository identity plus its local path, and the
/// base revision request — never identities the service must mint
/// (workspace id, branch, target path) and never event metadata.
#[derive(Debug, Clone)]
pub struct WorkspaceRequest {
    /// Execution that will own the lease (must already exist).
    pub execution_id: ExecutionId,
    /// Provider-neutral repository identity (persisted as given).
    pub repository_ref: RepositoryRef,
    /// Local path of that repository for this machine (operational input,
    /// never persisted — `RepositoryRoot` is infrastructure data).
    pub repository_path: String,
    /// Base revision request (branch name, `HEAD`, or OID); the service
    /// resolves it to an immutable OID before persisting anything.
    pub base_revision: String,
}

/// How `ensure_workspace_ready` reached its outcome. Every variant is
/// observable in durable state plus the journal — never a bare claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureOutcome {
    /// No lease existed: allocated, materialized, and finalized now.
    CreatedFresh,
    /// An `Allocated` lease existed with no Git side effects: creation
    /// resumed and finalized now (crash-A recovery).
    ResumedAllocated,
    /// An `Allocated` lease existed with exactly its Git state present:
    /// recognized and finalized without new Git mutation (crash-B
    /// recovery). Exactly-once `workspace.created` preserved by the
    /// guarded transition.
    RecognizedGitState,
    /// A provably-owned branch-only crash strand was continued using the
    /// existing managed branch, then finalized exactly once.
    RecoveredBranchOnly,
    /// A `Ready` lease with healthy Git state already existed: no Git
    /// mutation, no event, no state write.
    AlreadyReady,
}

/// The reconciled lease plus how it was reached.
#[derive(Debug, Clone)]
pub struct EnsuredWorkspace {
    /// Current durable lease after the call.
    pub lease: WorkspaceLease,
    /// Which path produced it.
    pub outcome: EnsureOutcome,
}

/// How `cleanup_workspace` reached its outcome. Every variant is
/// observable in durable state plus the journal — never a bare claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupOutcome {
    /// Worktree plus managed branch are gone; the lease moved
    /// `CleanupPending → Removed` with exactly one `workspace.removed`.
    RemovedFresh,
    /// The lease was already `Removed` with its Git resources absent and
    /// its `workspace.removed` fact present: no Git mutation, no event,
    /// no state write.
    AlreadyRemoved,
}

/// The reconciled lease plus how cleanup reached it.
#[derive(Debug, Clone)]
pub struct CleanedWorkspace {
    /// Current durable lease after the call.
    pub lease: WorkspaceLease,
    /// Which path produced it.
    pub outcome: CleanupOutcome,
}

/// Trusted owner of durable workspace facts. Generic over the Git backend
/// ([`GitPort`]) for testability and over the clock like the other
/// services; the store is the concrete `SqliteStore` like the other
/// services.
pub struct WorkspaceService<G: GitPort, C: Clock = SystemClock> {
    store: SqliteStore,
    git: G,
    root: ManagedWorkspaceRoot,
    clock: C,
    op_counter: u64,
}

impl<G: GitPort, C: Clock> WorkspaceService<G, C> {
    /// Loads the durable trusted anchor, failing closed when absent.
    /// No backfill from current FS: missing rows (pre-existing lease,
    /// machine move, loss) refuse every bound operation until re-admission
    /// with a new lease.
    fn load_anchor(&self, id: &WorkspaceId) -> Result<TrustedCommonGit, WorkspaceFailure> {
        match self.store.get_trusted_anchor(id)? {
            Some(anchor) => Ok(anchor),
            None => Err(WorkspaceFailure::GitBackendFailure {
                detail: format!(
                    "lease {} has no trusted Git anchor; re-admission with a new lease is required",
                    id.as_str()
                ),
            }),
        }
    }

    fn classify_lease_identity(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<PersistedManagedIdentityFormat, WorkspaceFailure> {
        let format = classify_persisted_identity(
            self.root.path(),
            lease.owner(),
            lease.id(),
            lease.branch(),
            lease.worktree_ref(),
        )?;
        match self.store.find_identity_reservation_branch(lease.owner())? {
            Some(reserved) if reserved != lease.branch().as_str() => {
                Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "lease {} branch {} disagrees with durable reservation {}; refusing",
                        lease.id().as_str(),
                        lease.branch().as_str(),
                        reserved
                    ),
                })
            }
            Some(_) => Ok(format),
            None if format == PersistedManagedIdentityFormat::CurrentV2 => {
                Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "current-v2 lease {} has no durable managed-identity reservation; refusing",
                        lease.id().as_str()
                    ),
                })
            }
            None => Ok(format),
        }
    }

    /// Opens (creating/migrating) the database at `path` with an explicit
    /// Git backend and managed root.
    pub fn open(
        path: impl AsRef<Path>,
        git: G,
        root: ManagedWorkspaceRoot,
        clock: C,
    ) -> Result<Self, WorkspaceFailure> {
        Ok(Self::attach(
            SqliteStore::open(path).map_err(store_failure)?,
            git,
            root,
            clock,
        ))
    }

    /// Wraps an already-open store, backend, and root (setup fixtures,
    /// multi-connection tests). The store must be migrated, which
    /// [`SqliteStore::open`] guarantees.
    pub fn attach(store: SqliteStore, git: G, root: ManagedWorkspaceRoot, clock: C) -> Self {
        Self {
            store,
            git,
            root,
            clock,
            op_counter: 0,
        }
    }

    /// Read access to the underlying store (current-state and journal
    /// queries for callers/tests).
    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    /// The managed root this service places worktrees under.
    pub fn managed_root(&self) -> &ManagedWorkspaceRoot {
        &self.root
    }

    /// Reads the single lease owned by `execution_id`, if any. Read-only:
    /// emits no journal evidence. Two attempts of one execution resolve
    /// through this to the same lease — attempts never own workspaces.
    pub fn workspace_for_execution(
        &self,
        execution_id: &ExecutionId,
    ) -> Result<Option<WorkspaceLease>, WorkspaceFailure> {
        self.store.get_workspace_for_execution(execution_id)
    }

    /// Reads one lease by identity, if any. Read-only.
    pub fn workspace(&self, id: &WorkspaceId) -> Result<Option<WorkspaceLease>, WorkspaceFailure> {
        self.store.get_workspace(id)
    }

    /// Runs the TASK-0135 (G8) prerequisite preflight for one lease:
    /// read-only validation of the task's declared
    /// [`task_domain::Prerequisite`]s against the live repository
    /// environment, bound to the lease's durable trusted Git anchor.
    ///
    /// Authority boundary: this informs, it never decides. The report is
    /// returned to the caller (the orchestrator), which alone may map
    /// `Failed` findings onto the journaled task `Block` transition;
    /// nothing is persisted or transitioned here.
    pub fn check_task_prerequisites(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        prerequisites: &[task_domain::Prerequisite],
    ) -> Result<crate::preflight_service::PreflightReport, WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        Ok(crate::preflight_service::check_prerequisites(
            &self.git,
            repository_path,
            &anchor,
            prerequisites,
        ))
    }

    /// Observes the canonical diff for one lease, baselined at its recorded
    /// `base_revision` (ADR-0023 §6). Read-only: emits no journal evidence,
    /// mutates no Git state, completes no Task. The returned evidence
    /// carries the authoritative workspace id, base, and current HEAD.
    pub fn compute_diff(&self, id: &WorkspaceId) -> Result<DiffEvidence, WorkspaceFailure> {
        let lease =
            self.store
                .get_workspace(id)?
                .ok_or_else(|| WorkspaceFailure::StorageFailure {
                    detail: format!("lease {} does not exist for diff", id.as_str()),
                })?;
        self.validate_managed_path(&lease)?;
        let anchor = self.load_anchor(lease.id())?;
        let evidence = self.git.compute_diff(&lease, &anchor)?;
        if evidence.diff.base_revision != *lease.base_revision() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} diff baseline drifted from recorded base; refusing",
                    id.as_str()
                ),
            });
        }
        Ok(evidence)
    }

    /// Requests teardown (`Ready → CleanupPending`). Operational state only:
    /// no journal event, authorizes nothing destructive, performs no Git
    /// mutation. Fails unless currently `Ready`.
    pub fn request_cleanup(
        &mut self,
        id: &WorkspaceId,
    ) -> Result<WorkspaceLease, WorkspaceFailure> {
        self.store.mark_lease_cleanup_pending(id)?;
        self.store
            .get_workspace(id)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!("lease {} vanished during cleanup request", id.as_str()),
            })
    }

    /// Full safe-cleanup saga for one lease, addressed by [`WorkspaceId`]
    /// ownership — never by caller-supplied path.
    ///
    /// Dirtiness is checked before `Ready → CleanupPending` (so a dirty
    /// `Ready` lease stays `Ready` with nothing written) and re-checked
    /// immediately before destructive removal (TOCTOU): a workspace that
    /// becomes dirty while `CleanupPending` stays `CleanupPending`,
    /// preserved, with no journal event and no `Removed` implication.
    /// There is no `CleanupPending → Ready` transition; a blocked
    /// `CleanupPending` lease remains explicitly blocked, never silently
    /// re-armed and never implying deletion.
    ///
    /// Ordering per ADR-0023 §8 (INV-145): every SQLite commit is
    /// short-lived and no SQLite transaction spans a Git subprocess —
    /// validate, commit `CleanupPending`, remove the worktree safely,
    /// observe, remove the managed branch only when safely authorized,
    /// observe, then commit `Removed` with `workspace.removed` atomically.
    /// `workspace.removed` is emitted only at `CleanupPending → Removed`,
    /// exactly once via the guarded store transition.
    pub fn cleanup_workspace(
        &mut self,
        id: &WorkspaceId,
        repository_path: &str,
    ) -> Result<CleanedWorkspace, WorkspaceFailure> {
        let lease =
            self.store
                .get_workspace(id)?
                .ok_or_else(|| WorkspaceFailure::StorageFailure {
                    detail: format!("lease {} does not exist for cleanup", id.as_str()),
                })?;
        match lease.lifecycle() {
            LeaseLifecycle::Allocated => {
                return Err(WorkspaceFailure::StorageFailure {
                    detail: format!(
                        "lease {} is Allocated (creation intent, not a ready workspace); \
                         cleanup does not apply",
                        id.as_str()
                    ),
                });
            }
            LeaseLifecycle::Removed => {
                return self.already_removed_noop(&lease, repository_path);
            }
            LeaseLifecycle::Ready | LeaseLifecycle::CleanupPending => {}
        }
        // Ownership first: the derived managed path must equal the durable
        // worktree reference, or nothing below may run.
        self.validate_managed_path(&lease)?;
        self.validate_cleanup_repository(&lease, repository_path)?;
        self.reject_live_owner(&lease)?;
        self.reject_protected_branch(&lease, repository_path)?;
        let anchor = self.load_anchor(lease.id())?;

        // Dirtiness before CleanupPending: a dirty Ready lease stays Ready.
        // Lease-to-repository binding (TASK-0046, G4 finding F1) is proven
        // per observed shape, before any mutation:
        // - present worktree → positive `worktree list` registration in the
        //   caller-supplied repository (a wrong-but-valid path fails here);
        // - absent worktree + CleanupPending → history binding below (the
        //   legitimate crash-continuation shape: no worktree left to
        //   register anywhere);
        // - absent worktree + Ready → drift error (read-only, no binding
        //   claim needed, nothing mutated);
        // - unusable worktree → inspection error, unchanged behavior.
        match self.git.worktree_status(lease.worktree_ref(), &anchor)? {
            WorktreeStatus::Clean => {
                self.validate_worktree_binding(&lease, repository_path)?;
            }
            WorktreeStatus::DirtyTracked | WorktreeStatus::DirtyUntracked => {
                self.validate_worktree_binding(&lease, repository_path)?;
                return Err(WorkspaceFailure::WorktreeDirty {
                    detail: format!(
                        "lease {} holds uncommitted work; ordinary cleanup refuses removal",
                        id.as_str()
                    ),
                });
            }
            WorktreeStatus::Missing => {
                if lease.lifecycle() == LeaseLifecycle::Ready {
                    // No cleanup intent exists: unexpected absence is drift,
                    // never success. No state change, no event.
                    return Err(WorkspaceFailure::WorktreeMissing {
                        detail: format!(
                            "lease {} is Ready but its worktree is absent; drift, not cleanup",
                            id.as_str()
                        ),
                    });
                }
                // CleanupPending + absent: legitimate reconciliation shape —
                // prove history binding, then continue to branch handling
                // below (no worktree to remove).
                self.validate_history_binding(&lease, repository_path)?;
                return self.finish_cleanup_pending(&lease, repository_path, true);
            }
            WorktreeStatus::Invalid => {
                return Err(WorkspaceFailure::CheckoutFailed {
                    detail: format!(
                        "lease {} worktree is unusable; refusing cleanup without inspection",
                        id.as_str()
                    ),
                });
            }
        }

        // Commit the operational intent (Ready → CleanupPending) before any
        // Git mutation. Already-CleanupPending leases skip this write.
        let lease = if lease.lifecycle() == LeaseLifecycle::Ready {
            self.store.mark_lease_cleanup_pending(id)?;
            self.store
                .get_workspace(id)?
                .ok_or_else(|| WorkspaceFailure::StorageFailure {
                    detail: format!("lease {} vanished during cleanup request", id.as_str()),
                })?
        } else {
            lease
        };
        self.finish_cleanup_pending(&lease, repository_path, false)
    }

    /// Full idempotent saga: allocate intent → Git mutation → observed
    /// finalization, routed through reconciliation when a lease already
    /// exists. Safe to call repeatedly: a healthy workspace returns
    /// `AlreadyReady` with no new branch, worktree, or event.
    pub fn ensure_workspace_ready(
        &mut self,
        request: &WorkspaceRequest,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        match self
            .store
            .get_workspace_for_execution(&request.execution_id)?
        {
            None => {
                let lease = self.allocate_workspace(request)?;
                self.materialize_allocated(&lease, &request.repository_path)?;
                let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
                Ok(EnsuredWorkspace {
                    lease: ready,
                    outcome: EnsureOutcome::CreatedFresh,
                })
            }
            Some(lease) => self.reconcile_existing(lease, request),
        }
    }

    /// Idempotent terminal read: a `Removed` lease with its Git resources
    /// absent and its `workspace.removed` fact present is healthy history —
    /// no Git mutation, no event, no state write. A `Removed` lease with a
    /// live worktree or branch is corruption/drift: never resurrected,
    /// never blindly deleted.
    fn already_removed_noop(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<CleanedWorkspace, WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        if !self
            .store
            .has_workspace_event(lease.id(), WorkspaceEventKind::Removed)?
        {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is Removed without workspace.removed evidence",
                    lease.id().as_str()
                ),
            });
        }
        // No lease-to-repository binding probe here by design: a healthy
        // `Removed` lease has no worktree left to register anywhere, so
        // registration is unprovable either way. This is benign post-F1:
        // `Removed` is reachable only through `finish_cleanup_pending`,
        // whose branch-absence gate ran under the entry binding check, so
        // a wrong-repo `AlreadyRemoved` can no longer mask a leak created
        // by this service. A leaked branch in the true repository still
        // surfaces loudly as `CheckoutFailed` corruption below.
        match self.git.worktree_status(lease.worktree_ref(), &anchor)? {
            WorktreeStatus::Missing => {}
            _ => {
                return Err(WorkspaceFailure::CheckoutFailed {
                    detail: format!(
                        "lease {} is Removed but its worktree still exists; \
                         corruption, refusing resurrection or deletion",
                        lease.id().as_str()
                    ),
                });
            }
        }
        if self
            .git
            .branch_exists(repository_path, lease.branch(), &anchor)?
        {
            return Err(WorkspaceFailure::CheckoutFailed {
                detail: format!(
                    "lease {} is Removed but its managed branch still exists; \
                     corruption, refusing resurrection or deletion",
                    lease.id().as_str()
                ),
            });
        }
        Ok(CleanedWorkspace {
            lease: lease.clone(),
            outcome: CleanupOutcome::AlreadyRemoved,
        })
    }

    /// Continues a `CleanupPending` lease through Git removal and DB
    /// finalization. `worktree_known_absent` skips the worktree-removal
    /// call (the path is already gone); branch handling still runs, so a
    /// stale registration surfaces as a refused branch deletion rather
    /// than silent success.
    fn finish_cleanup_pending(
        &mut self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_known_absent: bool,
    ) -> Result<CleanedWorkspace, WorkspaceFailure> {
        debug_assert_eq!(lease.lifecycle(), LeaseLifecycle::CleanupPending);
        let anchor = self.load_anchor(lease.id())?;
        if !worktree_known_absent {
            // TOCTOU re-check immediately before destruction: preflight
            // cleanliness may have changed since the entry check.
            match self.git.worktree_status(lease.worktree_ref(), &anchor)? {
                WorktreeStatus::Clean => {}
                WorktreeStatus::DirtyTracked | WorktreeStatus::DirtyUntracked => {
                    return Err(WorkspaceFailure::WorktreeDirty {
                        detail: format!(
                            "lease {} became dirty before removal; cleanup stops, work preserved",
                            lease.id().as_str()
                        ),
                    });
                }
                WorktreeStatus::Missing => {}
                WorktreeStatus::Invalid => {
                    return Err(WorkspaceFailure::CheckoutFailed {
                        detail: format!(
                            "lease {} worktree is unusable; refusing cleanup without inspection",
                            lease.id().as_str()
                        ),
                    });
                }
            }
            // Re-validate ownership immediately before the mutation: the
            // persisted path is not permission by itself.
            self.validate_managed_path(lease)?;
            match self
                .git
                .remove_worktree(lease, lease.worktree_ref(), &anchor)
            {
                Ok(()) => {}
                Err(WorkspaceFailure::WorktreeMissing { .. })
                | Err(WorkspaceFailure::AlreadyAbsent) => {}
                Err(other) => return Err(other),
            }
            match self.git.worktree_status(lease.worktree_ref(), &anchor)? {
                WorktreeStatus::Missing => {}
                WorktreeStatus::DirtyTracked | WorktreeStatus::DirtyUntracked => {
                    return Err(WorkspaceFailure::WorktreeDirty {
                        detail: format!(
                            "lease {} worktree still holds work after removal attempt; preserving",
                            lease.id().as_str()
                        ),
                    });
                }
                _ => {
                    return Err(WorkspaceFailure::GitBackendFailure {
                        detail: format!(
                            "git backend: worktree removal postcondition failed for lease {}",
                            lease.id().as_str()
                        ),
                    });
                }
            }
        }
        // Managed-branch follow-up: only the recorded lease branch, only
        // non-forced deletion. A branch with unique unmerged commits is
        // preserved (`CleanupUnsafe`): the lease stays `CleanupPending`
        // with no event — worktree gone, branch evidence kept for future
        // delivery, never force-deleted.
        if self
            .git
            .branch_exists(repository_path, lease.branch(), &anchor)?
        {
            match self
                .git
                .remove_branch(repository_path, lease.branch(), &anchor)
            {
                Ok(()) => {}
                Err(WorkspaceFailure::AlreadyAbsent) => {}
                Err(other) => return Err(other),
            }
            if self
                .git
                .branch_exists(repository_path, lease.branch(), &anchor)?
            {
                return Err(WorkspaceFailure::CleanupUnsafe {
                    detail: format!(
                        "lease {} branch {} preserved; refusing to discard its commits",
                        lease.id().as_str(),
                        lease.branch().as_str()
                    ),
                });
            }
        }
        // Finalization gate: both Git resources must be in their
        // contract-defined absent state before `workspace.removed`.
        match self.git.worktree_status(lease.worktree_ref(), &anchor)? {
            WorktreeStatus::Missing => {}
            _ => {
                return Err(WorkspaceFailure::GitBackendFailure {
                    detail: format!(
                        "git backend: worktree still present before finalization for lease {}",
                        lease.id().as_str()
                    ),
                });
            }
        }
        if self
            .git
            .branch_exists(repository_path, lease.branch(), &anchor)?
        {
            return Err(WorkspaceFailure::CleanupUnsafe {
                detail: format!(
                    "lease {} branch {} still present; refusing finalization",
                    lease.id().as_str(),
                    lease.branch().as_str()
                ),
            });
        }
        let execution = self
            .store
            .get_execution(lease.owner())
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} references missing execution {}",
                    lease.id().as_str(),
                    lease.owner().as_str()
                ),
            })?;
        let task = self
            .store
            .get_task(execution.task_id())
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "execution {} references missing task {}",
                    lease.owner().as_str(),
                    execution.task_id().as_str()
                ),
            })?;
        let evidence = self.mint_evidence(
            WorkspaceEventKind::Removed,
            task.id(),
            task.project_id(),
            &execution,
            lease,
            "cleanup_workspace",
        );
        self.store.mark_lease_removed(lease.id(), &evidence)?;
        let removed = self.store.get_workspace(lease.id())?.ok_or_else(|| {
            WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} vanished during removal finalization",
                    lease.id().as_str()
                ),
            }
        })?;
        Ok(CleanedWorkspace {
            lease: removed,
            outcome: CleanupOutcome::RemovedFresh,
        })
    }

    /// Re-validates managed-path containment before any destructive action.
    /// The persisted worktree reference is not permission by itself, and it
    /// is never rederived as authority: current-v2 leases are proven from
    /// the complete persisted branch/path pair plus durable reservation and
    /// must equal the rederived `<root>/ws-<v2-slug>`; pre-v2 leases keep their persisted reference
    /// byte-for-byte (TASK-0053 §13 — old branches/paths are historical
    /// artifacts, never renamed) and prove containment only. A live path
    /// must additionally canonicalize inside the managed root. Traversal or
    /// symlink escape fails closed on either path.
    fn validate_managed_path(&self, lease: &WorkspaceLease) -> Result<PathBuf, WorkspaceFailure> {
        let mismatch = || WorkspaceFailure::OwnershipMismatch {
            detail: format!(
                "lease {} worktree reference disagrees with the managed root; refusing",
                lease.id().as_str()
            ),
        };
        // Basename discipline holds for every version: the persisted
        // reference must name a direct `ws-…` child of the managed root —
        // never a traversal, never an absolute escape.
        let file_name = Path::new(lease.worktree_ref())
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(mismatch)?;
        if !file_name.starts_with("ws-") {
            return Err(mismatch());
        }
        let expected = match self.classify_lease_identity(lease)? {
            PersistedManagedIdentityFormat::CurrentV2 => self.root.join_workspace(lease.id())?,
            PersistedManagedIdentityFormat::Legacy => {
                // Compatibility path: the persisted reference must still spell
                // a direct child of this root (lexical), or containment below
                // proves nothing.
                let persisted = PathBuf::from(lease.worktree_ref());
                if persisted.parent() != Some(self.root.path()) {
                    return Err(mismatch());
                }
                persisted
            }
        };
        let path = PathBuf::from(lease.worktree_ref());
        if path.symlink_metadata().is_ok() {
            let canonical =
                std::fs::canonicalize(&path).map_err(|_| WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "lease {} worktree path cannot prove managed containment; refusing",
                        lease.id().as_str()
                    ),
                })?;
            if canonical.strip_prefix(self.root.path()).is_err() {
                return Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "lease {} worktree path escapes the managed root; refusing",
                        lease.id().as_str()
                    ),
                });
            }
            return Ok(canonical);
        }
        Ok(expected)
    }

    /// Validates the operational repository context for cleanup: it must be
    /// a working repository, and a live worktree path must not equal or
    /// nest inside the primary checkout (deleting the primary is never
    /// cleanup).
    fn validate_cleanup_repository(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        if repository_path.trim().is_empty() {
            return Err(WorkspaceFailure::NotRepository {
                detail: "workspace cleanup requires a repository path".to_owned(),
            });
        }
        if !self.git.is_repository(repository_path, &anchor)? {
            return Err(WorkspaceFailure::NotRepository {
                detail: format!("workspace cleanup path is not a repository: {repository_path}"),
            });
        }
        let worktree = PathBuf::from(lease.worktree_ref());
        if worktree.symlink_metadata().is_ok() {
            let repo_canon = std::fs::canonicalize(repository_path).map_err(|_| {
                WorkspaceFailure::NotRepository {
                    detail: "workspace cleanup repository path unusable".to_owned(),
                }
            })?;
            // Resolve the authoritative primary root through Git (macOS
            // symlinks canonicalized) rather than trusting the caller path
            // spelling for the nesting comparison.
            let primary_canon = self
                .git
                .resolve_revision(repository_path, "HEAD", &anchor)
                .ok()
                .and_then(|_| std::fs::canonicalize(&repo_canon).ok())
                .unwrap_or(repo_canon);
            if let Ok(worktree_canon) = std::fs::canonicalize(&worktree) {
                if worktree_canon == primary_canon
                    || worktree_canon.strip_prefix(&primary_canon).is_ok()
                {
                    return Err(WorkspaceFailure::OwnershipMismatch {
                        detail: format!(
                            "lease {} worktree nests inside the primary checkout; refusing",
                            lease.id().as_str()
                        ),
                    });
                }
            }
        }
        // Only service-derived `agent/` branches are ever deleted. The
        // scope predicate lives on the domain type (`BranchRef::is_managed`,
        // TASK-0046 interpretation B): untrusted/provider input can parse
        // as a valid ref yet never count as managed.
        if !lease.branch().is_managed() {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} branch {} is outside managed ownership; refusing",
                    lease.id().as_str(),
                    lease.branch().as_str()
                ),
            });
        }
        Ok(())
    }

    /// Positive lease-to-repository binding for a present worktree
    /// (TASK-0046, G4 finding F1): the caller-supplied repository must
    /// register this lease's worktree before any destructive step. A
    /// wrong-but-valid path fails closed here — before worktree removal —
    /// instead of finalizing success while the managed branch leaks in
    /// the true repository.
    fn validate_worktree_binding(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        if !self
            .git
            .is_registered_worktree(repository_path, lease.worktree_ref(), &anchor)?
        {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} worktree is not registered in repository {}; refusing",
                    lease.id().as_str(),
                    repository_path
                ),
            });
        }
        Ok(())
    }

    /// History binding for the legitimate absent-worktree continuation
    /// (CleanupPending lease, worktree already gone): no worktree exists
    /// to register anywhere, so the caller-supplied repository must
    /// instead prove it shares the lease's history by resolving the
    /// immutable recorded base commit. A wrong-but-valid (unrelated)
    /// repository cannot resolve that OID and fails closed, while the
    /// true repository — which still holds its own base — continues
    /// crash recovery. No Git mutation happens on either path.
    fn validate_history_binding(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        match self
            .git
            .resolve_revision(repository_path, lease.base_revision().as_str(), &anchor)
        {
            Ok(resolved) if resolved == *lease.base_revision() => Ok(()),
            _ => Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} base revision is unknown in repository {}; refusing",
                    lease.id().as_str(),
                    repository_path
                ),
            }),
        }
    }

    /// Rejects cleanup while the owning execution is known live.
    fn reject_live_owner(&self, lease: &WorkspaceLease) -> Result<(), WorkspaceFailure> {
        if self.git.owner_is_live(lease.repository(), lease.owner())? {
            return Err(WorkspaceFailure::ActiveOwnerRejectsCleanup {
                detail: format!(
                    "lease {} owner {} is live; refusing cleanup of a live execution workspace",
                    lease.id().as_str(),
                    lease.owner().as_str()
                ),
            });
        }
        Ok(())
    }

    /// Refuses to delete the protected/default branch (resolved from
    /// repository metadata, never by name). Indeterminate metadata does not
    /// block: the managed `agent/` prefix above already scopes deletion.
    fn reject_protected_branch(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        match self.git.default_branch(repository_path, &anchor) {
            Ok(default_ref) => {
                if default_ref == lease.branch().as_str() {
                    return Err(WorkspaceFailure::CleanupUnsafe {
                        detail: format!(
                            "lease {} branch {} is the protected/default branch; never deleted",
                            lease.id().as_str(),
                            lease.branch().as_str()
                        ),
                    });
                }
                Ok(())
            }
            Err(WorkspaceFailure::GitBackendFailure { .. }) => Ok(()),
            Err(other) => Err(other),
        }
    }
    /// Step 1–2 of the saga (crash-A seam): validate the request, resolve
    /// the base OID (read-only Git observation, no mutation), derive the
    /// managed branch and root-bound target, then commit the `Allocated`
    /// intent. The only SQLite write here is the intent row; the only Git
    /// calls are read-only. Stopping after this call leaves exactly a
    /// durable `Allocated` lease with no side effects.
    pub fn allocate_workspace(
        &mut self,
        request: &WorkspaceRequest,
    ) -> Result<WorkspaceLease, WorkspaceFailure> {
        let execution = self
            .store
            .get_execution(&request.execution_id)
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "execution {} does not exist for workspace allocation",
                    request.execution_id.as_str()
                ),
            })?;
        if self
            .store
            .get_workspace_for_execution(&request.execution_id)?
            .is_some()
        {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "execution {} already owns a workspace lease",
                    request.execution_id.as_str()
                ),
            });
        }
        // Trusted scope derivation (Execution → Task → Project): the
        // caller supplies no project/task scope, so contradictory
        // ownership is unrepresentable. Existence is proven now (fail
        // fast on a dangling execution); Task/Project facts are never
        // copied into the lease — the chain stays canonical, and journal
        // scope is derived again at finalization.
        let _task = self
            .store
            .get_task(execution.task_id())
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "execution {} references missing task {}",
                    request.execution_id.as_str(),
                    execution.task_id().as_str()
                ),
            })?;
        if request.repository_path.trim().is_empty() {
            return Err(WorkspaceFailure::NotRepository {
                detail: "workspace allocation requires a repository path".to_owned(),
            });
        }
        // Trusted admission capture (trusted-deposit before provider
        // execution): establish the anchor first via the sole anchor-free
        // path, then bind every subsequent observation to it. Git
        // observations precede the single short SQLite transaction below
        // (INV-145: no SQLite transaction spans a Git subprocess).
        let anchor = self
            .git
            .establish_trusted_anchor(&request.repository_path)?;
        if !self.git.is_repository(&request.repository_path, &anchor)? {
            return Err(WorkspaceFailure::NotRepository {
                detail: format!(
                    "workspace allocation path is not a repository: {}",
                    request.repository_path
                ),
            });
        }
        let base =
            self.git
                .resolve_revision(&request.repository_path, &request.base_revision, &anchor)?;
        let now = self.clock.now_millis();
        self.op_counter += 1;
        let id = WorkspaceId::new(format!(
            "ws-{now}-{}-{}",
            std::process::id(),
            LEASE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
        .map_err(domain_failure)?;
        let branch = crate::managed_identity::branch_for_execution(&request.execution_id)?;
        let target = self.root.join_workspace(&id)?;
        if target.symlink_metadata().is_ok() {
            return Err(WorkspaceFailure::WorktreeCollision {
                detail: format!(
                    "derived workspace target already exists; refusing overwrite: {}",
                    target.display()
                ),
            });
        }
        let target_text = target
            .to_str()
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: "derived workspace target is not UTF-8".to_owned(),
            })?;
        let lease = WorkspaceLease::allocate(
            id,
            request.repository_ref.clone(),
            request.execution_id.clone(),
            base,
            branch,
            target_text.to_owned(),
        )
        .map_err(domain_failure)?;
        self.store
            .insert_allocated_lease_with_anchor(&lease, &anchor, now)?;
        Ok(lease)
    }

    /// Step 3 of the saga (crash-A/B seam): run the single Git mutation
    /// for an `Allocated` lease. No SQLite is touched here — not even a
    /// read transaction spanning the subprocess — so a crash inside Git
    /// leaves exactly the durable intent plus whatever Git did, both
    /// reconcilable. Stale lease handles (superseded objects) are refused
    /// before any mutation.
    pub fn materialize_allocated(
        &mut self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        if lease.lifecycle() != LeaseLifecycle::Allocated {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "materialize requires an Allocated lease; {} is {}",
                    lease.id().as_str(),
                    lease.lifecycle()
                ),
            });
        }
        let stored = self.store.get_workspace(lease.id())?.ok_or_else(|| {
            WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} vanished before materialization",
                    lease.id().as_str()
                ),
            }
        })?;
        if stored != *lease {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "stale lease handle for {}; re-read before materializing",
                    lease.id().as_str()
                ),
            });
        }
        let target_text = self.materialize_target_text(lease)?;
        let anchor = self.load_anchor(lease.id())?;
        // SE-1/SE-3 gate: a stranded non-terminal intent refuses blind
        // re-dispatch — reconcile through `ensure_workspace_ready`
        // first. Terminal intents (and legacy leases without one) open
        // a fresh lineage below.
        let intent = match self.active_materialize_intent(lease)? {
            None => Some(self.record_materialize_intent(
                lease,
                repository_path,
                &target_text,
                &anchor,
            )?),
            Some(intent)
                if matches!(
                    intent.state(),
                    SideEffectState::Confirmed | SideEffectState::Failed
                ) =>
            {
                Some(self.record_materialize_intent(
                    lease,
                    repository_path,
                    &target_text,
                    &anchor,
                )?)
            }
            Some(intent) if intent.state() == SideEffectState::IntentRecorded => Some(intent),
            Some(intent) => {
                return Err(WorkspaceFailure::ReconciliationRequired {
                    detail: format!(
                        "lease {} holds a {:?} materialize intent; reconcile via \
                         ensure_workspace_ready before any retry",
                        lease.id().as_str(),
                        intent.state()
                    ),
                });
            }
        };
        let intent = intent.ok_or_else(|| WorkspaceFailure::StorageFailure {
            detail: format!(
                "materialize intent for lease {} vanished",
                lease.id().as_str()
            ),
        })?;
        self.transition_intent(&intent, TransitionEvent::Dispatch, &[])?;
        self.dispatch_materialize_effect(lease, repository_path, &target_text, &anchor, &intent)
    }

    /// Runs the fused Git dispatch for an EXECUTING materialize intent
    /// and records the classified outcome (SE-2/SE-5). Shared by first
    /// dispatch and crash-A re-dispatch; reconcile arms drive the
    /// machine to EXECUTING before calling.
    fn dispatch_materialize_effect(
        &mut self,
        lease: &WorkspaceLease,
        repository_path: &str,
        target_text: &str,
        anchor: &TrustedCommonGit,
        intent: &SideEffectIntent,
    ) -> Result<(), WorkspaceFailure> {
        match self
            .git
            .create_worktree(lease, repository_path, target_text, anchor)
        {
            Ok(()) => {
                self.transition_intent(
                    intent,
                    TransitionEvent::Confirm,
                    &["adapter-postconditions-verified".to_owned()],
                )?;
                Ok(())
            }
            Err(error) => {
                match classify_dispatch_error(&error) {
                    DispatchOutcome::Failed => {
                        self.transition_intent(
                            intent,
                            TransitionEvent::Fail,
                            &[format!("dispatch-refused:{}", error.kind())],
                        )?;
                    }
                    DispatchOutcome::Ambiguous => {
                        self.transition_intent(
                            intent,
                            TransitionEvent::MarkAmbiguous,
                            &[format!("unknown-dispatch-outcome:{}", error.kind())],
                        )?;
                    }
                }
                Err(error)
            }
        }
    }

    /// Derives the Git target path for a lease under this root,
    /// refusing when the persisted reference proves the root moved.
    fn materialize_target_text(&self, lease: &WorkspaceLease) -> Result<String, WorkspaceFailure> {
        // Root-stability gate: the persisted worktree reference must still
        // resolve under this root. Current-v2 leases rederive exactly; pre-v2
        // leases (persisted byte-for-byte per §13) prove the lexical child
        // relationship instead of being rederived under the new scheme.
        match self.classify_lease_identity(lease)? {
            PersistedManagedIdentityFormat::CurrentV2 => self
                .root
                .join_workspace(lease.id())?
                .to_str()
                .ok_or_else(|| WorkspaceFailure::StorageFailure {
                    detail: "derived workspace target is not UTF-8".to_owned(),
                })
                .map(str::to_owned),
            PersistedManagedIdentityFormat::Legacy => {
                let persisted = PathBuf::from(lease.worktree_ref());
                if persisted.parent() != Some(self.root.path()) {
                    return Err(WorkspaceFailure::StorageFailure {
                        detail: format!(
                            "managed root changed under lease {}; refusing",
                            lease.id().as_str()
                        ),
                    });
                }
                Ok(lease.worktree_ref().to_owned())
            }
        }
    }

    /// Records the materialize intent for a lease (SE-1): durable
    /// before any Git dispatch, with the observed pre-effect target
    /// reading when observation succeeds.
    fn record_materialize_intent(
        &mut self,
        lease: &WorkspaceLease,
        repository_path: &str,
        target_text: &str,
        anchor: &TrustedCommonGit,
    ) -> Result<SideEffectIntent, WorkspaceFailure> {
        let execution = self
            .store
            .get_execution(lease.owner())
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "execution {} does not exist for lease {}",
                    lease.owner().as_str(),
                    lease.id().as_str()
                ),
            })?;
        let now = self.clock.now_millis();
        let branch_absent = self
            .git
            .branch_exists(repository_path, lease.branch(), anchor)
            .map(|exists| !exists)
            .ok();
        let path_absent = std::fs::symlink_metadata(target_text).is_err();
        let intent = SideEffectIntent::new(workspace::SideEffectIntentFields {
            intent_id: mint_intent_id(now),
            kind: SideEffectKind::WorktreeMaterialize,
            target_ref: lease.id().as_str().to_owned(),
            params_hash: workspace::materialize_params_hash(
                lease.branch().as_str(),
                target_text,
                lease.base_revision().as_str(),
                lease.id().as_str(),
                lease.owner().as_str(),
            ),
            idempotency_key: idempotency_key(
                lease.id().as_str(),
                SideEffectKind::WorktreeMaterialize,
            ),
            task_id: execution.task_id().as_str().to_owned(),
            execution_id: lease.owner().as_str().to_owned(),
            lease_id: lease.id().as_str().to_owned(),
            pre_state_fingerprint: branch_absent
                .map(|absent| materialize_pre_state(absent, path_absent)),
            policy_fingerprint: WORKSPACE_CONTRACT_FINGERPRINT.to_owned(),
        })
        .map_err(|error| WorkspaceFailure::StorageFailure {
            detail: format!("side-effect intent malformed: {error}"),
        })?;
        let at_ms = i64::try_from(now).unwrap_or(i64::MAX);
        self.store
            .record_side_effect_intent(&intent, at_ms)
            .map_err(|error| match &error {
                // Slot race: a concurrent creator holds the active
                // intent — reconcile, never double-dispatch.
                storage_sqlite::StorageError::ConstraintViolation(_) => {
                    WorkspaceFailure::ReconciliationRequired {
                        detail: format!(
                            "lease {} already holds an active materialize intent; reconcile first",
                            lease.id().as_str()
                        ),
                    }
                }
                _ => store_failure(error),
            })
    }

    /// Reads the active materialize intent for a lease, if any.
    fn active_materialize_intent(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<Option<SideEffectIntent>, WorkspaceFailure> {
        self.store
            .get_side_effect_intent_for_lease(
                lease.id().as_str(),
                SideEffectKind::WorktreeMaterialize,
            )
            .map_err(store_failure)
    }

    /// Whether the lease's materialize escalation (if any) is resolved.
    fn materialize_escalation_resolved(
        &self,
        intent: &SideEffectIntent,
    ) -> Result<bool, WorkspaceFailure> {
        Ok(self
            .store
            .get_side_effect_escalation(intent.intent_id())
            .map_err(store_failure)?
            .is_some_and(|escalation| escalation.resolved_at_ms.is_some()))
    }

    /// Applies one intent transition with the service clock.
    fn transition_intent(
        &mut self,
        intent: &SideEffectIntent,
        event: TransitionEvent,
        evidence: &[String],
    ) -> Result<SideEffectIntent, WorkspaceFailure> {
        let now = self.clock.now_millis();
        let at_ms = i64::try_from(now).unwrap_or(i64::MAX);
        self.store
            .transition_side_effect_intent(intent.intent_id(), event, evidence, at_ms)
            .map_err(store_failure)
    }

    /// Step 4 of the saga (crash-B seam): independently observe Git
    /// postconditions, then atomically commit `Allocated → Ready` with the
    /// authoritative `workspace.created` fact. Strictly for `Allocated`
    /// leases; `Ready` leases reconcile through `ensure_workspace_ready`
    /// (which appends nothing), so `workspace.created` stays exactly-once
    /// even across restarts.
    pub fn finalize_ready(
        &mut self,
        id: &WorkspaceId,
        repository_path: &str,
    ) -> Result<WorkspaceLease, WorkspaceFailure> {
        let lease =
            self.store
                .get_workspace(id)?
                .ok_or_else(|| WorkspaceFailure::StorageFailure {
                    detail: format!("lease {} does not exist for finalization", id.as_str()),
                })?;
        if lease.lifecycle() != LeaseLifecycle::Allocated {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "finalize requires an Allocated lease; {} is {}",
                    id.as_str(),
                    lease.lifecycle()
                ),
            });
        }
        let anchor = self.load_anchor(lease.id())?;
        self.verify_git_consistent(&lease, repository_path, &anchor)?;
        let execution = self
            .store
            .get_execution(lease.owner())
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} references missing execution {}",
                    id.as_str(),
                    lease.owner().as_str()
                ),
            })?;
        let task = self
            .store
            .get_task(execution.task_id())
            .map_err(store_failure)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "execution {} references missing task {}",
                    lease.owner().as_str(),
                    execution.task_id().as_str()
                ),
            })?;
        let evidence = self.mint_evidence(
            WorkspaceEventKind::Created,
            task.id(),
            task.project_id(),
            &execution,
            &lease,
            "finalize_ready",
        );
        self.store.mark_lease_ready(id, &evidence)?;
        self.store
            .get_workspace(id)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!("lease {} vanished during finalization", id.as_str()),
            })
    }

    /// Reconciles one existing lease against a request and live Git. Never
    /// replays Git mutation blindly, never deletes anything: ambiguous or
    /// unsafe states fail explicitly for operator attention (TASK-0043
    /// owns cleanup; TASK-0044 owns the wider failure matrix).
    fn reconcile_existing(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        self.validate_request_against_lease(&lease, request)?;
        let anchor = self.load_anchor(lease.id())?;
        match lease.lifecycle() {
            LeaseLifecycle::Allocated => self.reconcile_allocated(lease, request, &anchor),
            LeaseLifecycle::Ready => {
                // Defensive exactly-once: Ready implies its created fact
                // committed atomically; a Ready lease without one is
                // corruption, not health.
                if !self
                    .store
                    .has_workspace_event(lease.id(), WorkspaceEventKind::Created)?
                {
                    return Err(WorkspaceFailure::StorageFailure {
                        detail: format!(
                            "lease {} is Ready without workspace.created evidence",
                            lease.id().as_str()
                        ),
                    });
                }
                // Drift never recreates: a vanished or changed Ready
                // workspace may hold agent work — fail explicitly.
                self.verify_git_consistent(&lease, &request.repository_path, &anchor)?;
                Ok(EnsuredWorkspace {
                    lease,
                    outcome: EnsureOutcome::AlreadyReady,
                })
            }
            LeaseLifecycle::CleanupPending => Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is CleanupPending; resume teardown with cleanup_workspace; \
                     no action taken",
                    lease.id().as_str()
                ),
            }),
            LeaseLifecycle::Removed => Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is Removed (terminal); no action taken",
                    lease.id().as_str()
                ),
            }),
        }
    }

    /// Allocated-lease reconcile (ADR-0031 SE-3/SE-6): observes the Git
    /// target, drives the stranded intent (if any) through RECONCILE,
    /// and continues exactly where the crash left off. Every outcome
    /// matches the pre-0126 behavior; the intent trail is additive.
    fn reconcile_allocated(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        let intent = self.active_materialize_intent(&lease)?;
        // Terminal CONFIRMED needs no machine: the effect provably
        // landed, so observe-and-finalize exactly as before (crash
        // between CONFIRMED and finalize, or a legacy re-entry).
        if intent
            .as_ref()
            .is_some_and(|intent| intent.state() == SideEffectState::Confirmed)
        {
            return self.reconcile_without_intent(lease, request, anchor);
        }
        // Unresolved ESCALATE suspends the autonomous sequence (SE-8).
        if let Some(intent) = intent
            .as_ref()
            .filter(|intent| intent.state() == SideEffectState::Escalate)
        {
            if !self.materialize_escalation_resolved(intent)? {
                return Err(WorkspaceFailure::ReconciliationRequired {
                    detail: format!(
                        "lease {} holds an unresolved materialize escalation; resolve it first",
                        lease.id().as_str()
                    ),
                });
            }
        }
        // Crash-B shape first (read-only observation): Git did the
        // work, finalization never ran. Converge every stranded state
        // to AMBIGUOUS, then confirm through RECONCILE.
        if self
            .verify_git_consistent(&lease, &request.repository_path, anchor)
            .is_ok()
        {
            if let Some(intent) = intent {
                let intent = match intent.state() {
                    SideEffectState::IntentRecorded => {
                        self.transition_intent(&intent, TransitionEvent::Dispatch, &[])?;
                        self.refresh_materialize_intent(&lease)?
                    }
                    SideEffectState::SafeToRetry => {
                        self.transition_intent(&intent, TransitionEvent::Redispatch, &[])?;
                        self.refresh_materialize_intent(&lease)?
                    }
                    SideEffectState::SafeToContinue => {
                        self.transition_intent(
                            &intent,
                            TransitionEvent::DispatchContinuation,
                            &[],
                        )?;
                        self.refresh_materialize_intent(&lease)?
                    }
                    _ => intent,
                };
                if intent.state() == SideEffectState::Executing {
                    self.transition_intent(
                        &intent,
                        TransitionEvent::MarkAmbiguous,
                        &["stranded-executing-on-resume".to_owned()],
                    )?;
                }
                let intent = self.refresh_materialize_intent(&lease)?;
                if intent.state() == SideEffectState::Ambiguous {
                    self.transition_intent(&intent, TransitionEvent::BeginReconcile, &[])?;
                }
                let intent = self.refresh_materialize_intent(&lease)?;
                if intent.state() == SideEffectState::Reconcile {
                    self.transition_intent(
                        &intent,
                        TransitionEvent::ReconciledToConfirmed,
                        &["crash-b-postconditions-verified".to_owned()],
                    )?;
                }
            }
            let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
            return Ok(EnsuredWorkspace {
                lease: ready,
                outcome: EnsureOutcome::RecognizedGitState,
            });
        }
        if let Some(intent) = intent {
            return self.reconcile_stranded_materialize(lease, request, anchor, intent);
        }
        self.reconcile_without_intent(lease, request, anchor)
    }

    /// Legacy path: no live intent (pre-0126 lease, terminal lineage,
    /// or resolved escalation). Observes the shape and proceeds
    /// exactly as before, claiming an intent only when effects exist
    /// to reconcile (never fabricating history for clean shapes).
    fn reconcile_without_intent(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        if self
            .verify_git_consistent(&lease, &request.repository_path, anchor)
            .is_ok()
        {
            let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
            return Ok(EnsuredWorkspace {
                lease: ready,
                outcome: EnsureOutcome::RecognizedGitState,
            });
        }
        if self.prove_branch_only_recovery(&lease, request)? {
            return self.claim_and_reconcile(lease, request, anchor);
        }
        if !self.side_effects_present(&lease, &request.repository_path) {
            // Crash-A shape: intent only, Git untouched. Resume.
            self.materialize_allocated(&lease, &request.repository_path)?;
            let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
            return Ok(EnsuredWorkspace {
                lease: ready,
                outcome: EnsureOutcome::ResumedAllocated,
            });
        }
        // Crash-C shape: claim the intent, then reconcile (SE-8).
        self.claim_and_reconcile(lease, request, anchor)
    }

    /// Claims an intent over observed out-of-band effects, then runs
    /// the stranded reconcile: Dispatch → AMBIGUOUS ("another party
    /// may have effected this target") → RECONCILE → shape.
    fn claim_and_reconcile(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        let target_text = self.materialize_target_text(&lease)?;
        let intent =
            self.record_materialize_intent(&lease, &request.repository_path, &target_text, anchor)?;
        self.transition_intent(&intent, TransitionEvent::Dispatch, &[])?;
        let intent = self.refresh_materialize_intent(&lease)?;
        self.transition_intent(
            &intent,
            TransitionEvent::MarkAmbiguous,
            &["out-of-band-effects-observed".to_owned()],
        )?;
        let intent = self.refresh_materialize_intent(&lease)?;
        self.reconcile_stranded_materialize(lease, request, anchor, intent)
    }

    /// Reconciles a stranded non-terminal intent: EXECUTING (or a
    /// claimed dispatch) becomes AMBIGUOUS, RECONCILE observes the
    /// shape, and exactly one typed outcome follows (SE-3/SE-6).
    fn reconcile_stranded_materialize(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
        intent: SideEffectIntent,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        // Drive to AMBIGUOUS from wherever the crash stranded the
        // lineage (EXECUTING normally; INTENT_RECORDED claims its
        // first dispatch here since effects may exist out-of-band).
        let intent = match intent.state() {
            SideEffectState::IntentRecorded => {
                self.transition_intent(&intent, TransitionEvent::Dispatch, &[])?;
                let intent = self.refresh_materialize_intent(&lease)?;
                self.transition_intent(
                    &intent,
                    TransitionEvent::MarkAmbiguous,
                    &["out-of-band-effects-observed".to_owned()],
                )?;
                self.refresh_materialize_intent(&lease)?
            }
            SideEffectState::Executing => {
                self.transition_intent(
                    &intent,
                    TransitionEvent::MarkAmbiguous,
                    &["stranded-executing-on-resume".to_owned()],
                )?;
                self.refresh_materialize_intent(&lease)?
            }
            SideEffectState::Ambiguous => intent,
            SideEffectState::Reconcile => intent,
            SideEffectState::SafeToRetry => {
                return self.redispatch_after_retry(lease, request, anchor, intent);
            }
            SideEffectState::SafeToContinue => {
                return self.continue_branch_only(lease, request, anchor, intent);
            }
            // Terminals and CONFIRMED never reach here (handled by the
            // caller); ESCALATE is gated on resolution there too.
            _ => {
                return self.reconcile_without_intent(lease, request, anchor);
            }
        };
        if intent.state() == SideEffectState::Ambiguous {
            self.transition_intent(&intent, TransitionEvent::BeginReconcile, &[])?;
        }
        let intent = self.refresh_materialize_intent(&lease)?;
        if self.prove_branch_only_recovery(&lease, request)? {
            self.transition_intent(
                &intent,
                TransitionEvent::ReconciledToContinue,
                &["branch-only-provenance-verified".to_owned()],
            )?;
            let intent = self.refresh_materialize_intent(&lease)?;
            return self.continue_branch_only(lease, request, anchor, intent);
        }
        if !self.side_effects_present(&lease, &request.repository_path) {
            self.transition_intent(
                &intent,
                TransitionEvent::ReconciledToRetry,
                &["crash-a-target-absent".to_owned()],
            )?;
            let intent = self.refresh_materialize_intent(&lease)?;
            return self.redispatch_after_retry(lease, request, anchor, intent);
        }
        self.transition_intent(
            &intent,
            TransitionEvent::Escalate,
            &["crash-c-shape-unrecognized".to_owned()],
        )?;
        let intent = self.refresh_materialize_intent(&lease)?;
        self.escalate_materialize(lease, intent, "ambiguous-target-unrecognized")
    }

    /// Crash-A continuation: the target provably matches the pre-effect
    /// state, so re-dispatch the original operation (SE-4 same key).
    fn redispatch_after_retry(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
        intent: SideEffectIntent,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        // Re-prove absence at dispatch time: the world may have moved
        // since RECONCILE ran (crash between SafeToRetry and here).
        if self.side_effects_present(&lease, &request.repository_path) {
            self.transition_intent(&intent, TransitionEvent::Redispatch, &[])?;
            let intent = self.refresh_materialize_intent(&lease)?;
            self.transition_intent(
                &intent,
                TransitionEvent::MarkAmbiguous,
                &["target-moved-since-reconcile".to_owned()],
            )?;
            let intent = self.refresh_materialize_intent(&lease)?;
            return self.reconcile_stranded_materialize(lease, request, anchor, intent);
        }
        self.transition_intent(&intent, TransitionEvent::Redispatch, &[])?;
        let intent = self.refresh_materialize_intent(&lease)?;
        let target_text = self.materialize_target_text(&lease)?;
        self.dispatch_materialize_effect(
            &lease,
            &request.repository_path,
            &target_text,
            anchor,
            &intent,
        )?;
        let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
        Ok(EnsuredWorkspace {
            lease: ready,
            outcome: EnsureOutcome::ResumedAllocated,
        })
    }

    /// Branch-only continuation (AttachWorktree): the named
    /// continuation re-proves provenance, then attaches.
    fn continue_branch_only(
        &mut self,
        lease: WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
        intent: SideEffectIntent,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        if intent.state() == SideEffectState::SafeToContinue {
            self.transition_intent(&intent, TransitionEvent::DispatchContinuation, &[])?;
        }
        let intent = self.refresh_materialize_intent(&lease)?;
        self.recover_branch_only_with_intent(&lease, request, anchor, &intent)
    }

    /// Crash-C: record the typed escalation and return the same
    /// refusal as before (SE-8; externally unchanged).
    fn escalate_materialize(
        &mut self,
        lease: WorkspaceLease,
        intent: SideEffectIntent,
        code: &str,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        let now = self.clock.now_millis();
        let at_ms = i64::try_from(now).unwrap_or(i64::MAX);
        self.store
            .record_side_effect_escalation(
                intent.intent_id(),
                code,
                &["crash-c-ambiguous-partial-state".to_owned()],
                at_ms,
            )
            .map_err(store_failure)?;
        Err(WorkspaceFailure::CheckoutFailed {
            detail: format!(
                "lease {} has ambiguous partial Git state; refusing blind replay; \
                 inspect explicitly (cleanup owned by TASK-0043)",
                lease.id().as_str()
            ),
        })
    }

    /// Reloads the lease's materialize intent (transitions move the
    /// row; in-memory copies go stale by design).
    fn refresh_materialize_intent(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<SideEffectIntent, WorkspaceFailure> {
        self.active_materialize_intent(lease)?
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: format!(
                    "materialize intent for lease {} vanished",
                    lease.id().as_str()
                ),
            })
    }

    /// Independently observes that live Git matches the durable lease:
    /// managed branch exists and points at the base, the worktree HEAD is
    /// the base, and the worktree observes attached (never Missing or
    /// Invalid). Dirt is accepted — it is new information, not creation
    /// inconsistency.
    fn verify_git_consistent(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        anchor: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        let inconsistent = |detail: String| WorkspaceFailure::CheckoutFailed { detail };
        if !self
            .git
            .branch_exists(repository_path, lease.branch(), anchor)?
        {
            return Err(inconsistent(format!(
                "managed branch {} absent for lease {}",
                lease.branch().as_str(),
                lease.id().as_str()
            )));
        }
        let at_branch =
            self.git
                .resolve_revision(repository_path, lease.branch().as_str(), anchor)?;
        if at_branch != *lease.base_revision() {
            return Err(inconsistent(format!(
                "managed branch {} points at {} for lease {}, expected base {}",
                lease.branch().as_str(),
                at_branch.as_str(),
                lease.id().as_str(),
                lease.base_revision().as_str()
            )));
        }
        let at_worktree = self
            .git
            .resolve_revision(lease.worktree_ref(), "HEAD", anchor)?;
        if at_worktree != *lease.base_revision() {
            return Err(inconsistent(format!(
                "worktree HEAD is {} for lease {}, expected base {}",
                at_worktree.as_str(),
                lease.id().as_str(),
                lease.base_revision().as_str()
            )));
        }
        match self.git.worktree_status(lease.worktree_ref(), anchor)? {
            workspace::WorktreeStatus::Clean
            | workspace::WorktreeStatus::DirtyTracked
            | workspace::WorktreeStatus::DirtyUntracked => Ok(()),
            workspace::WorktreeStatus::Missing | workspace::WorktreeStatus::Invalid => {
                Err(inconsistent(format!(
                    "worktree unusable for lease {}",
                    lease.id().as_str()
                )))
            }
        }
    }

    /// Read-only resume preflight for deterministic workspace state. This
    /// catches known conflicts before a live-run claim/acceptance; the
    /// background path still re-runs every proof before mutation, so this is
    /// never a TOCTOU correctness boundary.
    pub fn validate_resume_workspace(
        &self,
        request: &WorkspaceRequest,
    ) -> Result<(), WorkspaceFailure> {
        let Some(lease) = self
            .store
            .get_workspace_for_execution(&request.execution_id)?
        else {
            return Ok(());
        };
        self.validate_request_against_lease(&lease, request)?;
        let anchor = self.load_anchor(lease.id())?;
        match lease.lifecycle() {
            LeaseLifecycle::Allocated => {
                if self
                    .verify_git_consistent(&lease, &request.repository_path, &anchor)
                    .is_ok()
                    || self.prove_branch_only_recovery(&lease, request)?
                    || !self.side_effects_present(&lease, &request.repository_path)
                {
                    Ok(())
                } else {
                    Err(WorkspaceFailure::CheckoutFailed {
                        detail: format!(
                            "lease {} has ambiguous partial Git state; refusing resume before acceptance",
                            lease.id().as_str()
                        ),
                    })
                }
            }
            LeaseLifecycle::Ready => {
                if !self
                    .store
                    .has_workspace_event(lease.id(), WorkspaceEventKind::Created)?
                {
                    return Err(WorkspaceFailure::StorageFailure {
                        detail: format!(
                            "lease {} is Ready without workspace.created evidence",
                            lease.id().as_str()
                        ),
                    });
                }
                self.verify_git_consistent(&lease, &request.repository_path, &anchor)
            }
            LeaseLifecycle::CleanupPending => Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is CleanupPending; resume workspace recovery is unavailable",
                    lease.id().as_str()
                ),
            }),
            LeaseLifecycle::Removed => Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is Removed; resume workspace recovery is unavailable",
                    lease.id().as_str()
                ),
            }),
        }
    }

    fn validate_request_against_lease(
        &self,
        lease: &WorkspaceLease,
        request: &WorkspaceRequest,
    ) -> Result<(), WorkspaceFailure> {
        if request.repository_ref != *lease.repository() {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "execution {} requests unexpected repository {}; lease owns {}",
                    request.execution_id.as_str(),
                    request.repository_ref.as_str(),
                    lease.repository().as_str()
                ),
            });
        }
        let anchor = self.load_anchor(lease.id())?;
        let requested_base =
            self.git
                .resolve_revision(&request.repository_path, &request.base_revision, &anchor)?;
        if requested_base != *lease.base_revision() {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "execution {} requests base {} but lease {} is based at {}; bases are immutable",
                    request.execution_id.as_str(),
                    requested_base.as_str(),
                    lease.id().as_str(),
                    lease.base_revision().as_str()
                ),
            });
        }
        Ok(())
    }

    /// Proves the narrow TASK-0057 branch-only crash shape without mutation.
    /// `Ok(false)` means no current v2 managed branch exists and the normal
    /// Crash-A/Crash-C classifier must decide. Once a v2 branch exists, every
    /// proof is mandatory and mismatches fail closed.
    fn prove_branch_only_recovery(
        &self,
        lease: &WorkspaceLease,
        request: &WorkspaceRequest,
    ) -> Result<bool, WorkspaceFailure> {
        let anchor = self.load_anchor(lease.id())?;
        if self.classify_lease_identity(lease)? == PersistedManagedIdentityFormat::Legacy {
            return Ok(false);
        }
        if !self
            .git
            .branch_exists(&request.repository_path, lease.branch(), &anchor)?
        {
            return Ok(false);
        }

        if lease.owner() != &request.execution_id {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} is owned by execution {}, not {}",
                    lease.id().as_str(),
                    lease.owner().as_str(),
                    request.execution_id.as_str()
                ),
            });
        }
        let expected_branch = crate::managed_identity::branch_for_execution(lease.owner())?;
        if &expected_branch != lease.branch() {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} persisted branch is not the v2 branch owned by execution {}; refusing recovery",
                    lease.id().as_str(),
                    lease.owner().as_str()
                ),
            });
        }
        match self
            .store
            .find_identity_reservation_owner(lease.branch().as_str())?
        {
            Some(owner) if owner == *lease.owner() => {}
            Some(owner) => {
                return Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "managed branch {} is reserved by execution {}, not {}",
                        lease.branch().as_str(),
                        owner.as_str(),
                        lease.owner().as_str()
                    ),
                });
            }
            None => {
                return Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "managed branch {} has no durable identity reservation; refusing recovery",
                        lease.branch().as_str()
                    ),
                });
            }
        }
        match self
            .store
            .find_lease_owner_in_repository(lease.repository().as_str(), lease.branch().as_str())?
        {
            Some(owner) if owner == *lease.owner() => {}
            _ => {
                return Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "lease {} cannot prove exclusive branch ownership",
                        lease.id().as_str()
                    ),
                });
            }
        }
        match self
            .store
            .find_lease_owner_by_worktree_ref(lease.worktree_ref())?
        {
            Some(owner) if owner == *lease.owner() => {}
            _ => {
                return Err(WorkspaceFailure::OwnershipMismatch {
                    detail: format!(
                        "lease {} cannot prove exclusive worktree-path ownership",
                        lease.id().as_str()
                    ),
                });
            }
        }

        self.validate_history_binding(lease, &request.repository_path)?;
        let branch_tip = self.git.resolve_revision(
            &request.repository_path,
            lease.branch().as_str(),
            &anchor,
        )?;
        if branch_tip != *lease.base_revision() {
            return Err(WorkspaceFailure::CheckoutFailed {
                detail: format!(
                    "lease {} managed branch points at {}, expected exact recorded base {}; refusing branch-only recovery",
                    lease.id().as_str(),
                    branch_tip.as_str(),
                    lease.base_revision().as_str()
                ),
            });
        }

        let target = self.validate_managed_path(lease)?;
        match std::fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(WorkspaceFailure::WorktreeCollision {
                    detail: format!(
                        "lease {} branch-only recovery requires an absent target path; {} already exists",
                        lease.id().as_str(),
                        target.display()
                    ),
                });
            }
            Err(error) => {
                return Err(WorkspaceFailure::StorageFailure {
                    detail: format!(
                        "cannot inspect workspace target {} for lease {}: {error}",
                        target.display(),
                        lease.id().as_str()
                    ),
                });
            }
        }
        if self.git.is_registered_worktree(
            &request.repository_path,
            lease.worktree_ref(),
            &anchor,
        )? {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} target path is already registered; branch-only recovery does not guess ownership",
                    lease.id().as_str()
                ),
            });
        }
        if let Some(holder) =
            self.git
                .worktree_for_branch(&request.repository_path, lease.branch(), &anchor)?
        {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} managed branch is already registered at {holder}; refusing duplicate materialization",
                    lease.id().as_str()
                ),
            });
        }
        Ok(true)
    }

    /// Branch-only continuation with the intent trail: re-proves
    /// provenance (the caller proved it to select this path; the world
    /// may have moved), attaches, and records the classified outcome.
    /// Errors match the pre-0126 behavior exactly.
    fn recover_branch_only_with_intent(
        &mut self,
        lease: &WorkspaceLease,
        request: &WorkspaceRequest,
        anchor: &TrustedCommonGit,
        intent: &SideEffectIntent,
    ) -> Result<EnsuredWorkspace, WorkspaceFailure> {
        if !self.prove_branch_only_recovery(lease, request)? {
            // Provenance lapsed between selection and continuation:
            // back to AMBIGUOUS for a fresh reconcile round.
            self.transition_intent(
                intent,
                TransitionEvent::MarkAmbiguous,
                &["branch-only-provenance-lapsed".to_owned()],
            )?;
            let intent = self.refresh_materialize_intent(lease)?;
            self.transition_intent(&intent, TransitionEvent::BeginReconcile, &[])?;
            let intent = self.refresh_materialize_intent(lease)?;
            self.transition_intent(
                &intent,
                TransitionEvent::Escalate,
                &["continuation-precondition-lapsed".to_owned()],
            )?;
            let intent = self.refresh_materialize_intent(lease)?;
            let now = self.clock.now_millis();
            self.store
                .record_side_effect_escalation(
                    intent.intent_id(),
                    "ambiguous-target-unrecognized",
                    &["crash-c-ambiguous-partial-state".to_owned()],
                    i64::try_from(now).unwrap_or(i64::MAX),
                )
                .map_err(store_failure)?;
            return Err(WorkspaceFailure::CheckoutFailed {
                detail: format!(
                    "lease {} has ambiguous partial Git state; refusing blind replay; \
                     inspect explicitly (cleanup owned by TASK-0043)",
                    lease.id().as_str()
                ),
            });
        }
        match self.git.attach_worktree(
            lease,
            &request.repository_path,
            lease.worktree_ref(),
            anchor,
        ) {
            Ok(()) => {
                self.transition_intent(
                    intent,
                    TransitionEvent::Confirm,
                    &["attach-postconditions-verified".to_owned()],
                )?;
            }
            Err(error) => {
                match classify_dispatch_error(&error) {
                    DispatchOutcome::Failed => {
                        self.transition_intent(
                            intent,
                            TransitionEvent::Fail,
                            &[format!("attach-refused:{}", error.kind())],
                        )?;
                    }
                    _ => {
                        self.transition_intent(
                            intent,
                            TransitionEvent::MarkAmbiguous,
                            &[format!("unknown-attach-outcome:{}", error.kind())],
                        )?;
                    }
                }
                return Err(error);
            }
        }
        let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
        Ok(EnsuredWorkspace {
            lease: ready,
            outcome: EnsureOutcome::RecoveredBranchOnly,
        })
    }

    /// Conservative side-effect probe for the crash-A vs crash-C decision:
    /// only "branch absent AND target absent" counts as untouched. Backend
    /// errors and unusable states count as present (fail-closed direction).
    fn side_effects_present(&self, lease: &WorkspaceLease, repository_path: &str) -> bool {
        let Ok(anchor) = self.load_anchor(lease.id()) else {
            return true;
        };
        let branch_present = self
            .git
            .branch_exists(repository_path, lease.branch(), &anchor)
            .unwrap_or(true);
        if branch_present {
            return true;
        }
        self.git
            .worktree_status(lease.worktree_ref(), &anchor)
            .map(|s| s != workspace::WorktreeStatus::Missing)
            .unwrap_or(true)
    }

    /// Mints workspace journal evidence. The service owns every
    /// authoritative field — ids for correlation/causation, timestamp,
    /// provenance, and all scope references derived through Execution
    /// ownership. The caller controls none of it.
    fn mint_evidence(
        &mut self,
        kind: WorkspaceEventKind,
        task_id: &TaskId,
        project_id: &ProjectId,
        execution: &agent_runtime::Execution,
        lease: &WorkspaceLease,
        op: &str,
    ) -> WorkspaceJournalEvidence {
        let now = self.clock.now_millis();
        self.op_counter += 1;
        WorkspaceJournalEvidence {
            kind,
            task_id: task_id.clone(),
            project_id: project_id.clone(),
            execution_id: execution.id().clone(),
            workspace_id: lease.id().clone(),
            correlation_id: crate::op_id::correlation_id(now, self.op_counter),
            causation_id: crate::op_id::causation_id(now, self.op_counter),
            occurred_at: format_rfc3339(now),
            provenance: format!("{PROVENANCE_SERVICE}/{op}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_identity::{branch_for_execution, managed_slug};

    #[test]
    fn slugs_are_single_components_without_traversal() {
        for raw in [
            "E-1",
            "ws-123",
            "../..",
            "a/b",
            "a b",
            "fëature-日本語",
            "-leading",
            "trailing-",
            "semi;colon",
            "dollar$tick`",
            "percent%sign",
            "mix-2F-escape",
        ] {
            let body = managed_slug(raw);
            assert!(!body.is_empty(), "slug for {raw:?} must not be empty");
            assert!(!body.contains('/'), "slug for {raw:?} must not separate");
            assert!(!body.contains('\\'), "slug for {raw:?} must not separate");
            assert!(!body.contains(' '), "slug for {raw:?} must not space");
            assert_ne!(body, ".", "slug for {raw:?} must not be dot");
            assert_ne!(body, "..", "slug for {raw:?} must not be dotdot");
            assert_eq!(
                body.to_ascii_lowercase(),
                body,
                "slug for {raw:?} must already be casefold-stable"
            );
        }
        // Short identities stay readable: lowercase passes through under
        // the version prefix, uppercase is encoded (TASK-0053 v2).
        assert_eq!(managed_slug("e-1"), "e2-e-1");
        assert_eq!(managed_slug("e-9"), "e2-e-9");
        assert_eq!(managed_slug("E-1"), "e2-%45-1");
        assert_eq!(managed_slug("ws-123"), "e2-ws-123");
        assert_ne!(managed_slug("a/b"), managed_slug("a-b"));
        assert_ne!(managed_slug("a/b"), managed_slug("a_b"));
        // Long identities stay bounded and deterministic (TASK-0053: the
        // digest form is `h2-` + 64 hex = 67 chars; readable forms carry
        // the disjoint `e2-` prefix, so the ranges never overlap).
        let long = "x".repeat(300);
        let hashed = managed_slug(&long);
        assert!(hashed.len() <= 80, "digest slug must stay bounded");
        assert_eq!(hashed, managed_slug(&long));
        assert!(hashed.starts_with("h2-"));
        assert_eq!(hashed.len(), 67);
    }

    /// TASK-0051 H4 keeper (unit level, TASK-0053 v2 expectations): the
    /// exact production pair that wedged the second task must derive
    /// distinct managed branches.
    #[test]
    fn known_h4_collision_pair_is_resolved() {
        let slash = ExecutionId::new("E/S").unwrap();
        let mimic = ExecutionId::new("E-2FS").unwrap();
        let first = branch_for_execution(&slash).unwrap();
        let second = branch_for_execution(&mimic).unwrap();
        assert_ne!(
            first, second,
            "E/S and E-2FS must never share a managed branch"
        );
        assert_eq!(first.as_str(), "refs/heads/agent/e2-%45%2f%53");
        assert_eq!(second.as_str(), "refs/heads/agent/e2-%45-2%46%53");
    }

    /// TASK-0051 H4 keeper (unit level): adversarial families around the
    /// escape scheme — every pair that could mimic an encoded sequence
    /// must derive distinct branches.
    #[test]
    fn escape_mimic_families_stay_injective() {
        let pairs = [
            ("a/b", "a%2Fb"),
            ("a%b", "a%%b"),
            ("a%%b", "a%%%%b"),
            ("x%2F", "x%252F"),
            ("-", "--"),
            ("--", "----"),
            ("E-", "E--"),
            ("100%25", "100%%25"),
            ("a b", "a%20b"),
            ("a%20b", "a%%20b"),
            ("dot.name", "dot%2Ename"),
            ("semi;colon", "semi%3Bcolon"),
        ];
        for (left, right) in pairs {
            let a = branch_for_execution(&ExecutionId::new(left).unwrap()).unwrap();
            let b = branch_for_execution(&ExecutionId::new(right).unwrap()).unwrap();
            assert_ne!(a, b, "{left:?} and {right:?} must derive distinct branches");
        }
    }

    /// TASK-0051 H4 keeper (unit level): bounded deterministic corpus
    /// across the valid `ExecutionId` domain — every derived branch is
    /// unique, single-component, and managed-scoped.
    #[test]
    fn branch_derivation_is_injective_over_corpus() {
        let mut corpus = vec![
            "E-1".to_owned(),
            "E/S".to_owned(),
            "E-2FS".to_owned(),
            "E%2FS".to_owned(),
            "a".repeat(64),
            "a".repeat(65),
            "x".repeat(256),
            "y".repeat(255),
            "-".to_owned(),
            "--".to_owned(),
            "%".to_owned(),
            "%%".to_owned(),
            "h".to_owned() + &"a".repeat(64) + "-",
            "日本語-日本語".to_owned(),
            "é".repeat(100),
            "Mixed_Case-123.ok".to_owned(),
            "..".to_owned(),
            ".".to_owned(),
            "HEAD".to_owned(),
            "main".to_owned(),
            "refs".to_owned(),
        ];
        // Raw control bytes (valid `ExecutionId` content when embedded
        // mid-string) must escape, never leak into the ref.
        for byte in [0x00u8, 0x01, 0x1F, 0x7F] {
            corpus.push(format!("a{}b", byte as char));
        }
        for prefix in ["E", "E-", "E-2", "E-2F", "E-2FS", "E-2FS-"] {
            if !corpus.iter().any(|existing| existing == prefix) {
                corpus.push(prefix.to_owned());
            }
            corpus.push(format!("{prefix}/x"));
        }
        let mut seen = std::collections::BTreeMap::new();
        for raw in &corpus {
            let id = ExecutionId::new(raw).unwrap();
            let derived = branch_for_execution(&id).unwrap();
            assert!(
                derived.is_managed(),
                "{raw:?} must stay in the managed scope"
            );
            let slug = derived.short_name().strip_prefix("agent/").unwrap();
            assert!(!slug.contains('/'), "{raw:?} slug must be one component");
            assert!(
                derived.as_str().len() <= 128,
                "{raw:?} branch must stay bounded"
            );
            if let Some(previous) = seen.insert(derived.as_str().to_owned(), raw.clone()) {
                panic!("branch collision: {previous:?} and {raw:?} share a branch");
            }
        }
        // Long-identity digests are distinguished by exact input bytes:
        // same length, one byte apart → different branches.
        let mut long_a = "z".repeat(200);
        let mut long_b = "z".repeat(200);
        long_b.replace_range(100..101, "y");
        assert_ne!(managed_slug(&long_a), managed_slug(&long_b));
        long_a.push('q');
        assert_ne!(managed_slug(&long_a), managed_slug(&long_b));
    }

    /// Managed identity follows `ExecutionId` exact-equality semantics:
    /// no Unicode normalization is applied — canonically equivalent but
    /// byte-distinct inputs stay distinct (documented choice, TASK-0051
    /// §31). Both still derive valid branches.
    #[test]
    fn unicode_branches_are_byte_exact() {
        let composed = "caf\u{e9}";
        let decomposed = "cafe\u{301}";
        assert_ne!(composed.as_bytes(), decomposed.as_bytes());
        let a = branch_for_execution(&ExecutionId::new(composed).unwrap()).unwrap();
        let b = branch_for_execution(&ExecutionId::new(decomposed).unwrap()).unwrap();
        assert_ne!(a, b);
        assert!(a.is_managed() && b.is_managed());
    }

    #[test]
    fn branch_derivation_is_deterministic_and_valid() {
        let owner = ExecutionId::new("E-9").unwrap();
        let first = branch_for_execution(&owner).unwrap();
        let second = branch_for_execution(&owner).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.as_str(), "refs/heads/agent/e2-%45-9");
        let hostile = ExecutionId::new("../../x").unwrap();
        let derived = branch_for_execution(&hostile).unwrap();
        assert!(derived.as_str().starts_with("refs/heads/agent/"));
        // The slug after the fixed `agent/` prefix is a single component:
        // traversal and separators are encoded away, and the slug always
        // leads with the version prefix (`e2-`/`h2-`), never a dash or dot.
        let slug = derived.short_name().strip_prefix("agent/").unwrap();
        assert!(!slug.contains('/'));
        assert!(!slug.contains(".."));
        assert!(derived.short_name().starts_with("agent/"));
    }

    #[test]
    fn managed_root_rejects_non_roots_and_contains_slugs() {
        let root = ManagedWorkspaceRoot::new(std::env::temp_dir()).unwrap();
        let id = WorkspaceId::new("ws-1").unwrap();
        let target = root.join_workspace(&id).unwrap();
        assert!(target.strip_prefix(root.path()).is_ok());
        assert!(target.ends_with("ws-e2-ws-1"));

        assert!(ManagedWorkspaceRoot::new("relative/path").is_err());
        assert!(ManagedWorkspaceRoot::new("/nonexistent-root-for-task0042").is_err());
    }
}
