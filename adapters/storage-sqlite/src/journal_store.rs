//! Event Journal persistence, strict decoding, and scope validation.
//!
//! This module owns the single SQLite Event Journal path used by Task, runtime,
//! workspace, and Plan persistence. Plan events are checked against canonical
//! `plan_revisions` provenance during reconstruction and fail closed on drift.

use agent_runtime::{AttemptId, ExecutionId};
use plan_domain::{
    EvidenceRef, PlanBranchId, PlannerAdapterId, PlannerIdentity, PlanningCostReceiptRef,
    RevisionProposedBy, ValidatedBy,
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use task_domain::{ProjectId, TaskId, TaskState};
use workspace::{LeaseLifecycle, WorkspaceId};

use crate::audit_store::{hash_field, GENESIS_HASH};
use crate::error::{StorageError, StorageResult};
use crate::event::{
    decode_state, AttemptOutcome, EventId, EventType, JournalEvent, NewJournalEvent,
    PlanRevisionJournalPayload, EVENT_SCHEMA_VERSION,
};

const JOURNAL_SELECT: &str = "seq, event_type, event_version, task_id, project_id, \
    from_state, to_state, reason, correlation_id, causation_id, occurred_at, provenance, \
    execution_id, attempt_id, attempt_outcome, workspace_id, plan_branch_id, plan_revision_seq, \
    plan_changeset_hash, plan_planner_adapter_id, plan_model_ref, plan_planning_cost_receipt_ref, \
    plan_validated_service_id, plan_policy_fingerprint";

pub(crate) fn get_event(
    connection: &Connection,
    id: &EventId,
) -> StorageResult<Option<JournalEvent>> {
    let sql = format!("SELECT {JOURNAL_SELECT} FROM event_journal WHERE seq = ?1");
    let row = connection
        .query_row(&sql, params![id.seq()], JournalRow::read)
        .optional()?;
    row.map(|row| decode_journal_row(connection, row))
        .transpose()
}

pub(crate) fn list_events_for_task(
    connection: &Connection,
    task_id: &TaskId,
) -> StorageResult<Vec<JournalEvent>> {
    let sql = format!("SELECT {JOURNAL_SELECT} FROM event_journal WHERE task_id = ?1 ORDER BY seq");
    list_events(connection, &sql, params![task_id.as_str()])
}

pub(crate) fn list_events_for_project(
    connection: &Connection,
    project_id: &ProjectId,
) -> StorageResult<Vec<JournalEvent>> {
    let sql =
        format!("SELECT {JOURNAL_SELECT} FROM event_journal WHERE project_id = ?1 ORDER BY seq");
    list_events(connection, &sql, params![project_id.as_str()])
}

pub(crate) fn list_events_ordered(connection: &Connection) -> StorageResult<Vec<JournalEvent>> {
    let sql = format!("SELECT {JOURNAL_SELECT} FROM event_journal ORDER BY seq");
    list_events(connection, &sql, [])
}

fn list_events(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> StorageResult<Vec<JournalEvent>> {
    // Full-journal scan even for task/project-filtered reads: a filtered
    // subset cannot prove the chain, and a break anywhere is evidence loss.
    verify_journal_chain(connection)?;
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params, JournalRow::read)?;
    let mut events = Vec::new();
    for row in rows {
        events.push(decode_journal_row(connection, row?)?);
    }
    Ok(events)
}

struct JournalRow {
    seq: i64,
    event_type: String,
    event_version: u32,
    task_id: Option<String>,
    project_id: Option<String>,
    from_state: Option<String>,
    to_state: Option<String>,
    reason: Option<String>,
    correlation_id: String,
    causation_id: String,
    occurred_at: String,
    provenance: String,
    execution_id: Option<String>,
    attempt_id: Option<String>,
    attempt_outcome: Option<String>,
    workspace_id: Option<String>,
    plan_branch_id: Option<String>,
    plan_revision_seq: Option<i64>,
    plan_changeset_hash: Option<String>,
    plan_planner_adapter_id: Option<String>,
    plan_model_ref: Option<String>,
    plan_planning_cost_receipt_ref: Option<String>,
    plan_validated_service_id: Option<String>,
    plan_policy_fingerprint: Option<String>,
}

impl JournalRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: row.get(0)?,
            event_type: row.get(1)?,
            event_version: row.get(2)?,
            task_id: row.get(3)?,
            project_id: row.get(4)?,
            from_state: row.get(5)?,
            to_state: row.get(6)?,
            reason: row.get(7)?,
            correlation_id: row.get(8)?,
            causation_id: row.get(9)?,
            occurred_at: row.get(10)?,
            provenance: row.get(11)?,
            execution_id: row.get(12)?,
            attempt_id: row.get(13)?,
            attempt_outcome: row.get(14)?,
            workspace_id: row.get(15)?,
            plan_branch_id: row.get(16)?,
            plan_revision_seq: row.get(17)?,
            plan_changeset_hash: row.get(18)?,
            plan_planner_adapter_id: row.get(19)?,
            plan_model_ref: row.get(20)?,
            plan_planning_cost_receipt_ref: row.get(21)?,
            plan_validated_service_id: row.get(22)?,
            plan_policy_fingerprint: row.get(23)?,
        })
    }
}

