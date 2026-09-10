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
    ExecutionId, LeaseLifecycle, WorkspaceFailure, WorkspaceId, WorkspaceJournalEvidence,
    WorkspaceLease, WorkspaceLeaseSnapshot, WorkspaceStore,
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
