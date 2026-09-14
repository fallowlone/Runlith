//! Durable WorkspaceLease ownership: the [`workspace::WorkspaceStore`]
//! implementation over SQLite (migration 0005).
//!
//! Split discipline: this module knows SQL rows and journal mechanics and
//! nothing about Git commands or filesystem reconciliation — all Git truth
//! flows through `GitPort` at the service layer, never here. State changes
//! that require journal evidence (Ready, Removed) commit the row update
//! together with the event in one SQLite transaction; guarded `WHERE
//! lifecycle = ...` clauses make concurrent movers fail instead of
//! silently overwriting newer state.

use rusqlite::{params, OptionalExtension};
use workspace::{
    ExecutionId, LeaseLifecycle, TrustedCommonGit, WorkspaceFailure, WorkspaceId,
    WorkspaceJournalEvidence, WorkspaceLease, WorkspaceLeaseSnapshot, WorkspaceStore,
};

use crate::error::StorageError;
use crate::event::{EventType, NewJournalEvent};
use crate::task_store::{insert_journal_event, require_envelope};

/// Maps a storage-local failure across the port boundary. The trait error
/// is [`WorkspaceFailure`]; every SQLite, constraint, and corruption
/// failure becomes [`WorkspaceFailure::StorageFailure`] with its precise
/// detail preserved. Accepts both [`StorageError`] and [`rusqlite::Error`]
/// so call sites map directly.
fn storage_failure(error: impl Into<StorageError>) -> WorkspaceFailure {
    WorkspaceFailure::StorageFailure {
        detail: error.into().to_string(),
    }
}

fn corrupt_lease(id: &str, detail: impl std::fmt::Display) -> WorkspaceFailure {
    WorkspaceFailure::StorageFailure {
        detail: format!("corrupt persisted lease {id}: {detail}"),
    }
}

struct LeaseRow {
    workspace_id: String,
    execution_id: String,
    repository_ref: String,
    base_revision: String,
    branch_ref: String,
    worktree_ref: String,
    lifecycle: String,
}

impl LeaseRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            workspace_id: row.get(0)?,
            execution_id: row.get(1)?,
            repository_ref: row.get(2)?,
            base_revision: row.get(3)?,
            branch_ref: row.get(4)?,
            worktree_ref: row.get(5)?,
            lifecycle: row.get(6)?,
        })
    }

    fn snapshot(self) -> WorkspaceLeaseSnapshot {
        WorkspaceLeaseSnapshot {
            workspace_id: self.workspace_id,
            repository_ref: self.repository_ref,
            execution_id: self.execution_id,
            base_revision: self.base_revision,
            branch_ref: self.branch_ref,
            worktree_ref: self.worktree_ref,
            lifecycle: self.lifecycle,
        }
    }
}