/// Typed decode of one journal row. Unknown types/versions are corruption;
/// ids and states go through the same strict validation as task rows.
/// Scope is re-checked on read: a task-scoped row with a NULL task or a
/// project-scoped row with a task reference is corruption, never a default.
fn decode_journal_row(connection: &Connection, row: JournalRow) -> StorageResult<JournalEvent> {
    if row.event_version != EVENT_SCHEMA_VERSION {
        return Err(StorageError::Corrupt(format!(
            "unsupported persisted event version {} for journal seq {}",
            row.event_version, row.seq
        )));
    }
    let event_type = EventType::parse(&row.event_type)?;
    let plan_revision = decode_plan_journal_payload(connection, &row, event_type)?;
    let task_id = row
        .task_id
        .map(|id| {
            TaskId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal task reference: {error}"))
            })
        })
        .transpose()?;
    let project_id = row
        .project_id
        .map(|id| {
            ProjectId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal project reference: {error}"))
            })
        })
        .transpose()?;
    // The shared state columns carry exactly one vocabulary per row:
    // lease lifecycle names on workspace rows, Task names (or NULL) on
    // every other row. Each column is consumed once into its pair.
    let (from_state, from_workspace_state) = match (&event_type, row.from_state) {
        (event_type, Some(raw)) if event_type.is_workspace_scoped() => (
            None,
            Some(LeaseLifecycle::parse(&raw).map_err(|_| {
                StorageError::Corrupt(format!(
                    "journal seq {} stores unknown lease lifecycle {raw:?}",
                    row.seq
                ))
            })?),
        ),
        (_, raw) => (
            raw.map(|state| {
                decode_state(&state).map_err(|_| {
                    StorageError::Corrupt(format!(
                        "journal seq {} stores unknown from_state {state:?}",
                        row.seq
                    ))
                })
            })
            .transpose()?,
            None,
        ),
    };
    let (to_state, to_workspace_state) = match (&event_type, row.to_state) {
        (event_type, Some(raw)) if event_type.is_workspace_scoped() => (
            None,
            Some(LeaseLifecycle::parse(&raw).map_err(|_| {
                StorageError::Corrupt(format!(
                    "journal seq {} stores unknown lease lifecycle {raw:?}",
                    row.seq
                ))
            })?),
        ),
        (_, raw) => (
            raw.map(|state| {
                decode_state(&state).map_err(|_| {
                    StorageError::Corrupt(format!(
                        "journal seq {} stores unknown to_state {state:?}",
                        row.seq
                    ))
                })
            })
            .transpose()?,
            None,
        ),
    };
    let execution_id = row
        .execution_id
        .map(|id| {
            ExecutionId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal execution reference: {error}"))
            })
        })
        .transpose()?;
    let attempt_id = row
        .attempt_id
        .map(|id| {
            AttemptId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal attempt reference: {error}"))
            })
        })
        .transpose()?;
    let attempt_outcome = row
        .attempt_outcome
        .map(|raw| AttemptOutcome::parse(&raw))
        .transpose()?;
    let workspace_id = row
        .workspace_id
        .map(|id| {
            WorkspaceId::new(id).map_err(|error| {
                StorageError::DomainReconstruction(format!("journal workspace reference: {error}"))
            })
        })
        .transpose()?;
    if event_type.is_project_scoped() {
        if project_id.is_none()
            || task_id.is_some()
            || from_state.is_some()
            || to_state.is_some()
            || execution_id.is_some()
            || attempt_id.is_some()
            || attempt_outcome.is_some()
            || workspace_id.is_some()
            || from_workspace_state.is_some()
            || to_workspace_state.is_some()
            || plan_revision.is_some()
        {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a project-scoped event with task, runtime, or workspace scope",
                row.seq
            )));
        }
    } else if event_type.is_runtime_scoped() {
        if project_id.is_none() || plan_revision.is_some() {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a runtime event without project scope or with plan scope",
                row.seq
            )));
        }
        decode_runtime_scope(
            row.seq,
            &event_type,
            &task_id,
            &from_state,
            &to_state,
            &execution_id,
            &attempt_id,
            &attempt_outcome,
        )?;
        if workspace_id.is_some() || from_workspace_state.is_some() || to_workspace_state.is_some()
        {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a runtime event with workspace scope",
                row.seq
            )));
        }
    } else if event_type.is_workspace_scoped() {
        if project_id.is_none() || plan_revision.is_some() {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a workspace event without project scope or with plan scope",
                row.seq
            )));
        }
        decode_workspace_scope(
            row.seq,
            &event_type,
            &task_id,
            &execution_id,
            &workspace_id,
            &attempt_id,
            &attempt_outcome,
            &from_workspace_state,
            &to_workspace_state,
        )?;
        if from_state.is_some() || to_state.is_some() {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a workspace event with Task lifecycle states",
                row.seq
            )));
        }
    } else if event_type.is_plan_scoped() {
        if project_id.is_some()
            || task_id.is_some()
            || from_state.is_some()
            || to_state.is_some()
            || execution_id.is_some()
            || attempt_id.is_some()
            || attempt_outcome.is_some()
            || workspace_id.is_some()
            || from_workspace_state.is_some()
            || to_workspace_state.is_some()
            || plan_revision.is_none()
            || row
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores a plan event with invalid plan scope",
                row.seq
            )));
        }
    } else if project_id.is_none()
        || task_id.is_none()
        || from_state.is_none()
        || to_state.is_none()
        || execution_id.is_some()
        || attempt_id.is_some()
        || attempt_outcome.is_some()
        || workspace_id.is_some()
        || from_workspace_state.is_some()
        || to_workspace_state.is_some()
        || plan_revision.is_some()
    {
        return Err(StorageError::Corrupt(format!(
            "journal seq {} stores a task-scoped event without task scope or with runtime/workspace scope",
            row.seq
        )));
    }
    Ok(JournalEvent {
        id: EventId::parse(&format!("evt-{}", row.seq))?,
        seq: row.seq,
        event_version: row.event_version,
        fact: NewJournalEvent {
            event_type,
            task_id,
            project_id,
            from_state,
            to_state,
            reason: row.reason,
            correlation_id: row.correlation_id,
            causation_id: row.causation_id,
            occurred_at: row.occurred_at,
            provenance: row.provenance,
            execution_id,
            attempt_id,
            attempt_outcome,
            workspace_id,
            from_workspace_state,
            to_workspace_state,
            plan_revision,
        },
    })
}

