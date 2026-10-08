//! Durable TAKEOVER continuation persistence (TASK-0172).
//!
//! Stores the `TakeoverAdmission` evidence record alongside HDR state:
//! fenced admission, human-authored appends, return completion and
//! expiry abandonment, each with its workspace-anchor snapshot.

use plan_domain::{HumanContinuation, HumanDecisionRequestId, HumanDecisionStatus, PlanBranchId};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use security_audit::{SecurityAuditEvent, SecurityAuditOutcome};
use takeover::{
    AuthoredField, TakeoverAdmission, TakeoverLeaseAnchor, TakeoverRecordId, TakeoverStatus,
};
use task_domain::{TaskId, TaskState};

use crate::error::{StorageError, StorageResult};
use crate::event::NewJournalEvent;
use crate::human_decision_store::{
    append_human_decision_audit, load_request, to_i64, validate_record_current,
    HumanDecisionAuditFact,
};
use crate::journal_store::{insert_journal_event, require_envelope};
use crate::planning_control_store::{load_current_run, PlanningRunState};
use crate::task_store::SqliteStore;

/// Fenced TAKEOVER admission: consumes a Resolved TAKEOVER decision and
/// parks the task RUNNING→BLOCKED with the admission record, atomically.
#[derive(Clone, Debug)]
pub struct AdmitTakeover {
    pub hdr_id: HumanDecisionRequestId,
    pub expires_at_ms: u64,
    pub park_event: NewJournalEvent,
    pub now_ms: u64,
}

/// Human-authored appends to an ACTIVE record. Each provided field
/// overwrites its column trio; absent fields are untouched. Appends
/// accumulate across calls, last-wins per field.
#[derive(Clone, Debug)]
pub struct AppendTakeover {
    pub record_id: TakeoverRecordId,
    pub actions: Option<AuthoredField>,
    pub uncertainty: Option<AuthoredField>,
    pub question: Option<AuthoredField>,
    pub resulting_revision: Option<u64>,
    pub now_ms: u64,
}

/// Return completion after a successful TriggerPlanning cycle: the
/// anchor recheck, the BLOCKED→RUNNING unpark and the COMPLETED mark,
/// atomically. A record that expired mid-cycle abandons instead.
#[derive(Clone, Debug)]
pub struct CompleteTakeoverReturn {
    pub record_id: TakeoverRecordId,
    pub unpark_event: NewJournalEvent,
    pub now_ms: u64,
}

/// Outcome of [`SqliteStore::complete_takeover_return`].
#[derive(Clone, Debug)]
pub enum TakeoverCompletion {
    Completed(TakeoverAdmission),
    ExpiredAbandoned(TakeoverAdmission),
}

/// Expiry abandonment of an ACTIVE record: the BLOCKED→RUNNING unpark
/// with the record id plus the expiry marker as the resolution
/// reference, and the EXPIRED mark, atomically.
#[derive(Clone, Debug)]
pub struct AbandonTakeover {
    pub record_id: TakeoverRecordId,
    pub unpark_event: NewJournalEvent,
    pub now_ms: u64,
}