impl crate::task_store::SqliteStore {
    /// Decodes one lease row through domain validation. A row whose owner
    /// vanished is corruption (foreign keys stop this on write; this stops
    /// it on read when the database was mutated behind the adapter).
    fn decode_lease(&self, row: LeaseRow) -> Result<WorkspaceLease, WorkspaceFailure> {
        let id = row.workspace_id.clone();
        let lease =
            WorkspaceLease::restore(row.snapshot()).map_err(|error| corrupt_lease(&id, error))?;
        let owner_exists: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM executions WHERE id = ?1)",
                params![lease.owner().as_str()],
                |row| row.get(0),
            )
            .map_err(storage_failure)?;
        if !owner_exists {
            return Err(corrupt_lease(
                &id,
                format!("references missing execution {}", lease.owner()),
            ));
        }
        Ok(lease)
    }

    fn load_lease_row(&self, id: &WorkspaceId) -> Result<Option<LeaseRow>, WorkspaceFailure> {
        self.connection()
            .query_row(
                "SELECT workspace_id, execution_id, repository_ref, base_revision,
                        branch_ref, worktree_ref, lifecycle
                 FROM workspace_leases WHERE workspace_id = ?1",
                params![id.as_str()],
                LeaseRow::read,
            )
            .optional()
            .map_err(storage_failure)
    }

    /// Returns the owning execution of any lease recording `branch`, if
    /// any — compared case-fold-insensitively. Read-only fail-fast helper
    /// for the execution-entry boundary (TASK-0053, G5 B-CF): with
    /// case-fold-safe v2 derivation two distinct executions never share a
    /// folded branch, so a hit owned by another execution refuses entry
    /// before the first mutation. The explicit ASCII fold (`COLLATE NOCASE`
    /// is ASCII-only, matching the ASCII-only managed alphabet) also lets
    /// historical mixed-case leases participate in detection. Not a
    /// correctness boundary by itself — Git state can still change
    /// afterwards; that stays a recoverable saga concern. The atomic
    /// admission transaction re-checks inside its own transaction, which
    /// is the correctness boundary under concurrency.
    pub fn find_lease_owner_by_branch(
        &self,
        branch: &str,
    ) -> Result<Option<ExecutionId>, WorkspaceFailure> {
        let owner: Option<String> = self
            .connection()
            .query_row(
                "SELECT execution_id FROM workspace_leases WHERE branch_ref = ?1 COLLATE NOCASE",
                params![branch],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_failure)?;
        owner
            .map(|raw| {
                ExecutionId::new(raw).map_err(|error| WorkspaceFailure::StorageFailure {
                    detail: format!("stored lease owner is not a valid execution id: {error}"),
                })
            })
            .transpose()
    }

    /// Returns the owning execution of any lease in `repository` recording
    /// a case-fold-equal branch, if any. Repository-scoped companion to
    /// [`Self::find_lease_owner_by_branch`] for the lease-allocation
    /// boundary: a direct allocation (bypassing admission) must never
    /// attach a second execution to one managed branch of one repository.
    pub fn find_lease_owner_in_repository(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<Option<ExecutionId>, WorkspaceFailure> {
        let owner: Option<String> = self
            .connection()
            .query_row(
                "SELECT execution_id FROM workspace_leases
                 WHERE repository_ref = ?1 AND branch_ref = ?2 COLLATE NOCASE",
                params![repository, branch],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_failure)?;
        owner
            .map(|raw| {
                ExecutionId::new(raw).map_err(|error| WorkspaceFailure::StorageFailure {
                    detail: format!("stored lease owner is not a valid execution id: {error}"),
                })
            })
            .transpose()
    }

    /// Returns the sole lease owner claiming `worktree_ref`. Duplicate
    /// claims are corruption and fail closed rather than picking one row.
    pub fn find_lease_owner_by_worktree_ref(
        &self,
        worktree_ref: &str,
    ) -> Result<Option<ExecutionId>, WorkspaceFailure> {
        let mut statement = self
            .connection()
            .prepare("SELECT execution_id FROM workspace_leases WHERE worktree_ref = ?1 LIMIT 2")
            .map_err(storage_failure)?;
        let mut rows = statement
            .query(params![worktree_ref])
            .map_err(storage_failure)?;
        let Some(first) = rows.next().map_err(storage_failure)? else {
            return Ok(None);
        };
        let owner: String = first.get(0).map_err(storage_failure)?;
        if rows.next().map_err(storage_failure)?.is_some() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "multiple workspace leases claim worktree reference {worktree_ref}"
                ),
            });
        }
        ExecutionId::new(owner)
            .map(Some)
            .map_err(|error| WorkspaceFailure::StorageFailure {
                detail: format!("stored lease owner is not a valid execution id: {error}"),
            })
    }

    /// Returns the owning execution of one managed-identity reservation,
    /// if any. Read-only helper for entry preflight diagnostics and tests;
    /// the atomic admission transaction is the correctness boundary.
    pub fn find_identity_reservation_owner(
        &self,
        branch: &str,
    ) -> Result<Option<ExecutionId>, WorkspaceFailure> {
        let owner: Option<String> = self
            .connection()
            .query_row(
                "SELECT execution_id FROM managed_identity_reservations WHERE branch_ref = ?1",
                params![branch],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_failure)?;
        owner
            .map(|raw| {
                ExecutionId::new(raw).map_err(|error| WorkspaceFailure::StorageFailure {
                    detail: format!(
                        "stored reservation owner is not a valid execution id: {error}"
                    ),
                })
            })
            .transpose()
    }

    /// Returns the reserved branch for one execution, if any.
    /// Read-only helper for same-execution idempotency checks.
    pub fn find_identity_reservation_branch(
        &self,
        execution: &ExecutionId,
    ) -> Result<Option<String>, WorkspaceFailure> {
        self.connection()
            .query_row(
                "SELECT branch_ref FROM managed_identity_reservations WHERE execution_id = ?1",
                params![execution.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_failure)
    }

    /// Claims the managed branch for one execution (`origin`: `admission`
    /// or `lease`; the CHECK constraint freezes the vocabulary).
    /// Idempotent for the owning execution; a branch reserved by another
    /// execution refuses as typed [`WorkspaceFailure::BranchCollision`];
    /// an execution attempting a second, different branch refuses as a
    /// storage inconsistency (one execution owns at most one lease).
    /// The single INSERT plus the PRIMARY KEY / UNIQUE backstops is the
    /// concurrency guard: concurrent claimants serialize on the
    /// constraints, and every loser fails closed even after passing the
    /// read-before-write checks above.
    pub fn claim_managed_branch(
        &mut self,
        branch: &str,
        execution: &ExecutionId,
        origin: &str,
    ) -> Result<(), WorkspaceFailure> {
        match workspace::BranchRef::new(branch) {
            Ok(parsed) if parsed.is_managed() => {}
            _ => {
                return Err(WorkspaceFailure::StorageFailure {
                    detail: "managed branch claim requires a well-formed managed branch".to_owned(),
                });
            }
        }
        if let Some(owner) = self.find_identity_reservation_owner(branch)? {
            if owner == *execution {
                return Ok(());
            }
            return Err(WorkspaceFailure::BranchCollision {
                detail: format!(
                    "managed branch {branch} is already reserved by execution {owner}; \
                     refusing reuse"
                ),
            });
        }
        if let Some(held) = self.find_identity_reservation_branch(execution)? {
            // Sequential detection: this execution already holds a
            // reservation (normally the admission claim for its derived
            // branch). A different branch here is an internal
            // inconsistency, never a second valid claim.
            if held != branch {
                return Err(WorkspaceFailure::StorageFailure {
                    detail: format!(
                        "execution {} already holds managed reservation {held}; \
                         refusing second reservation {branch}",
                        execution.as_str(),
                    ),
                });
            }
            return Ok(());
        }
        self.connection()
            .execute(
                "INSERT INTO managed_identity_reservations (branch_ref, execution_id, origin)
                 VALUES (?1, ?2, ?3)",
                params![branch, execution.as_str(), origin],
            )
            .map_err(|error| match &error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    // Residual race: a concurrent claimant committed
                    // between the checks above and this insert. Re-read
                    // for an honest owner-naming message; still
                    // fail-closed when the winner is unreadable.
                    match self.find_identity_reservation_owner(branch) {
                        Ok(Some(owner)) => WorkspaceFailure::BranchCollision {
                            detail: format!(
                                "managed branch {branch} is already reserved by execution {owner}; \
                                 refusing reuse"
                            ),
                        },
                        _ => WorkspaceFailure::BranchCollision {
                            detail: format!(
                                "managed branch {branch} conflicts with an existing reservation; \
                                 refusing reuse"
                            ),
                        },
                    }
                }
                _ => storage_failure(crate::error::map_constraint(error, "claim managed branch")),
            })?;
        Ok(())
    }

    /// Translates service-minted workspace evidence into a journal fact.
    /// The lifecycle pair always comes from the evidence kind's fixed
    /// transition — never from caller input.
    fn workspace_fact(evidence: &WorkspaceJournalEvidence) -> NewJournalEvent {
        let (from, to) = evidence.kind.transition();
        NewJournalEvent {
            event_type: match evidence.kind {
                workspace::WorkspaceEventKind::Created => EventType::WorkspaceCreated,
                workspace::WorkspaceEventKind::Removed => EventType::WorkspaceRemoved,
            },
            task_id: Some(evidence.task_id.clone()),
            project_id: evidence.project_id.clone(),
            from_state: None,
            to_state: None,
            reason: None,
            correlation_id: evidence.correlation_id.clone(),
            causation_id: evidence.causation_id.clone(),
            occurred_at: evidence.occurred_at.clone(),
            provenance: evidence.provenance.clone(),
            execution_id: Some(evidence.execution_id.clone()),
            attempt_id: None,
            attempt_outcome: None,
            workspace_id: Some(evidence.workspace_id.clone()),
            from_workspace_state: Some(from),
            to_workspace_state: Some(to),
        }
    }
}