fn decode_plan_journal_payload(
    connection: &Connection,
    row: &JournalRow,
    event_type: EventType,
) -> StorageResult<Option<PlanRevisionJournalPayload>> {
    let fields_present = [
        row.plan_branch_id.is_some(),
        row.plan_revision_seq.is_some(),
        row.plan_changeset_hash.is_some(),
        row.plan_planner_adapter_id.is_some(),
        row.plan_model_ref.is_some(),
        row.plan_planning_cost_receipt_ref.is_some(),
        row.plan_validated_service_id.is_some(),
        row.plan_policy_fingerprint.is_some(),
    ];
    if !event_type.is_plan_scoped() {
        if fields_present.into_iter().any(|present| present) {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores non-plan event with plan scope",
                row.seq
            )));
        }
        let cause_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM event_plan_causes WHERE event_seq = ?1",
            params![row.seq],
            |record| record.get(0),
        )?;
        if cause_count != 0 {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores non-plan event with plan causes",
                row.seq
            )));
        }
        return Ok(None);
    }
    if fields_present.into_iter().any(|present| !present) {
        return Err(StorageError::Corrupt(format!(
            "journal seq {} stores incomplete plan provenance",
            row.seq
        )));
    }
    let plan_branch_id = row
        .plan_branch_id
        .clone()
        .ok_or_else(|| {
            StorageError::Corrupt(format!(
                "journal seq {} stores plan event without plan branch",
                row.seq
            ))
        })
        .and_then(|branch| {
            PlanBranchId::new(branch).map_err(|error| {
                StorageError::DomainReconstruction(format!(
                    "journal plan branch reference: {error}"
                ))
            })
        })?;
    let raw_revision = row.plan_revision_seq.ok_or_else(|| {
        StorageError::Corrupt(format!(
            "journal seq {} stores plan event without revision sequence",
            row.seq
        ))
    })?;
    let revision_seq = u64::try_from(raw_revision).map_err(|_| {
        StorageError::Corrupt(format!(
            "journal seq {} stores invalid plan revision sequence {raw_revision}",
            row.seq
        ))
    })?;
    if revision_seq == 0 {
        return Err(StorageError::Corrupt(format!(
            "journal seq {} stores zero plan revision sequence",
            row.seq
        )));
    }
    let changeset_hash = row.plan_changeset_hash.clone().ok_or_else(|| {
        StorageError::Corrupt(format!(
            "journal seq {} stores plan event without changeset hash",
            row.seq
        ))
    })?;
    require_canonical_sha256(&changeset_hash).map_err(|error| {
        StorageError::Corrupt(format!(
            "journal seq {} stores invalid plan changeset hash: {error}",
            row.seq
        ))
    })?;
    let proposed_by = RevisionProposedBy::new(
        PlannerIdentity::new(
            PlannerAdapterId::new(row.plan_planner_adapter_id.clone().ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "journal seq {} stores plan event without planner adapter",
                    row.seq
                ))
            })?)
            .map_err(|error| {
                StorageError::DomainReconstruction(format!(
                    "journal planner adapter reference: {error}"
                ))
            })?,
            row.plan_model_ref.clone().ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "journal seq {} stores plan event without planner model",
                    row.seq
                ))
            })?,
        )
        .map_err(|error| {
            StorageError::DomainReconstruction(format!("journal planner identity: {error}"))
        })?,
        PlanningCostReceiptRef::new(row.plan_planning_cost_receipt_ref.clone().ok_or_else(
            || {
                StorageError::Corrupt(format!(
                    "journal seq {} stores plan event without planning cost receipt",
                    row.seq
                ))
            },
        )?)
        .map_err(|error| {
            StorageError::DomainReconstruction(format!(
                "journal planning cost receipt reference: {error}"
            ))
        })?,
    );
    let validated_by = ValidatedBy::new(
        row.plan_validated_service_id.clone().ok_or_else(|| {
            StorageError::Corrupt(format!(
                "journal seq {} stores plan event without validator service",
                row.seq
            ))
        })?,
        row.plan_policy_fingerprint.clone().ok_or_else(|| {
            StorageError::Corrupt(format!(
                "journal seq {} stores plan event without policy fingerprint",
                row.seq
            ))
        })?,
    )
    .map_err(|error| {
        StorageError::DomainReconstruction(format!("journal validation provenance: {error}"))
    })?;
    let mut statement = connection.prepare(
        "SELECT position, evidence_ref FROM event_plan_causes WHERE event_seq = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map(params![row.seq], |record| {
        Ok((record.get::<_, i64>(0)?, record.get::<_, String>(1)?))
    })?;
    let mut caused_by = Vec::new();
    for (expected, record) in (0_i64..).zip(rows) {
        let (position, evidence) = record?;
        if position != expected {
            return Err(StorageError::Corrupt(format!(
                "journal seq {} stores non-contiguous plan cause order",
                row.seq
            )));
        }
        caused_by.push(EvidenceRef::new(evidence).map_err(|error| {
            StorageError::DomainReconstruction(format!("journal plan evidence reference: {error}"))
        })?);
    }
    let payload = PlanRevisionJournalPayload {
        plan_branch_id,
        revision_seq,
        changeset_hash,
        caused_by,
        proposed_by,
        validated_by,
    };
    validate_plan_revision_consistency(connection, row, &payload)?;
    Ok(Some(payload))
}

