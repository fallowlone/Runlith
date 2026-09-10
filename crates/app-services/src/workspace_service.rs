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
    BranchRef, DiffEvidence, GitPort, LeaseLifecycle, RepositoryRef, WorkspaceEventKind,
    WorkspaceFailure, WorkspaceId, WorkspaceJournalEvidence, WorkspaceLease, WorkspaceStore,
    WorktreeStatus,
};

use crate::clock::{format_rfc3339, Clock, SystemClock};

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

/// FNV-1a 64-bit digest for overlong identity slugs. Only reached by
/// identities that cannot fit a readable encoding; short identities keep
/// the injective byte encoding below.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Injective, filesystem-safe slug body for one validated identity:
/// ASCII alphanumerics plus `-`/`_` pass through, every other byte becomes
/// `-XX` hex. The output never contains `/`, never forms `.`/`..`, and
/// never starts with `-` after the caller's fixed prefix, so it is always
/// a single safe path component / ref segment. Inputs longer than 64 bytes
/// use a stable digest instead (length-bounded, still deterministic).
fn slug_body(raw: &str) -> String {
    if raw.len() > 64 {
        return format!("h{:016X}", fnv1a_64(raw.as_bytes()));
    }
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            out.push(byte as char);
        } else {
            out.push('-');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Derives the managed branch for one execution (`refs/heads/agent/<slug>`,
/// ADR-0023: the branch-safe slug comes from the owner ExecutionId and the
/// exact ref is recorded durably). Deterministic, so reopen and
/// reconciliation derive the same branch without new input.
fn branch_for_execution(owner: &ExecutionId) -> Result<BranchRef, WorkspaceFailure> {
    BranchRef::new(format!("refs/heads/agent/{}", slug_body(owner.as_str())))
        .map_err(domain_failure)
}

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

    /// Derives `<root>/ws-<slug>` for one workspace id and proves
    /// containment. Fails closed on any escape (unreachable by
    /// construction — the slug holds no separators — but checked, not
    /// assumed).
    pub fn join_workspace(&self, id: &WorkspaceId) -> Result<PathBuf, WorkspaceFailure> {
        let target = self.root.join(format!("ws-{}", slug_body(id.as_str())));
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
        let evidence = self.git.compute_diff(&lease)?;
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
        match self.git.worktree_status(lease.worktree_ref())? {
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
        match self.git.worktree_status(lease.worktree_ref())? {
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
        if self.git.branch_exists(repository_path, lease.branch())? {
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
        if !worktree_known_absent {
            // TOCTOU re-check immediately before destruction: preflight
            // cleanliness may have changed since the entry check.
            match self.git.worktree_status(lease.worktree_ref())? {
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
            match self.git.remove_worktree(lease, lease.worktree_ref()) {
                Ok(()) => {}
                Err(WorkspaceFailure::WorktreeMissing { .. })
                | Err(WorkspaceFailure::AlreadyAbsent) => {}
                Err(other) => return Err(other),
            }
            match self.git.worktree_status(lease.worktree_ref())? {
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
        if self.git.branch_exists(repository_path, lease.branch())? {
            match self.git.remove_branch(repository_path, lease.branch()) {
                Ok(()) => {}
                Err(WorkspaceFailure::AlreadyAbsent) => {}
                Err(other) => return Err(other),
            }
            if self.git.branch_exists(repository_path, lease.branch())? {
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
        match self.git.worktree_status(lease.worktree_ref())? {
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
        if self.git.branch_exists(repository_path, lease.branch())? {
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
    /// The persisted worktree reference is not permission by itself: the
    /// derived `<root>/ws-<slug>` must equal it, and a live path must
    /// canonicalize inside the managed root. Traversal or symlink escape
    /// fails closed.
    fn validate_managed_path(&self, lease: &WorkspaceLease) -> Result<PathBuf, WorkspaceFailure> {
        let expected = self.root.join_workspace(lease.id())?;
        if expected.to_string_lossy().as_ref() != lease.worktree_ref() {
            return Err(WorkspaceFailure::OwnershipMismatch {
                detail: format!(
                    "lease {} worktree reference disagrees with the managed root; refusing",
                    lease.id().as_str()
                ),
            });
        }
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
        if repository_path.trim().is_empty() {
            return Err(WorkspaceFailure::NotRepository {
                detail: "workspace cleanup requires a repository path".to_owned(),
            });
        }
        if !self.git.is_repository(repository_path)? {
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
                .resolve_revision(repository_path, "HEAD")
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
        if !self
            .git
            .is_registered_worktree(repository_path, lease.worktree_ref())?
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
        match self
            .git
            .resolve_revision(repository_path, lease.base_revision().as_str())
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
        match self.git.default_branch(repository_path) {
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
        if !self.git.is_repository(&request.repository_path)? {
            return Err(WorkspaceFailure::NotRepository {
                detail: format!(
                    "workspace allocation path is not a repository: {}",
                    request.repository_path
                ),
            });
        }
        let base = self
            .git
            .resolve_revision(&request.repository_path, &request.base_revision)?;
        let now = self.clock.now_millis();
        self.op_counter += 1;
        let id = WorkspaceId::new(format!(
            "ws-{now}-{}-{}",
            std::process::id(),
            LEASE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
        .map_err(domain_failure)?;
        let branch = branch_for_execution(&request.execution_id)?;
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
        self.store.insert_allocated_lease(&lease)?;
        Ok(lease)
    }

    /// Step 3 of the saga (crash-A/B seam): run the single Git mutation
    /// for an `Allocated` lease. No SQLite is touched here — not even a
    /// read transaction spanning the subprocess — so a crash inside Git
    /// leaves exactly the durable intent plus whatever Git did, both
    /// reconcilable. Stale lease handles (superseded objects) are refused
    /// before any mutation.
    pub fn materialize_allocated(
        &self,
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
        let target = self.root.join_workspace(lease.id())?;
        if target.to_string_lossy().as_ref() != lease.worktree_ref() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "managed root changed under lease {}; refusing",
                    lease.id().as_str()
                ),
            });
        }
        let target_text = target
            .to_str()
            .ok_or_else(|| WorkspaceFailure::StorageFailure {
                detail: "derived workspace target is not UTF-8".to_owned(),
            })?;
        self.git
            .create_worktree(lease, repository_path, target_text)?;
        Ok(())
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
        self.verify_git_consistent(&lease, repository_path)?;
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
        // Request/lease agreement: the same execution must ask for the
        // same repository and base it already owns. Anything else is an
        // ownership conflict, never a silent rebind.
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
        let requested_base = self
            .git
            .resolve_revision(&request.repository_path, &request.base_revision)?;
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
        match lease.lifecycle() {
            LeaseLifecycle::Allocated => {
                if self
                    .verify_git_consistent(&lease, &request.repository_path)
                    .is_ok()
                {
                    // Crash-B shape: Git did the work, finalization never
                    // ran. Recognize — do not create anything new — and let
                    // the guarded transition preserve exactly-once created.
                    let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
                    Ok(EnsuredWorkspace {
                        lease: ready,
                        outcome: EnsureOutcome::RecognizedGitState,
                    })
                } else if !self.side_effects_present(&lease, &request.repository_path) {
                    // Crash-A shape: intent only, Git untouched. Resume.
                    self.materialize_allocated(&lease, &request.repository_path)?;
                    let ready = self.finalize_ready(lease.id(), &request.repository_path)?;
                    Ok(EnsuredWorkspace {
                        lease: ready,
                        outcome: EnsureOutcome::ResumedAllocated,
                    })
                } else {
                    // Crash-C shape: something happened, but not the exact
                    // expected state. No replay, no cleanup, no guessing.
                    Err(WorkspaceFailure::CheckoutFailed {
                        detail: format!(
                            "lease {} has ambiguous partial Git state; refusing blind replay; \
                             inspect explicitly (cleanup owned by TASK-0043)",
                            lease.id().as_str()
                        ),
                    })
                }
            }
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
                self.verify_git_consistent(&lease, &request.repository_path)?;
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

    /// Independently observes that live Git matches the durable lease:
    /// managed branch exists and points at the base, the worktree HEAD is
    /// the base, and the worktree observes attached (never Missing or
    /// Invalid). Dirt is accepted — it is new information, not creation
    /// inconsistency.
    fn verify_git_consistent(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        let inconsistent = |detail: String| WorkspaceFailure::CheckoutFailed { detail };
        if !self.git.branch_exists(repository_path, lease.branch())? {
            return Err(inconsistent(format!(
                "managed branch {} absent for lease {}",
                lease.branch().as_str(),
                lease.id().as_str()
            )));
        }
        let at_branch = self
            .git
            .resolve_revision(repository_path, lease.branch().as_str())?;
        if at_branch != *lease.base_revision() {
            return Err(inconsistent(format!(
                "managed branch {} points at {} for lease {}, expected base {}",
                lease.branch().as_str(),
                at_branch.as_str(),
                lease.id().as_str(),
                lease.base_revision().as_str()
            )));
        }
        let at_worktree = self.git.resolve_revision(lease.worktree_ref(), "HEAD")?;
        if at_worktree != *lease.base_revision() {
            return Err(inconsistent(format!(
                "worktree HEAD is {} for lease {}, expected base {}",
                at_worktree.as_str(),
                lease.id().as_str(),
                lease.base_revision().as_str()
            )));
        }
        match self.git.worktree_status(lease.worktree_ref())? {
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

    /// Conservative side-effect probe for the crash-A vs crash-C decision:
    /// only "branch absent AND target absent" counts as untouched. Backend
    /// errors and unusable states count as present (fail-closed direction).
    fn side_effects_present(&self, lease: &WorkspaceLease, repository_path: &str) -> bool {
        let branch_present = self
            .git
            .branch_exists(repository_path, lease.branch())
            .unwrap_or(true);
        if branch_present {
            return true;
        }
        self.git
            .worktree_status(lease.worktree_ref())
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
            correlation_id: format!("op-{now}-{}", self.op_counter),
            causation_id: format!("req-{now}-{}", self.op_counter),
            occurred_at: format_rfc3339(now),
            provenance: format!("{PROVENANCE_SERVICE}/{op}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ] {
            let body = slug_body(raw);
            assert!(!body.is_empty(), "slug for {raw:?} must not be empty");
            assert!(!body.contains('/'), "slug for {raw:?} must not separate");
            assert!(!body.contains('\\'), "slug for {raw:?} must not separate");
            assert!(!body.contains(' '), "slug for {raw:?} must not space");
            assert_ne!(body, ".", "slug for {raw:?} must not be dot");
            assert_ne!(body, "..", "slug for {raw:?} must not be dotdot");
        }
        // Short identities stay readable and injective.
        assert_eq!(slug_body("E-1"), "E-1");
        assert_ne!(slug_body("a/b"), slug_body("a-b"));
        assert_ne!(slug_body("a/b"), slug_body("a_b"));
        // Long identities stay bounded and deterministic.
        let long = "x".repeat(300);
        let hashed = slug_body(&long);
        assert!(hashed.len() < 32);
        assert_eq!(hashed, slug_body(&long));
    }

    #[test]
    fn branch_derivation_is_deterministic_and_valid() {
        let owner = ExecutionId::new("E-9").unwrap();
        let first = branch_for_execution(&owner).unwrap();
        let second = branch_for_execution(&owner).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.as_str(), "refs/heads/agent/E-9");
        let hostile = ExecutionId::new("../../x").unwrap();
        let derived = branch_for_execution(&hostile).unwrap();
        assert!(derived.as_str().starts_with("refs/heads/agent/"));
        // The slug after the fixed `agent/` prefix is a single component:
        // traversal and separators are encoded away, and the full short name
        // never leads with a dash (it always leads with `agent/`).
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
        assert!(target.ends_with("ws-ws-1"));

        assert!(ManagedWorkspaceRoot::new("relative/path").is_err());
        assert!(ManagedWorkspaceRoot::new("/nonexistent-root-for-task0042").is_err());
    }
}