impl SqliteStore {
    /// Admits a TAKEOVER: every precondition is checked inside one
    /// IMMEDIATE transaction and any refusal leaves zero state change.
    /// On success the HDR is Consumed, the task is Blocked and an ACTIVE
    /// admission record with anchor snapshots is appended.
    pub fn admit_takeover(&mut self, params: AdmitTakeover) -> StorageResult<TakeoverAdmission> {
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = load_request(&tx, params.hdr_id.as_str())?.ok_or_else(|| {
            StorageError::NotFound(format!("human decision {}", params.hdr_id.as_str()))
        })?;
        // Terminal first: a Consumed decision is already fenced out of
        // supersession by construction, so it refuses with its stored
        // state even when its basis has also drifted (admission always
        // parks the task, which is drift by definition).
        if record.status == HumanDecisionStatus::Consumed {
            return Err(StorageError::HumanDecisionState {
                request_id: params.hdr_id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            });
        }
        validate_record_current(&tx, &record, params.now_ms)?;
        if record.status != HumanDecisionStatus::Resolved
            || record.selected != Some(HumanContinuation::Takeover)
        {
            return Err(StorageError::HumanDecisionState {
                request_id: params.hdr_id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            });
        }
        let task = crate::task_store::load_task(&tx, &record.task_id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "takeover task {} is missing",
                record.task_id.as_str()
            ))
        })?;
        if task.state() != TaskState::Running {
            return Err(StorageError::HumanDecisionState {
                request_id: params.hdr_id.as_str().to_owned(),
                state: "TAKEOVER_TASK_NOT_RUNNING".to_owned(),
            });
        }
        if let Some(run) = load_current_run(&tx, &record.plan_branch_id)? {
            if run.state == PlanningRunState::Active {
                return Err(StorageError::HumanDecisionState {
                    request_id: params.hdr_id.as_str().to_owned(),
                    state: "TAKEOVER_ACTIVE_RUN".to_owned(),
                });
            }
        }
        let open: Option<String> = tx
            .query_row(
                "SELECT a.attempt_id FROM execution_attempts a
                 JOIN executions e ON e.id = a.execution_id
                 WHERE e.task_id = ?1 AND a.outcome IS NULL LIMIT 1",
                params![record.task_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if open.is_some() {
            return Err(StorageError::HumanDecisionState {
                request_id: params.hdr_id.as_str().to_owned(),
                state: "TAKEOVER_OPEN_ATTEMPT".to_owned(),
            });
        }
        let updated = tx.execute(
            "UPDATE human_decision_requests SET status = 'CONSUMED', consumed_at_ms = ?1
             WHERE request_id = ?2 AND status = 'RESOLVED' AND selected_continuation = 'TAKEOVER'",
            params![
                to_i64(params.now_ms, "consumed_at_ms")?,
                params.hdr_id.as_str(),
            ],
        )?;
        if updated != 1 {
            let state: Option<String> = tx
                .query_row(
                    "SELECT status FROM human_decision_requests WHERE request_id = ?1",
                    params![params.hdr_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            return Err(match state {
                None => {
                    StorageError::NotFound(format!("human decision {}", params.hdr_id.as_str()))
                }
                Some(state) => StorageError::HumanDecisionState {
                    request_id: params.hdr_id.as_str().to_owned(),
                    state,
                },
            });
        }
        check_park_event(&task, &params.park_event)?;
        let parked = tx.execute(
            "UPDATE tasks SET state = 'BLOCKED' WHERE id = ?1 AND state = 'RUNNING'",
            params![record.task_id.as_str()],
        )?;
        if parked == 0 {
            let current: Option<String> = tx
                .query_row(
                    "SELECT state FROM tasks WHERE id = ?1",
                    params![record.task_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            return Err(match current {
                None => StorageError::NotFound(format!("task {} does not exist", record.task_id)),
                Some(found) => StorageError::Stale {
                    task: record.task_id.to_string(),
                    expected: TaskState::Running.as_str().to_owned(),
                    found,
                },
            });
        }
        insert_journal_event(&tx, &params.park_event)?;
        let (seq, id) = mint_record_id(&tx)?;
        tx.execute(
            "INSERT INTO takeover_admissions
             (seq, record_id, hdr_id, task_id, plan_branch_id, basis_plan_revision,
              expires_at_ms, status, created_at_ms, resolved_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ACTIVE', ?8, NULL)",
            params![
                seq,
                id.as_str(),
                params.hdr_id.as_str(),
                record.task_id.as_str(),
                record.plan_branch_id.as_str(),
                to_i64(record.precondition.plan_head, "basis_plan_revision")?,
                to_i64(params.expires_at_ms, "expires_at_ms")?,
                to_i64(params.now_ms, "created_at_ms")?,
            ],
        )?;
        capture_anchors(&tx, &id, &record.task_id)?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionApplied,
                outcome: SecurityAuditOutcome::Consumed,
                id: &params.hdr_id,
                task_id: &record.task_id,
                policy_fingerprint: &record.precondition.policy_fingerprint,
                reason_code: HumanContinuation::Takeover.as_str(),
                occurred_at_ms: params.now_ms,
            },
        )?;
        tx.commit()?;
        load_record(self.connection(), id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!("takeover {} vanished after admission", id.as_str()))
        })
    }

    /// Reads one takeover record by identity, or `None` when absent.
    pub fn get_takeover(&self, id: &TakeoverRecordId) -> StorageResult<Option<TakeoverAdmission>> {
        load_record(self.connection(), id.as_str())
    }

    /// Reads the ACTIVE takeover record for a task/branch scope, if any.
    /// At most one exists: admission requires a Running task while an
    /// ACTIVE record implies a Blocked one.
    pub fn active_takeover_for_scope(
        &self,
        task_id: &TaskId,
        branch: &PlanBranchId,
    ) -> StorageResult<Option<TakeoverAdmission>> {
        let id: Option<String> = self
            .connection()
            .query_row(
                "SELECT record_id FROM takeover_admissions
                 WHERE task_id = ?1 AND plan_branch_id = ?2 AND status = 'ACTIVE'
                 ORDER BY seq DESC LIMIT 1",
                params![task_id.as_str(), branch.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        match id {
            None => Ok(None),
            Some(id) => load_record(self.connection(), &id),
        }
    }

    /// Reads the anchor snapshots captured at admission for a record.
    pub fn takeover_anchors(
        &self,
        id: &TakeoverRecordId,
    ) -> StorageResult<Vec<TakeoverLeaseAnchor>> {
        load_anchors(self.connection(), id.as_str())
    }

    /// Derives the live workspace bindings for every execution of a task
    /// in the same shape as the captured snapshots, for anchor comparison.
    pub fn live_takeover_anchors(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Vec<TakeoverLeaseAnchor>> {
        live_anchors(self.connection(), task_id)
    }

    /// Appends human-authored fields (and optionally the
    /// resulting-revision reference) to an ACTIVE record. An expired
    /// record abandons inside the same transaction and the append
    /// refuses with the stored EXPIRED state.
    pub fn append_takeover(&mut self, params: AppendTakeover) -> StorageResult<TakeoverAdmission> {
        for field in [&params.actions, &params.uncertainty, &params.question]
            .into_iter()
            .flatten()
        {
            field
                .validate()
                .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        }
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = load_record(&tx, params.record_id.as_str())?.ok_or_else(|| {
            StorageError::NotFound(format!("takeover {}", params.record_id.as_str()))
        })?;
        if record.status != TakeoverStatus::Active {
            return Err(StorageError::HumanDecisionState {
                request_id: record.hdr_id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            });
        }
        if params.now_ms > record.expires_at_ms {
            abandon_in_tx(&tx, &record, params.now_ms)?;
            tx.commit()?;
            return Err(StorageError::HumanDecisionState {
                request_id: record.hdr_id.as_str().to_owned(),
                state: TakeoverStatus::Expired.as_str().to_owned(),
            });
        }
        if let Some(revision) = params.resulting_revision {
            let accepted: Option<i64> = tx
                .query_row(
                    "SELECT revision_seq FROM plan_revisions
                     WHERE plan_branch_id = ?1 AND revision_seq = ?2",
                    params![
                        record.plan_branch_id.as_str(),
                        to_i64(revision, "revision")?
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            if accepted.is_none() {
                return Err(StorageError::ConstraintViolation(format!(
                    "takeover {} resulting revision {revision} is not an accepted revision of branch {}",
                    params.record_id.as_str(),
                    record.plan_branch_id.as_str(),
                )));
            }
            tx.execute(
                "UPDATE takeover_admissions SET resulting_revision = ?1 WHERE record_id = ?2",
                params![to_i64(revision, "revision")?, params.record_id.as_str()],
            )?;
        }
        append_field(&tx, &params.record_id, "actions", params.actions.as_ref())?;
        append_field(
            &tx,
            &params.record_id,
            "uncertainty",
            params.uncertainty.as_ref(),
        )?;
        append_field(&tx, &params.record_id, "question", params.question.as_ref())?;
        let policy = hdr_policy_fingerprint(&tx, &record)?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionApplied,
                outcome: SecurityAuditOutcome::Consumed,
                id: &record.hdr_id,
                task_id: &record.task_id,
                policy_fingerprint: &policy,
                reason_code: "TAKEOVER_APPENDED",
                occurred_at_ms: params.now_ms,
            },
        )?;
        tx.commit()?;
        load_record(self.connection(), params.record_id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "takeover {} vanished after append",
                params.record_id.as_str()
            ))
        })
    }

    /// Completes a takeover return: rechecks the workspace anchors
    /// against the live bindings, unparks the task and marks the record
    /// COMPLETED, atomically. Anchor drift refuses with
    /// `TakeoverAnchorMismatch` and the scope stays Blocked; a record
    /// that expired mid-cycle abandons instead of completing.
    pub fn complete_takeover_return(
        &mut self,
        params: CompleteTakeoverReturn,
    ) -> StorageResult<TakeoverCompletion> {
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = load_record(&tx, params.record_id.as_str())?.ok_or_else(|| {
            StorageError::NotFound(format!("takeover {}", params.record_id.as_str()))
        })?;
        if record.status != TakeoverStatus::Active {
            return Err(StorageError::HumanDecisionState {
                request_id: record.hdr_id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            });
        }
        if params.now_ms > record.expires_at_ms {
            abandon_in_tx(&tx, &record, params.now_ms)?;
            tx.commit()?;
            let abandoned =
                load_record(self.connection(), params.record_id.as_str())?.ok_or_else(|| {
                    StorageError::Corrupt(format!(
                        "takeover {} vanished after abandonment",
                        params.record_id.as_str()
                    ))
                })?;
            return Ok(TakeoverCompletion::ExpiredAbandoned(abandoned));
        }
        let captured = load_anchors(&tx, params.record_id.as_str())?;
        let live = live_anchors(&tx, &record.task_id)?;
        takeover::compare_anchors(&captured, &live).map_err(|error| {
            StorageError::TakeoverAnchorMismatch {
                record_id: params.record_id.as_str().to_owned(),
                detail: error.to_string(),
            }
        })?;
        let task = crate::task_store::load_task(&tx, &record.task_id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "takeover task {} is missing",
                record.task_id.as_str()
            ))
        })?;
        check_unpark_event(&task, &params.unpark_event)?;
        let unparked = tx.execute(
            "UPDATE tasks SET state = 'RUNNING' WHERE id = ?1 AND state = 'BLOCKED'",
            params![record.task_id.as_str()],
        )?;
        if unparked == 0 {
            return Err(unpark_write_refusal(&tx, &record.task_id)?);
        }
        insert_journal_event(&tx, &params.unpark_event)?;
        tx.execute(
            "UPDATE takeover_admissions SET status = 'COMPLETED', resolved_at_ms = ?1
             WHERE record_id = ?2",
            params![
                to_i64(params.now_ms, "resolved_at_ms")?,
                params.record_id.as_str()
            ],
        )?;
        let policy = hdr_policy_fingerprint(&tx, &record)?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionApplied,
                outcome: SecurityAuditOutcome::Consumed,
                id: &record.hdr_id,
                task_id: &record.task_id,
                policy_fingerprint: &policy,
                reason_code: "TAKEOVER_RETURNED",
                occurred_at_ms: params.now_ms,
            },
        )?;
        tx.commit()?;
        load_record(self.connection(), params.record_id.as_str())?
            .ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "takeover {} vanished after return",
                    params.record_id.as_str()
                ))
            })
            .map(TakeoverCompletion::Completed)
    }

    /// Abandons an expired ACTIVE record: unparks the task with the
    /// record id plus the expiry marker as the resolution reference and
    /// marks the record EXPIRED, atomically. A live record refuses.
    pub fn abandon_takeover(
        &mut self,
        params: AbandonTakeover,
    ) -> StorageResult<TakeoverAdmission> {
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = load_record(&tx, params.record_id.as_str())?.ok_or_else(|| {
            StorageError::NotFound(format!("takeover {}", params.record_id.as_str()))
        })?;
        if record.status != TakeoverStatus::Active {
            return Err(StorageError::HumanDecisionState {
                request_id: record.hdr_id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            });
        }
        if params.now_ms <= record.expires_at_ms {
            return Err(StorageError::ConstraintViolation(format!(
                "takeover {} has not expired",
                params.record_id.as_str()
            )));
        }
        let task = crate::task_store::load_task(&tx, &record.task_id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "takeover task {} is missing",
                record.task_id.as_str()
            ))
        })?;
        check_unpark_event(&task, &params.unpark_event)?;
        let unparked = tx.execute(
            "UPDATE tasks SET state = 'RUNNING' WHERE id = ?1 AND state = 'BLOCKED'",
            params![record.task_id.as_str()],
        )?;
        if unparked == 0 {
            return Err(unpark_write_refusal(&tx, &record.task_id)?);
        }
        insert_journal_event(&tx, &params.unpark_event)?;
        tx.execute(
            "UPDATE takeover_admissions SET status = 'EXPIRED', resolved_at_ms = ?1
             WHERE record_id = ?2",
            params![
                to_i64(params.now_ms, "resolved_at_ms")?,
                params.record_id.as_str()
            ],
        )?;
        let policy = hdr_policy_fingerprint(&tx, &record)?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionApplied,
                outcome: SecurityAuditOutcome::Consumed,
                id: &record.hdr_id,
                task_id: &record.task_id,
                policy_fingerprint: &policy,
                reason_code: "TAKEOVER_EXPIRED",
                occurred_at_ms: params.now_ms,
            },
        )?;
        tx.commit()?;
        load_record(self.connection(), params.record_id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "takeover {} vanished after abandonment",
                params.record_id.as_str()
            ))
        })
    }

    /// Lists ACTIVE records whose expiry passed, oldest first, optionally
    /// scoped to one task. The reconciler abandons each through
    /// [`SqliteStore::abandon_takeover`].
    pub fn expired_active_takeovers(
        &self,
        task_id: Option<&TaskId>,
        now_ms: u64,
    ) -> StorageResult<Vec<TakeoverAdmission>> {
        let now = to_i64(now_ms, "now")?;
        let ids: Vec<String> = match task_id {
            Some(task) => {
                let mut statement = self.connection().prepare(
                    "SELECT record_id FROM takeover_admissions
                     WHERE task_id = ?1 AND status = 'ACTIVE' AND expires_at_ms < ?2
                     ORDER BY seq",
                )?;
                let rows: Vec<String> = statement
                    .query_map(params![task.as_str(), now], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            }
            None => {
                let mut statement = self.connection().prepare(
                    "SELECT record_id FROM takeover_admissions
                     WHERE status = 'ACTIVE' AND expires_at_ms < ?1 ORDER BY seq",
                )?;
                let rows: Vec<String> = statement
                    .query_map(params![now], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            }
        };
        ids.into_iter()
            .map(|id| {
                load_record(self.connection(), &id)?.ok_or_else(|| {
                    StorageError::Corrupt(format!("takeover {id} vanished during expiry scan"))
                })
            })
            .collect()
    }
}

fn check_park_event(task: &task_domain::Task, event: &NewJournalEvent) -> StorageResult<()> {
    if event.event_type.is_project_scoped() {
        return Err(StorageError::ConstraintViolation(
            "takeover park cannot carry a project-scoped event".to_owned(),
        ));
    }
    if event.event_type.is_runtime_scoped() {
        return Err(StorageError::ConstraintViolation(
            "takeover park cannot carry a runtime event".to_owned(),
        ));
    }
    if event.task_id.as_ref() != Some(task.id())
        || event.project_id.as_ref() != Some(task.project_id())
    {
        return Err(StorageError::ConstraintViolation(
            "takeover park event references a different task or project than the state write"
                .to_owned(),
        ));
    }
    if event.from_state != Some(TaskState::Running) || event.to_state != Some(TaskState::Blocked) {
        return Err(StorageError::ConstraintViolation(
            "takeover park event states do not match the guarded RUNNING to BLOCKED write"
                .to_owned(),
        ));
    }
    require_envelope(event)
}

fn mint_record_id(connection: &Connection) -> StorageResult<(i64, TakeoverRecordId)> {
    let seq: i64 = connection.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM takeover_admissions",
        [],
        |row| row.get(0),
    )?;
    let id = TakeoverRecordId::new(format!("takeover-{seq}"))
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
    Ok((seq, id))
}