#[allow(clippy::type_complexity)]
fn validate_plan_revision_consistency(
    connection: &Connection,
    row: &JournalRow,
    payload: &PlanRevisionJournalPayload,
) -> StorageResult<()> {
    let canonical: Option<(
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = connection
        .query_row(
            "SELECT plan_branch_id, revision_seq, changeset_hash, reason,
                    planner_adapter_id, planner_model_ref, planning_cost_receipt_ref,
                    validated_service_id, policy_fingerprint
             FROM plan_revisions
             WHERE plan_branch_id = ?1 AND revision_seq = ?2",
            params![payload.plan_branch_id.as_str(), row.plan_revision_seq],
            |record| {
                Ok((
                    record.get(0)?,
                    record.get(1)?,
                    record.get(2)?,
                    record.get(3)?,
                    record.get(4)?,
                    record.get(5)?,
                    record.get(6)?,
                    record.get(7)?,
                    record.get(8)?,
                ))
            },
        )
        .optional()?;
    let Some((
        canonical_branch,
        canonical_seq,
        canonical_hash,
        canonical_reason,
        canonical_adapter,
        canonical_model,
        canonical_receipt,
        canonical_service,
        canonical_policy,
    )) = canonical
    else {
        return Err(StorageError::Corrupt(format!(
            "journal seq {} references missing canonical plan revision {}:{}",
            row.seq,
            payload.plan_branch_id.as_str(),
            payload.revision_seq
        )));
    };

    require_canonical_sha256(&canonical_hash).map_err(|error| {
        StorageError::Corrupt(format!(
            "canonical plan revision {}:{} stores invalid changeset hash: {error}",
            canonical_branch, canonical_seq
        ))
    })?;
    let canonical_causes =
        load_canonical_plan_causes(connection, payload.plan_branch_id.as_str(), canonical_seq)?;

    let matches = canonical_branch == payload.plan_branch_id.as_str()
        && u64::try_from(canonical_seq).ok() == Some(payload.revision_seq)
        && canonical_reason == row.reason.as_deref().unwrap_or_default()
        && canonical_hash == payload.changeset_hash
        && canonical_causes == payload.caused_by
        && canonical_adapter == payload.proposed_by.planner.adapter_id.as_str()
        && canonical_model == payload.proposed_by.planner.model_ref
        && canonical_receipt == payload.proposed_by.planning_cost_receipt_ref.as_str()
        && canonical_service == payload.validated_by.service_id
        && canonical_policy == payload.validated_by.policy_fingerprint;

    if !matches {
        return Err(StorageError::Corrupt(format!(
            "journal seq {} plan revision provenance does not match canonical revision {}:{}",
            row.seq,
            payload.plan_branch_id.as_str(),
            payload.revision_seq
        )));
    }
    Ok(())
}

fn load_canonical_plan_causes(
    connection: &Connection,
    branch: &str,
    revision_seq: i64,
) -> StorageResult<Vec<EvidenceRef>> {
    let mut statement = connection.prepare(
        "SELECT position, evidence_ref FROM plan_revision_causes
         WHERE plan_branch_id = ?1 AND revision_seq = ?2 ORDER BY position",
    )?;
    let rows = statement.query_map(params![branch, revision_seq], |record| {
        Ok((record.get::<_, i64>(0)?, record.get::<_, String>(1)?))
    })?;
    let mut caused_by = Vec::new();
    for (expected, record) in (0_i64..).zip(rows) {
        let (position, evidence) = record?;
        if position != expected {
            return Err(StorageError::Corrupt(format!(
                "canonical plan revision {branch}:{revision_seq} stores non-contiguous cause order"
            )));
        }
        caused_by.push(EvidenceRef::new(evidence).map_err(|error| {
            StorageError::Corrupt(format!(
                "canonical plan revision {branch}:{revision_seq} stores invalid evidence reference: {error}"
            ))
        })?);
    }
    Ok(caused_by)
}

/// Read-path mirror of the ADR-0022 scope matrix: each runtime family
/// carries exactly its scope. `from_state`/`to_state` stay Task-only,
/// so any Task state on a runtime row is corruption, not data.
#[allow(clippy::too_many_arguments)]
fn decode_runtime_scope(
    seq: i64,
    event_type: &EventType,
    task_id: &Option<TaskId>,
    from_state: &Option<TaskState>,
    to_state: &Option<TaskState>,
    execution_id: &Option<ExecutionId>,
    attempt_id: &Option<AttemptId>,
    attempt_outcome: &Option<AttemptOutcome>,
) -> StorageResult<()> {
    let corrupt =
        |detail: &str| StorageError::Corrupt(format!("journal seq {seq} stores {detail}"));
    if task_id.is_none() || from_state.is_some() || to_state.is_some() {
        return Err(corrupt(
            "a runtime event without task scope or with Task lifecycle states",
        ));
    }
    let valid = execution_id.is_some()
        && match event_type {
            EventType::ExecutionCreated => attempt_id.is_none() && attempt_outcome.is_none(),
            EventType::AttemptCreated => attempt_id.is_some() && attempt_outcome.is_none(),
            EventType::AttemptConcluded => attempt_id.is_some() && attempt_outcome.is_some(),
            _ => false,
        };
    if !valid {
        return Err(corrupt(&format!(
            "runtime event {event_type} with invalid scope for its family"
        )));
    }
    Ok(())
}

/// Read-path mirror of the ADR-0023 §9 scope matrix: each workspace
/// family carries project + task + execution + workspace scope, no attempt
/// scope, no Task lifecycle states, and exactly its contract-fixed lease
/// lifecycle pair.
#[allow(clippy::too_many_arguments)]
fn decode_workspace_scope(
    seq: i64,
    event_type: &EventType,
    task_id: &Option<TaskId>,
    execution_id: &Option<ExecutionId>,
    workspace_id: &Option<WorkspaceId>,
    attempt_id: &Option<AttemptId>,
    attempt_outcome: &Option<AttemptOutcome>,
    from_workspace_state: &Option<LeaseLifecycle>,
    to_workspace_state: &Option<LeaseLifecycle>,
) -> StorageResult<()> {
    use workspace::WorkspaceEventKind;
    let expected = match event_type {
        EventType::WorkspaceCreated => WorkspaceEventKind::Created.transition(),
        EventType::WorkspaceRemoved => WorkspaceEventKind::Removed.transition(),
        _ => {
            return Err(StorageError::Corrupt(format!(
                "journal seq {seq} stores non-workspace event {event_type} in workspace scope decode"
            )));
        }
    };
    let valid = task_id.is_some()
        && execution_id.is_some()
        && workspace_id.is_some()
        && attempt_id.is_none()
        && attempt_outcome.is_none()
        && *from_workspace_state == Some(expected.0)
        && *to_workspace_state == Some(expected.1);
    if !valid {
        return Err(StorageError::Corrupt(format!(
            "journal seq {seq} stores workspace event {event_type} with invalid scope for its family"
        )));
    }
    Ok(())
}

/// Envelope invariants the schema requires (non-blank correlation and
/// causation) are checked before touching SQL so a violation never becomes
/// a half-written transaction.
pub(crate) fn require_envelope(event: &NewJournalEvent) -> StorageResult<()> {
    if event.correlation_id.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires a non-blank correlation_id".to_owned(),
        ));
    }
    if event.causation_id.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires a non-blank causation_id".to_owned(),
        ));
    }
    if event.occurred_at.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires an occurred_at timestamp".to_owned(),
        ));
    }
    if event.provenance.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "journal event requires provenance".to_owned(),
        ));
    }
    require_scope(event)
}