impl WorkspaceStore for crate::task_store::SqliteStore {
    fn get_workspace(&self, id: &WorkspaceId) -> Result<Option<WorkspaceLease>, WorkspaceFailure> {
        self.load_lease_row(id)?
            .map(|row| self.decode_lease(row))
            .transpose()
    }

    fn get_workspace_for_execution(
        &self,
        owner: &ExecutionId,
    ) -> Result<Option<WorkspaceLease>, WorkspaceFailure> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT workspace_id, execution_id, repository_ref, base_revision,
                        branch_ref, worktree_ref, lifecycle
                 FROM workspace_leases WHERE execution_id = ?1",
            )
            .map_err(storage_failure)?;
        let rows = statement
            .query_map(params![owner.as_str()], LeaseRow::read)
            .map_err(storage_failure)?;
        let mut leases = Vec::new();
        for row in rows {
            leases.push(self.decode_lease(row.map_err(storage_failure)?)?);
        }
        match leases.len() {
            0 => Ok(None),
            1 => Ok(Some(leases.pop().expect("checked length"))),
            _ => Err(corrupt_lease(
                "?",
                format!("execution {owner} owns multiple leases despite UNIQUE(execution_id)"),
            )),
        }
    }

    fn has_workspace_event(
        &self,
        id: &WorkspaceId,
        kind: workspace::WorkspaceEventKind,
    ) -> Result<bool, WorkspaceFailure> {
        let present: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM event_journal
                 WHERE workspace_id = ?1 AND event_type = ?2)",
                params![id.as_str(), kind.as_str()],
                |row| row.get(0),
            )
            .map_err(storage_failure)?;
        Ok(present)
    }

    fn insert_allocated_lease(&mut self, lease: &WorkspaceLease) -> Result<(), WorkspaceFailure> {
        if lease.lifecycle() != LeaseLifecycle::Allocated {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is not an Allocated intent; only intents are inserted",
                    lease.id()
                ),
            });
        }
        if self.load_lease_row(lease.id())?.is_some() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!("workspace lease {} already exists", lease.id()),
            });
        }
        if self.get_workspace_for_execution(lease.owner())?.is_some() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!("execution {} already owns a workspace lease", lease.owner()),
            });
        }
        let owner_exists: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM executions WHERE id = ?1)",
                params![lease.owner().as_str()],
                |row| row.get(0),
            )
            .map_err(storage_failure)?;
        if !owner_exists {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "owning execution {} does not exist for lease {}",
                    lease.owner(),
                    lease.id()
                ),
            });
        }
        // Managed-identity backstop (TASK-0053 B-CF): claim the lease
        // branch for its owner before inserting. Admission-path leases
        // already own their reservation (verified, idempotent); direct
        // allocation paths register here, so a second execution can never
        // attach to one managed branch — concurrently or otherwise. A
        // same-repository folded conflict against existing leases refuses
        // with the same typed collision (explicit ASCII fold for
        // historical mixed-case rows; v2 namespaces are disjoint from
        // them by construction, so this is defense in depth).
        self.claim_managed_branch(lease.branch().as_str(), lease.owner(), "lease")?;
        if let Some(owner) = self
            .find_lease_owner_in_repository(lease.repository().as_str(), lease.branch().as_str())?
        {
            if owner != *lease.owner() {
                return Err(WorkspaceFailure::BranchCollision {
                    detail: format!(
                        "managed branch {} is already owned by execution {} in repository {}; \
                         refusing reuse",
                        lease.branch().as_str(),
                        owner.as_str(),
                        lease.repository().as_str()
                    ),
                });
            }
        }
        let snapshot = lease.snapshot();
        self.connection()
            .execute(
                "INSERT INTO workspace_leases
                 (workspace_id, execution_id, repository_ref, base_revision,
                  branch_ref, worktree_ref, lifecycle)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    snapshot.workspace_id,
                    snapshot.execution_id,
                    snapshot.repository_ref,
                    snapshot.base_revision,
                    snapshot.branch_ref,
                    snapshot.worktree_ref,
                    snapshot.lifecycle,
                ],
            )
            .map_err(|error| {
                storage_failure(crate::error::map_constraint(
                    error,
                    "insert workspace lease",
                ))
            })?;
        Ok(())
    }

    fn insert_allocated_lease_with_anchor(
        &mut self,
        lease: &WorkspaceLease,
        anchor: &TrustedCommonGit,
        captured_at_millis: u64,
    ) -> Result<(), WorkspaceFailure> {
        if lease.lifecycle() != LeaseLifecycle::Allocated {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {} is not an Allocated intent; only intents are inserted",
                    lease.id()
                ),
            });
        }
        // u64 millis are always non-negative; SQLite CHECK enforces >= 0.
        let anchor_text = anchor.as_str()?.to_owned();
        if self.load_lease_row(lease.id())?.is_some() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!("workspace lease {} already exists", lease.id()),
            });
        }
        if self.get_workspace_for_execution(lease.owner())?.is_some() {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!("execution {} already owns a workspace lease", lease.owner()),
            });
        }
        let owner_exists: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM executions WHERE id = ?1)",
                params![lease.owner().as_str()],
                |row| row.get(0),
            )
            .map_err(storage_failure)?;
        if !owner_exists {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "owning execution {} does not exist for lease {}",
                    lease.owner(),
                    lease.id()
                ),
            });
        }
        self.claim_managed_branch(lease.branch().as_str(), lease.owner(), "lease")?;
        if let Some(owner) = self
            .find_lease_owner_in_repository(lease.repository().as_str(), lease.branch().as_str())?
        {
            if owner != *lease.owner() {
                return Err(WorkspaceFailure::BranchCollision {
                    detail: format!(
                        "managed branch {} is already owned by execution {} in repository {}; refusing reuse",
                        lease.branch().as_str(),
                        owner.as_str(),
                        lease.repository().as_str()
                    ),
                });
            }
        }
        let snapshot = lease.snapshot();
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_failure)?;
        transaction
            .execute(
                "INSERT INTO workspace_leases
                 (workspace_id, execution_id, repository_ref, base_revision,
                  branch_ref, worktree_ref, lifecycle)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    snapshot.workspace_id,
                    snapshot.execution_id,
                    snapshot.repository_ref,
                    snapshot.base_revision,
                    snapshot.branch_ref,
                    snapshot.worktree_ref,
                    snapshot.lifecycle,
                ],
            )
            .map_err(|error| {
                storage_failure(crate::error::map_constraint(
                    error,
                    "insert workspace lease",
                ))
            })?;
        let captured_at: i64 =
            i64::try_from(captured_at_millis).map_err(|_| WorkspaceFailure::StorageFailure {
                detail: "anchor capture time out of range".to_owned(),
            })?;
        transaction
            .execute(
                "INSERT INTO workspace_trusted_anchor
                 (workspace_id, canonical_commondir, captured_at_millis, capture_provenance)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    snapshot.workspace_id,
                    anchor_text,
                    captured_at,
                    "workspace-application-service/allocate_workspace",
                ],
            )
            .map_err(|error| {
                storage_failure(crate::error::map_constraint(error, "insert trusted anchor"))
            })?;
        transaction.commit().map_err(storage_failure)?;
        Ok(())
    }

    fn get_trusted_anchor(
        &self,
        id: &WorkspaceId,
    ) -> Result<Option<TrustedCommonGit>, WorkspaceFailure> {
        let row: Option<String> = self
            .connection()
            .query_row(
                "SELECT canonical_commondir FROM workspace_trusted_anchor WHERE workspace_id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_failure)?;
        let Some(canonical) = row else {
            return Ok(None);
        };
        match TrustedCommonGit::restore(&canonical) {
            Ok(anchor) => Ok(Some(anchor)),
            Err(e) => Err(e),
        }
    }

    fn mark_lease_ready(
        &mut self,
        id: &WorkspaceId,
        evidence: &WorkspaceJournalEvidence,
    ) -> Result<(), WorkspaceFailure> {
        if evidence.kind != workspace::WorkspaceEventKind::Created {
            return Err(WorkspaceFailure::StorageFailure {
                detail: "ready transition requires workspace.created evidence".to_owned(),
            });
        }
        if evidence.workspace_id != *id {
            return Err(WorkspaceFailure::StorageFailure {
                detail: "workspace.created evidence references a different lease".to_owned(),
            });
        }
        let fact = Self::workspace_fact(evidence);
        require_envelope(&fact).map_err(storage_failure)?;
        // IMMEDIATE (not DEFERRED): concurrent daemon writers must
        // serialize at BEGIN instead of deadlocking on lock upgrade
        // (TASK-0047; both sides holding RESERVED breaks the busy wait).
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_failure)?;
        let moved = transaction
            .execute(
                "UPDATE workspace_leases SET lifecycle = 'READY'
                 WHERE workspace_id = ?1 AND lifecycle = 'ALLOCATED'",
                params![id.as_str()],
            )
            .map_err(storage_failure)?;
        if moved != 1 {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!(
                    "lease {id} is not Allocated; a concurrent mover won or the lease is missing"
                ),
            });
        }
        insert_journal_event(&transaction, &fact).map_err(storage_failure)?;
        transaction.commit().map_err(storage_failure)?;
        Ok(())
    }

    fn mark_lease_cleanup_pending(&mut self, id: &WorkspaceId) -> Result<(), WorkspaceFailure> {
        let moved = self
            .connection()
            .execute(
                "UPDATE workspace_leases SET lifecycle = 'CLEANUP_PENDING'
                 WHERE workspace_id = ?1 AND lifecycle = 'READY'",
                params![id.as_str()],
            )
            .map_err(storage_failure)?;
        if moved != 1 {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!("lease {id} is not Ready; cleanup cannot be requested"),
            });
        }
        Ok(())
    }

    fn mark_lease_removed(
        &mut self,
        id: &WorkspaceId,
        evidence: &WorkspaceJournalEvidence,
    ) -> Result<(), WorkspaceFailure> {
        if evidence.kind != workspace::WorkspaceEventKind::Removed {
            return Err(WorkspaceFailure::StorageFailure {
                detail: "removed transition requires workspace.removed evidence".to_owned(),
            });
        }
        if evidence.workspace_id != *id {
            return Err(WorkspaceFailure::StorageFailure {
                detail: "workspace.removed evidence references a different lease".to_owned(),
            });
        }
        let fact = Self::workspace_fact(evidence);
        require_envelope(&fact).map_err(storage_failure)?;
        // IMMEDIATE (not DEFERRED): concurrent daemon writers must
        // serialize at BEGIN instead of deadlocking on lock upgrade
        // (TASK-0047; both sides holding RESERVED breaks the busy wait).
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_failure)?;
        let moved = transaction
            .execute(
                "UPDATE workspace_leases SET lifecycle = 'REMOVED'
                 WHERE workspace_id = ?1 AND lifecycle = 'CLEANUP_PENDING'",
                params![id.as_str()],
            )
            .map_err(storage_failure)?;
        if moved != 1 {
            return Err(WorkspaceFailure::StorageFailure {
                detail: format!("lease {id} is not CleanupPending; removal cannot be recorded"),
            });
        }
        insert_journal_event(&transaction, &fact).map_err(storage_failure)?;
        transaction.commit().map_err(storage_failure)?;
        Ok(())
    }
}