fn capture_anchors(
    tx: &rusqlite::Transaction<'_>,
    id: &TakeoverRecordId,
    task_id: &TaskId,
) -> StorageResult<()> {
    for binding in lease_bindings(tx, task_id)? {
        tx.execute(
            "INSERT INTO takeover_admission_anchors
             (record_id, execution_id, lease_id, base_revision, worktree_ref, lifecycle, anchor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id.as_str(),
                binding.execution_id,
                binding.lease_id,
                binding.base_revision,
                binding.worktree_ref,
                binding.lifecycle,
                binding.anchor,
            ],
        )?;
    }
    Ok(())
}

struct LeaseBinding {
    execution_id: String,
    lease_id: String,
    base_revision: String,
    worktree_ref: String,
    lifecycle: String,
    anchor: Option<String>,
}

/// Reads the live lease binding for every execution of a task that
/// holds a lease. Executions without a lease leave no row; derivation
/// is symmetric at capture and at return comparison.
fn lease_bindings(connection: &Connection, task_id: &TaskId) -> StorageResult<Vec<LeaseBinding>> {
    let mut executions = connection.prepare("SELECT id FROM executions WHERE task_id = ?1")?;
    let ids = executions
        .query_map(params![task_id.as_str()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut bindings = Vec::new();
    for execution_id in ids {
        let mut leases = connection.prepare(
            "SELECT workspace_id, base_revision, worktree_ref, lifecycle
             FROM workspace_leases WHERE execution_id = ?1",
        )?;
        let rows = leases
            .query_map(params![execution_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut rows = rows.into_iter();
        let Some((lease_id, base_revision, worktree_ref, lifecycle)) = rows.next() else {
            continue;
        };
        if rows.next().is_some() {
            return Err(StorageError::Corrupt(format!(
                "execution {execution_id} holds more than one workspace lease"
            )));
        }
        let anchor: Option<String> = connection
            .query_row(
                "SELECT canonical_commondir FROM workspace_trusted_anchor WHERE workspace_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .optional()?;
        bindings.push(LeaseBinding {
            execution_id,
            lease_id,
            base_revision,
            worktree_ref,
            lifecycle,
            anchor,
        });
    }
    bindings.sort_by(|left, right| left.execution_id.cmp(&right.execution_id));
    Ok(bindings)
}

fn live_anchors(
    connection: &Connection,
    task_id: &TaskId,
) -> StorageResult<Vec<TakeoverLeaseAnchor>> {
    Ok(lease_bindings(connection, task_id)?
        .into_iter()
        .map(|binding| TakeoverLeaseAnchor {
            execution_id: binding.execution_id,
            lease_id: binding.lease_id,
            base_revision: binding.base_revision,
            worktree_ref: binding.worktree_ref,
            lifecycle: binding.lifecycle,
            anchor: binding.anchor,
        })
        .collect())
}

fn load_record(connection: &Connection, id: &str) -> StorageResult<Option<TakeoverAdmission>> {
    type Row = (
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        i64,
        Option<i64>,
    );
    let row: Option<Row> = connection
        .query_row(
            "SELECT record_id, hdr_id, task_id, plan_branch_id, basis_plan_revision,
                    expires_at_ms, status,
                    actions_body_kind, actions_body, actions_sensitivity,
                    uncertainty_body_kind, uncertainty_body, uncertainty_sensitivity,
                    question_body_kind, question_body, question_sensitivity,
                    resulting_revision, created_at_ms, resolved_at_ms
             FROM takeover_admissions WHERE record_id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                    row.get(16)?,
                    row.get(17)?,
                    row.get(18)?,
                ))
            },
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(TakeoverAdmission {
        id: TakeoverRecordId::new(row.0).map_err(decode_error)?,
        hdr_id: HumanDecisionRequestId::new(row.1).map_err(decode_error)?,
        task_id: TaskId::new(row.2).map_err(decode_error)?,
        plan_branch_id: PlanBranchId::new(row.3).map_err(decode_error)?,
        basis_plan_revision: nonnegative(row.4, "basis plan revision")?,
        expires_at_ms: nonnegative(row.5, "takeover expiry")?,
        status: TakeoverStatus::parse(&row.6).map_err(decode_error)?,
        actions: decode_authored(row.7, row.8, row.9, "actions")?,
        uncertainty: decode_authored(row.10, row.11, row.12, "uncertainty")?,
        question: decode_authored(row.13, row.14, row.15, "question")?,
        resulting_revision: row
            .16
            .map(|value| nonnegative(value, "resulting revision"))
            .transpose()?,
        created_at_ms: nonnegative(row.17, "takeover created_at")?,
        resolved_at_ms: row
            .18
            .map(|value| nonnegative(value, "takeover resolved_at"))
            .transpose()?,
    }))
}

fn decode_authored(
    kind: Option<String>,
    body: Option<String>,
    sensitivity: Option<String>,
    field: &str,
) -> StorageResult<Option<AuthoredField>> {
    match (kind, body, sensitivity) {
        (None, None, None) => Ok(None),
        (Some(kind), Some(body), Some(sensitivity)) => {
            let body = match kind.as_str() {
                "INLINE" => continuity::model::NoteBody::Inline(body),
                "REF" => continuity::model::NoteBody::Ref(body),
                _ => {
                    return Err(StorageError::Corrupt(format!(
                        "takeover {field} has unknown body kind {kind:?}"
                    )));
                }
            };
            let sensitivity = context_engine::model::Sensitivity::parse(&sensitivity)
                .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?;
            AuthoredField::new(body, sensitivity)
                .map(Some)
                .map_err(|error| StorageError::DomainReconstruction(error.to_string()))
        }
        _ => Err(StorageError::Corrupt(format!(
            "takeover {field} has a partial authored trio"
        ))),
    }
}

fn decode_error(error: impl ToString) -> StorageError {
    StorageError::DomainReconstruction(error.to_string())
}

fn nonnegative(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::Corrupt(format!("takeover {field} is negative")))
}

/// Renders epoch millis as RFC 3339 UTC at second precision. Local
/// mirror of `app_services::format_rfc3339` (same Hinnant civil-date
/// math): the adapter cannot depend on the application layer, and the
/// in-transaction abandonment path mints its own journal evidence.
fn format_rfc3339(millis: u64) -> String {
    let total_secs = millis / 1000;
    let days = (total_secs / 86_400) as i64;
    let time = total_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60,
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn append_field(
    tx: &rusqlite::Transaction<'_>,
    id: &TakeoverRecordId,
    column: &str,
    field: Option<&AuthoredField>,
) -> StorageResult<()> {
    let Some(field) = field else {
        return Ok(());
    };
    let (kind, body) = match &field.body {
        continuity::model::NoteBody::Inline(text) => ("INLINE", text),
        continuity::model::NoteBody::Ref(text) => ("REF", text),
    };
    let sql = format!(
        "UPDATE takeover_admissions SET {column}_body_kind = ?1, {column}_body = ?2, \
         {column}_sensitivity = ?3 WHERE record_id = ?4"
    );
    tx.execute(
        &sql,
        params![kind, body, field.sensitivity.as_str(), id.as_str(),],
    )?;
    Ok(())
}

fn hdr_policy_fingerprint(
    tx: &rusqlite::Transaction<'_>,
    record: &TakeoverAdmission,
) -> StorageResult<String> {
    load_request(tx, record.hdr_id.as_str())?
        .map(|hdr| hdr.precondition.policy_fingerprint)
        .ok_or_else(|| {
            StorageError::Corrupt(format!(
                "takeover {} references missing human decision {}",
                record.id.as_str(),
                record.hdr_id.as_str(),
            ))
        })
}

fn check_unpark_event(task: &task_domain::Task, event: &NewJournalEvent) -> StorageResult<()> {
    if event.event_type.is_project_scoped() {
        return Err(StorageError::ConstraintViolation(
            "takeover unpark cannot carry a project-scoped event".to_owned(),
        ));
    }
    if event.event_type.is_runtime_scoped() {
        return Err(StorageError::ConstraintViolation(
            "takeover unpark cannot carry a runtime event".to_owned(),
        ));
    }
    if event.task_id.as_ref() != Some(task.id())
        || event.project_id.as_ref() != Some(task.project_id())
    {
        return Err(StorageError::ConstraintViolation(
            "takeover unpark event references a different task or project than the state write"
                .to_owned(),
        ));
    }
    if event.from_state != Some(TaskState::Blocked) || event.to_state != Some(TaskState::Running) {
        return Err(StorageError::ConstraintViolation(
            "takeover unpark event states do not match the guarded BLOCKED to RUNNING write"
                .to_owned(),
        ));
    }
    require_envelope(event)
}

fn unpark_write_refusal(
    tx: &rusqlite::Transaction<'_>,
    task_id: &TaskId,
) -> StorageResult<StorageError> {
    let current: Option<String> = tx
        .query_row(
            "SELECT state FROM tasks WHERE id = ?1",
            params![task_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match current {
        None => StorageError::NotFound(format!("task {task_id} does not exist")),
        Some(found) => StorageError::Stale {
            task: task_id.to_string(),
            expected: TaskState::Blocked.as_str().to_owned(),
            found,
        },
    })
}

/// Abandons an expired ACTIVE record inside an open transaction with a
/// store-derived journal event: the record id is the correlation and a
/// deterministic expiry marker is the causation. The caller commits.
fn abandon_in_tx(
    tx: &rusqlite::Transaction<'_>,
    record: &TakeoverAdmission,
    now_ms: u64,
) -> StorageResult<()> {
    let task = crate::task_store::load_task(tx, &record.task_id)?.ok_or_else(|| {
        StorageError::Corrupt(format!(
            "takeover task {} is missing",
            record.task_id.as_str()
        ))
    })?;
    let unparked = tx.execute(
        "UPDATE tasks SET state = 'RUNNING' WHERE id = ?1 AND state = 'BLOCKED'",
        params![record.task_id.as_str()],
    )?;
    if unparked == 0 {
        return Err(unpark_write_refusal(tx, &record.task_id)?);
    }
    insert_journal_event(
        tx,
        &NewJournalEvent {
            event_type: crate::event::EventType::Started,
            task_id: Some(record.task_id.clone()),
            project_id: Some(task.project_id().clone()),
            from_state: Some(TaskState::Blocked),
            to_state: Some(TaskState::Running),
            reason: None,
            correlation_id: record.id.as_str().to_owned(),
            causation_id: format!("takeover-expiry:{}:{now_ms}", record.id.as_str()),
            occurred_at: format_rfc3339(now_ms),
            provenance: "human-decision-service/abandon-takeover".to_owned(),
            execution_id: None,
            attempt_id: None,
            attempt_outcome: None,
            workspace_id: None,
            from_workspace_state: None,
            to_workspace_state: None,
            plan_revision: None,
        },
    )?;
    tx.execute(
        "UPDATE takeover_admissions SET status = 'EXPIRED', resolved_at_ms = ?1
         WHERE record_id = ?2",
        params![to_i64(now_ms, "resolved_at_ms")?, record.id.as_str()],
    )?;
    let policy = hdr_policy_fingerprint(tx, record)?;
    append_human_decision_audit(
        tx,
        HumanDecisionAuditFact {
            event: SecurityAuditEvent::HumanDecisionApplied,
            outcome: SecurityAuditOutcome::Consumed,
            id: &record.hdr_id,
            task_id: &record.task_id,
            policy_fingerprint: &policy,
            reason_code: "TAKEOVER_EXPIRED",
            occurred_at_ms: now_ms,
        },
    )
}

fn load_anchors(connection: &Connection, id: &str) -> StorageResult<Vec<TakeoverLeaseAnchor>> {
    let mut statement = connection.prepare(
        "SELECT execution_id, lease_id, base_revision, worktree_ref, lifecycle, anchor
         FROM takeover_admission_anchors WHERE record_id = ?1 ORDER BY execution_id",
    )?;
    let rows = statement
        .query_map(params![id], |row| {
            Ok(TakeoverLeaseAnchor {
                execution_id: row.get(0)?,
                lease_id: row.get(1)?,
                base_revision: row.get(2)?,
                worktree_ref: row.get(3)?,
                lifecycle: row.get(4)?,
                anchor: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