/// Scope coherence between an event's type and its references, mirroring
/// the migration-0003/migration-0004/migration-0005 CHECKs:
/// `project.created` carries no task scope at all (never a fake or
/// sentinel id); G2 task/dependency/lifecycle facts carry full Task scope
/// and no runtime or workspace scope; runtime facts follow the ADR-0022
/// matrix (execution scope, attempt scope for attempt facts, outcome only
/// on conclusion, never Task lifecycle states, never workspace scope);
/// workspace facts follow the ADR-0023 §9 matrix (full Task scope minus
/// Task states, plus execution and workspace scope, never attempt scope,
/// lease lifecycle in the workspace-state fields). Checked before
/// touching SQL.
fn require_scope(event: &NewJournalEvent) -> StorageResult<()> {
    if event.event_type.is_project_scoped() {
        if event.project_id.is_none()
            || event.task_id.is_some()
            || event.from_state.is_some()
            || event.to_state.is_some()
            || event.execution_id.is_some()
            || event.attempt_id.is_some()
            || event.attempt_outcome.is_some()
            || event.workspace_id.is_some()
            || event.from_workspace_state.is_some()
            || event.to_workspace_state.is_some()
            || event.plan_revision.is_some()
        {
            return Err(StorageError::ConstraintViolation(
                "project-scoped journal event must not carry task, runtime, or workspace scope"
                    .to_owned(),
            ));
        }
    } else if event.event_type.is_runtime_scoped() {
        require_runtime_scope(event)?;
    } else if event.event_type.is_workspace_scoped() {
        require_workspace_scope(event)?;
    } else if event.event_type.is_plan_scoped() {
        if event.project_id.is_some()
            || event.task_id.is_some()
            || event.from_state.is_some()
            || event.to_state.is_some()
            || event.execution_id.is_some()
            || event.attempt_id.is_some()
            || event.attempt_outcome.is_some()
            || event.workspace_id.is_some()
            || event.from_workspace_state.is_some()
            || event.to_workspace_state.is_some()
            || event.plan_revision.is_none()
            || event
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return Err(StorageError::ConstraintViolation(
                "plan-scoped journal event requires typed plan revision scope and no task/project/runtime/workspace scope"
                    .to_owned(),
            ));
        }
        require_canonical_sha256(
            &event
                .plan_revision
                .as_ref()
                .ok_or_else(|| {
                    StorageError::ConstraintViolation(
                        "plan-scoped journal event requires typed plan revision scope".to_owned(),
                    )
                })?
                .changeset_hash,
        )?;
    } else if event.project_id.is_none()
        || event.task_id.is_none()
        || event.from_state.is_none()
        || event.to_state.is_none()
        || event.execution_id.is_some()
        || event.attempt_id.is_some()
        || event.attempt_outcome.is_some()
        || event.workspace_id.is_some()
        || event.from_workspace_state.is_some()
        || event.to_workspace_state.is_some()
        || event.plan_revision.is_some()
    {
        return Err(StorageError::ConstraintViolation(
            "task-scoped journal event requires task_id, from_state, and to_state \
             and must not carry runtime or workspace scope"
                .to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn require_canonical_sha256(hash: &str) -> StorageResult<()> {
    let Some(hex) = hash.strip_prefix("sha256:") else {
        return Err(StorageError::ConstraintViolation(
            "changeset hash must use canonical sha256:<hex> format".to_owned(),
        ));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StorageError::ConstraintViolation(
            "changeset hash must contain exactly 64 lowercase hexadecimal digits".to_owned(),
        ));
    }
    Ok(())
}

/// ADR-0022 scope matrix enforcement on the write path: each runtime
/// type carries exactly its family's scope, nothing more.
fn require_runtime_scope(event: &NewJournalEvent) -> StorageResult<()> {
    let valid = event.project_id.is_some()
        && event.task_id.is_some()
        && event.from_state.is_none()
        && event.to_state.is_none()
        && event.workspace_id.is_none()
        && event.from_workspace_state.is_none()
        && event.to_workspace_state.is_none()
        && event.plan_revision.is_none()
        && match event.event_type {
            EventType::ExecutionCreated => {
                event.execution_id.is_some()
                    && event.attempt_id.is_none()
                    && event.attempt_outcome.is_none()
            }
            EventType::AttemptCreated => {
                event.execution_id.is_some()
                    && event.attempt_id.is_some()
                    && event.attempt_outcome.is_none()
            }
            EventType::AttemptConcluded => {
                event.execution_id.is_some()
                    && event.attempt_id.is_some()
                    && event.attempt_outcome.is_some()
            }
            _ => false,
        };
    if !valid {
        return Err(StorageError::ConstraintViolation(format!(
            "runtime event {} carries invalid scope for its family",
            event.event_type
        )));
    }
    Ok(())
}

/// ADR-0023 §9 scope matrix enforcement on the write path: each workspace
/// type carries exactly its family's scope — project + task + execution +
/// workspace, no attempt scope, no Task lifecycle states — and exactly the
/// contract-fixed lease lifecycle pair (callers never choose it).
fn require_workspace_scope(event: &NewJournalEvent) -> StorageResult<()> {
    use workspace::WorkspaceEventKind;
    let expected = match event.event_type {
        EventType::WorkspaceCreated => WorkspaceEventKind::Created.transition(),
        EventType::WorkspaceRemoved => WorkspaceEventKind::Removed.transition(),
        _ => {
            return Err(StorageError::ConstraintViolation(format!(
                "non-workspace event {} reached workspace scope validation",
                event.event_type
            )));
        }
    };
    let valid = event.project_id.is_some()
        && event.task_id.is_some()
        && event.from_state.is_none()
        && event.to_state.is_none()
        && event.execution_id.is_some()
        && event.workspace_id.is_some()
        && event.attempt_id.is_none()
        && event.attempt_outcome.is_none()
        && event.from_workspace_state == Some(expected.0)
        && event.to_workspace_state == Some(expected.1);
    if !valid {
        return Err(StorageError::ConstraintViolation(format!(
            "workspace event {} carries invalid scope for its family",
            event.event_type
        )));
    }
    Ok(())
}

/// Appends one validated journal record inside the caller's transaction
/// and returns it with its assigned identity. The caller owns the atomic
/// boundary (state writes plus this append commit together); this helper
/// never commits on its own.
pub(crate) fn insert_journal_event(
    connection: &Connection,
    event: &NewJournalEvent,
) -> StorageResult<JournalEvent> {
    require_envelope(event)?;
    // Workspace rows carry lease lifecycle names in the shared state
    // columns; every other row carries Task names or NULL. The vocabularies
    // never mix: require_scope above already enforced which pair is set.
    let from_state = event
        .from_workspace_state
        .map(|state| state.as_str().to_owned())
        .or_else(|| event.from_state.map(|state| state.as_str().to_owned()));
    let to_state = event
        .to_workspace_state
        .map(|state| state.as_str().to_owned())
        .or_else(|| event.to_state.map(|state| state.as_str().to_owned()));
    let plan_revision_seq = event
        .plan_revision
        .as_ref()
        .map(|payload| {
            i64::try_from(payload.revision_seq).map_err(|_| {
                StorageError::ConstraintViolation(
                    "plan revision sequence exceeds SQLite INTEGER range".to_owned(),
                )
            })
        })
        .transpose()?;
    let (previous_hash, seq) = chain_head(connection)?;
    let plan = event.plan_revision.as_ref();
    let row = JournalRow {
        seq,
        event_type: event.event_type.as_str().to_owned(),
        event_version: EVENT_SCHEMA_VERSION,
        task_id: event.task_id.as_ref().map(|id| id.as_str().to_owned()),
        project_id: event.project_id.as_ref().map(|id| id.as_str().to_owned()),
        from_state,
        to_state,
        reason: event.reason.clone(),
        correlation_id: event.correlation_id.clone(),
        causation_id: event.causation_id.clone(),
        occurred_at: event.occurred_at.clone(),
        provenance: event.provenance.clone(),
        execution_id: event.execution_id.as_ref().map(|id| id.as_str().to_owned()),
        attempt_id: event.attempt_id.as_ref().map(|id| id.as_str().to_owned()),
        attempt_outcome: event
            .attempt_outcome
            .map(|outcome| outcome.as_str().to_owned()),
        workspace_id: event.workspace_id.as_ref().map(|id| id.as_str().to_owned()),
        plan_branch_id: plan.map(|payload| payload.plan_branch_id.as_str().to_owned()),
        plan_revision_seq,
        plan_changeset_hash: plan.map(|payload| payload.changeset_hash.as_str().to_owned()),
        plan_planner_adapter_id: plan
            .map(|payload| payload.proposed_by.planner.adapter_id.as_str().to_owned()),
        plan_model_ref: plan
            .map(|payload| payload.proposed_by.planner.model_ref.as_str().to_owned()),
        plan_planning_cost_receipt_ref: plan.map(|payload| {
            payload
                .proposed_by
                .planning_cost_receipt_ref
                .as_str()
                .to_owned()
        }),
        plan_validated_service_id: plan
            .map(|payload| payload.validated_by.service_id.as_str().to_owned()),
        plan_policy_fingerprint: plan
            .map(|payload| payload.validated_by.policy_fingerprint.as_str().to_owned()),
    };
    let record_hash = hash_journal_row(&row, &previous_hash);
    connection.execute(
        "INSERT INTO event_journal
         (seq, event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id,
          plan_branch_id, plan_revision_seq, plan_changeset_hash,
          plan_planner_adapter_id, plan_model_ref, plan_planning_cost_receipt_ref,
          plan_validated_service_id, plan_policy_fingerprint,
          previous_hash, record_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
        params![
            row.seq,
            row.event_type,
            row.event_version,
            row.task_id,
            row.project_id,
            row.from_state,
            row.to_state,
            row.reason,
            row.correlation_id,
            row.causation_id,
            row.occurred_at,
            row.provenance,
            row.execution_id,
            row.attempt_id,
            row.attempt_outcome,
            row.workspace_id,
            row.plan_branch_id,
            row.plan_revision_seq,
            row.plan_changeset_hash,
            row.plan_planner_adapter_id,
            row.plan_model_ref,
            row.plan_planning_cost_receipt_ref,
            row.plan_validated_service_id,
            row.plan_policy_fingerprint,
            previous_hash,
            record_hash,
        ],
    )?;
    if let Some(payload) = &event.plan_revision {
        for (position, evidence) in payload.caused_by.iter().enumerate() {
            connection.execute(
                "INSERT INTO event_plan_causes (event_seq, position, evidence_ref) VALUES (?1, ?2, ?3)",
                params![seq, position as i64, evidence.as_str()],
            )?;
        }
    }
    Ok(JournalEvent {
        id: EventId::parse(&format!("evt-{seq}"))?,
        seq,
        event_version: EVENT_SCHEMA_VERSION,
        fact: event.clone(),
    })
}

/// Previous record hash (genesis when empty) and the next `seq`. Callers hold
/// an IMMEDIATE transaction, so the head read and the INSERT cannot fork the
/// chain. `seq` is chosen explicitly because it is part of the hash.
fn chain_head(connection: &Connection) -> StorageResult<(String, i64)> {
    let head: Option<(i64, Option<String>)> = connection
        .query_row(
            "SELECT seq, record_hash FROM event_journal ORDER BY seq DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let issued: Option<i64> = connection
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'event_journal'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let (head_seq, previous_hash) = match head {
        None => (0, GENESIS_HASH.to_owned()),
        Some((seq, Some(hash))) => (seq, hash),
        Some((seq, None)) => {
            return Err(StorageError::Corrupt(format!(
                "event journal row {seq} has no record hash"
            )))
        }
    };
    Ok((previous_hash, head_seq.max(issued.unwrap_or(0)) + 1))
}

/// Fails closed with `StorageError::Corrupt` on any broken, missing, or
/// reordered link. Empty journal is genesis, not a break.
pub(crate) fn verify_journal_chain(connection: &Connection) -> StorageResult<()> {
    let sql = format!(
        "SELECT {JOURNAL_SELECT}, previous_hash, record_hash FROM event_journal ORDER BY seq"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        Ok((JournalRow::read(row)?, row.get(24)?, row.get(25)?))
    })?;
    let mut expected_previous = GENESIS_HASH.to_owned();
    for row in rows {
        let (row, previous, record): (JournalRow, Option<String>, Option<String>) = row?;
        let (Some(previous), Some(record)) = (previous, record) else {
            return Err(StorageError::Corrupt(format!(
                "event journal row {} has no hash chain link",
                row.seq
            )));
        };
        if previous != expected_previous {
            return Err(StorageError::Corrupt(format!(
                "event journal chain break before seq {}",
                row.seq
            )));
        }
        if record != hash_journal_row(&row, &previous) {
            return Err(StorageError::Corrupt(format!(
                "event journal hash mismatch at seq {}",
                row.seq
            )));
        }
        expected_previous = record;
    }
    Ok(())
}

/// Migration 0024: chains every pre-chain row in `seq` order.
pub(crate) fn backfill_journal_chain(connection: &Connection) -> StorageResult<()> {
    let sql = format!("SELECT {JOURNAL_SELECT} FROM event_journal ORDER BY seq");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map([], JournalRow::read)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut previous = GENESIS_HASH.to_owned();
    for row in rows {
        let record = hash_journal_row(&row, &previous);
        connection.execute(
            "UPDATE event_journal SET previous_hash = ?1, record_hash = ?2 WHERE seq = ?3",
            params![previous, record, row.seq],
        )?;
        previous = record;
    }
    Ok(())
}

fn hash_journal_row(row: &JournalRow, previous_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "contract", Some("event-journal-v1"));
    hash_field(&mut hasher, "seq", Some(&row.seq.to_string()));
    hash_field(&mut hasher, "previous_hash", Some(previous_hash));
    hash_field(&mut hasher, "event_type", Some(&row.event_type));
    hash_field(
        &mut hasher,
        "event_version",
        Some(&row.event_version.to_string()),
    );
    hash_field(&mut hasher, "task_id", row.task_id.as_deref());
    hash_field(&mut hasher, "project_id", row.project_id.as_deref());
    hash_field(&mut hasher, "from_state", row.from_state.as_deref());
    hash_field(&mut hasher, "to_state", row.to_state.as_deref());
    hash_field(&mut hasher, "reason", row.reason.as_deref());
    hash_field(&mut hasher, "correlation_id", Some(&row.correlation_id));
    hash_field(&mut hasher, "causation_id", Some(&row.causation_id));
    hash_field(&mut hasher, "occurred_at", Some(&row.occurred_at));
    hash_field(&mut hasher, "provenance", Some(&row.provenance));
    hash_field(&mut hasher, "execution_id", row.execution_id.as_deref());
    hash_field(&mut hasher, "attempt_id", row.attempt_id.as_deref());
    hash_field(
        &mut hasher,
        "attempt_outcome",
        row.attempt_outcome.as_deref(),
    );
    hash_field(&mut hasher, "workspace_id", row.workspace_id.as_deref());
    hash_field(&mut hasher, "plan_branch_id", row.plan_branch_id.as_deref());
    hash_field(
        &mut hasher,
        "plan_revision_seq",
        row.plan_revision_seq.map(|seq| seq.to_string()).as_deref(),
    );
    hash_field(
        &mut hasher,
        "plan_changeset_hash",
        row.plan_changeset_hash.as_deref(),
    );
    hash_field(
        &mut hasher,
        "plan_planner_adapter_id",
        row.plan_planner_adapter_id.as_deref(),
    );
    hash_field(&mut hasher, "plan_model_ref", row.plan_model_ref.as_deref());
    hash_field(
        &mut hasher,
        "plan_planning_cost_receipt_ref",
        row.plan_planning_cost_receipt_ref.as_deref(),
    );
    hash_field(
        &mut hasher,
        "plan_validated_service_id",
        row.plan_validated_service_id.as_deref(),
    );
    hash_field(
        &mut hasher,
        "plan_policy_fingerprint",
        row.plan_policy_fingerprint.as_deref(),
    );
    format!("sha256:{:x}", hasher.finalize())
}
