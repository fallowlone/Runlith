//! Durable bounded Human Decision Queue persistence (TASK-0079).

use plan_domain::{
    EvidenceRef, HumanContinuation, HumanDecisionPrecondition, HumanDecisionReason,
    HumanDecisionRequest, HumanDecisionRequestId, HumanDecisionStatus, PlanBranchId, PlanChangeSet,
    PlanningCostReceiptRef, PlanningInputSnapshotId, RiskClass, ScopeBound, HUMAN_DECISION_VERSION,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use security_audit::{
    NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditEvent, SecurityAuditOutcome,
    SecurityAuditSubject, SecurityAuditSubjectKind,
};
use task_domain::{TaskId, TaskState};

use crate::audit_store::append_security_audit_in_transaction;
use crate::error::{StorageError, StorageResult};
use crate::event::decode_state;
use crate::plan_cost_store::load_plan_cost_receipt;
use crate::task_store::SqliteStore;

type ApprovedProposalRow = (Option<Vec<u8>>, Option<String>, Option<String>);

#[derive(Clone, Debug)]
pub struct ApprovedPlanProposal {
    pub changeset: PlanChangeSet,
    pub receipt_ref: PlanningCostReceiptRef,
}

#[derive(Clone, Debug)]
pub struct NewHumanDecisionRequest {
    pub task_id: TaskId,
    pub plan_branch_id: PlanBranchId,
    pub reason: HumanDecisionReason,
    pub risk: RiskClass,
    pub affected_scope: ScopeBound,
    pub options: Vec<HumanContinuation>,
    pub recommendation: Option<HumanContinuation>,
    pub evidence: Vec<EvidenceRef>,
    pub proposal_hash: Option<String>,
    pub approved_proposal: Option<ApprovedPlanProposal>,
    pub plan_head: u64,
    pub planning_input_snapshot_id: PlanningInputSnapshotId,
    pub planning_input_fingerprint: String,
    pub policy_fingerprint: String,
    pub expires_at_ms: Option<u64>,
}

impl SqliteStore {
    pub fn create_human_decision_request(
        &mut self,
        mut request: NewHumanDecisionRequest,
        created_at_ms: u64,
    ) -> StorageResult<HumanDecisionRequest> {
        normalize_new(&mut request)?;
        let approved = request.approved_proposal.as_ref().map(|approved| {
            (
                crate::approved_proposal_codec::encode(&approved.changeset),
                approved.receipt_ref.as_str().to_owned(),
            )
        });
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_plan_precondition(
            &tx,
            &request.plan_branch_id,
            request.plan_head,
            &request.planning_input_snapshot_id,
            &request.planning_input_fingerprint,
        )?;
        let task_state = current_task_state(&tx, &request.task_id)?;
        let task_event_seq = latest_task_event_seq_tx(&tx, &request.task_id)?;

        if let Some(approved) = &request.approved_proposal {
            let stored_receipt =
                load_plan_cost_receipt(&tx, &approved.receipt_ref)?.ok_or_else(|| {
                    StorageError::ConstraintViolation(format!(
                        "approved proposal receipt {} is not durable",
                        approved.receipt_ref.as_str()
                    ))
                })?;
            if stored_receipt != approved.changeset.planning_cost_receipt {
                return Err(StorageError::ConstraintViolation(format!(
                    "approved proposal receipt {} does not match proposal evidence",
                    approved.receipt_ref.as_str()
                )));
            }
        }

        // A newer request for the same bounded owner scope fences every
        // unresolved predecessor. Capture exact rows first so each actual
        // authoritative transition has matching Security Audit evidence in
        // this same transaction. Conditional per-row updates also avoid
        // emitting a supersession fact for a row that did not transition.
        let predecessor_ids = {
            let mut statement = tx.prepare(
                "SELECT request_id FROM human_decision_requests
                 WHERE task_id = ?1 AND plan_branch_id = ?2
                   AND status IN ('PENDING', 'RESOLVED')
                 ORDER BY seq",
            )?;
            let rows = statement
                .query_map(
                    params![request.task_id.as_str(), request.plan_branch_id.as_str()],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for predecessor_id in predecessor_ids {
            let updated = tx.execute(
                "UPDATE human_decision_requests SET status = 'SUPERSEDED'
                 WHERE request_id = ?1 AND status IN ('PENDING', 'RESOLVED')",
                params![predecessor_id],
            )?;
            if updated == 0 {
                continue;
            }
            let predecessor = load_request(&tx, &predecessor_id)?.ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "human decision {predecessor_id} vanished during replacement"
                ))
            })?;
            append_human_decision_audit(
                &tx,
                HumanDecisionAuditFact {
                    event: SecurityAuditEvent::HumanDecisionSuperseded,
                    outcome: SecurityAuditOutcome::Superseded,
                    id: &predecessor.id,
                    task_id: &predecessor.task_id,
                    policy_fingerprint: &predecessor.precondition.policy_fingerprint,
                    reason_code: predecessor.reason.as_str(),
                    occurred_at_ms: created_at_ms,
                },
            )?;
        }

        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM human_decision_requests",
            [],
            |row| row.get(0),
        )?;
        let id = HumanDecisionRequestId::new(format!("hdr-{seq}"))
            .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        tx.execute(
            "INSERT INTO human_decision_requests
             (seq, request_id, request_version, task_id, plan_branch_id, reason, risk_class,
              recommendation, proposal_hash, status, selected_continuation, decided_by,
              created_at_ms, expires_at_ms, decided_at_ms, consumed_at_ms, task_state,
              task_event_seq, plan_head, planning_input_snapshot_id,
              planning_input_fingerprint, policy_fingerprint,
              approved_changeset, approved_receipt_ref)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, 'PENDING', NULL, NULL,
                     ?9, ?10, NULL, NULL, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                seq,
                id.as_str(),
                request.task_id.as_str(),
                request.plan_branch_id.as_str(),
                request.reason.as_str(),
                request.risk.as_str(),
                request.recommendation.map(HumanContinuation::as_str),
                request.proposal_hash,
                to_i64(created_at_ms, "created_at_ms")?,
                request
                    .expires_at_ms
                    .map(|v| to_i64(v, "expires_at_ms"))
                    .transpose()?,
                task_state.as_str(),
                to_i64(task_event_seq, "task_event_seq")?,
                to_i64(request.plan_head, "plan_head")?,
                request.planning_input_snapshot_id.as_str(),
                request.planning_input_fingerprint,
                request.policy_fingerprint,
                approved.as_ref().map(|value| value.0.as_slice()),
                approved.as_ref().map(|value| value.1.as_str()),
            ],
        )?;
        insert_values(
            &tx,
            "human_decision_scope_roots",
            "scope_root",
            id.as_str(),
            &request.affected_scope.roots(),
        )?;
        let options = request
            .options
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect::<Vec<_>>();
        insert_values(
            &tx,
            "human_decision_options",
            "continuation",
            id.as_str(),
            &options,
        )?;
        let evidence = request
            .evidence
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect::<Vec<_>>();
        insert_values(
            &tx,
            "human_decision_evidence",
            "evidence_ref",
            id.as_str(),
            &evidence,
        )?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionRequested,
                outcome: SecurityAuditOutcome::Requested,
                id: &id,
                task_id: &request.task_id,
                policy_fingerprint: &request.policy_fingerprint,
                reason_code: request.reason.as_str(),
                occurred_at_ms: created_at_ms,
            },
        )?;
        tx.commit()?;
        self.get_human_decision(&id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished after commit",
                id.as_str()
            ))
        })
    }

    pub fn get_human_decision(
        &self,
        id: &HumanDecisionRequestId,
    ) -> StorageResult<Option<HumanDecisionRequest>> {
        load_request(self.connection(), id.as_str())
    }

    /// Loads the immutable proposal approved by one V9 risk decision. The
    /// adapter codec is verified against the canonical proposal hash and the
    /// durable measured receipt before returning any value to app-services.
    pub fn get_approved_human_decision_proposal(
        &self,
        id: &HumanDecisionRequestId,
    ) -> StorageResult<Option<ApprovedPlanProposal>> {
        let row: Option<ApprovedProposalRow> = self
            .connection()
            .query_row(
                "SELECT approved_changeset, approved_receipt_ref, proposal_hash
                 FROM human_decision_requests WHERE request_id = ?1",
                params![id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((blob, receipt_ref, proposal_hash)) = row else {
            return Ok(None);
        };
        let (blob, receipt_ref) = match (blob, receipt_ref) {
            (None, None) => return Ok(None),
            (Some(blob), Some(receipt_ref)) => (blob, receipt_ref),
            _ => {
                return Err(StorageError::Corrupt(format!(
                    "human decision {} has a partial approved proposal binding",
                    id.as_str()
                )))
            }
        };
        let changeset = crate::approved_proposal_codec::decode(&blob)?;
        if proposal_hash.as_deref() != Some(changeset.proposal_hash.as_str()) {
            return Err(StorageError::Corrupt(format!(
                "human decision {} approved proposal hash binding changed",
                id.as_str()
            )));
        }
        let receipt_ref = PlanningCostReceiptRef::new(receipt_ref)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?;
        let receipt =
            load_plan_cost_receipt(self.connection(), &receipt_ref)?.ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "human decision {} approved proposal receipt is missing",
                    id.as_str()
                ))
            })?;
        if receipt != changeset.planning_cost_receipt {
            return Err(StorageError::Corrupt(format!(
                "human decision {} approved proposal receipt changed",
                id.as_str()
            )));
        }
        Ok(Some(ApprovedPlanProposal {
            changeset,
            receipt_ref,
        }))
    }

    pub fn list_pending_human_decisions(&self) -> StorageResult<Vec<HumanDecisionRequest>> {
        let mut statement = self.connection().prepare(
            "SELECT request_id FROM human_decision_requests
             WHERE status = 'PENDING' ORDER BY seq",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                load_request(self.connection(), &id)?.ok_or_else(|| {
                    StorageError::Corrupt(format!("human decision {id} vanished while listing"))
                })
            })
            .collect()
    }

    pub fn latest_human_decision_for_scope(
        &self,
        task_id: &TaskId,
        branch: &PlanBranchId,
    ) -> StorageResult<Option<HumanDecisionRequest>> {
        let id: Option<String> = self
            .connection()
            .query_row(
                "SELECT request_id FROM human_decision_requests
                 WHERE task_id = ?1 AND plan_branch_id = ?2
                   AND (status IN ('PENDING', 'RESOLVED')
                        OR (status = 'CONSUMED' AND selected_continuation = 'ABORT'))
                 ORDER BY seq DESC LIMIT 1",
                params![task_id.as_str(), branch.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|value| {
            load_request(self.connection(), &value)?.ok_or_else(|| {
                StorageError::Corrupt(format!("human decision {value} vanished while loading"))
            })
        })
        .transpose()
    }

    pub fn resolve_human_decision(
        &mut self,
        id: &HumanDecisionRequestId,
        selected: HumanContinuation,
        decided_by: &str,
        decided_at_ms: u64,
    ) -> StorageResult<HumanDecisionRequest> {
        if decided_by.trim().is_empty() || decided_by.chars().any(char::is_control) {
            return Err(StorageError::ConstraintViolation(
                "human decision decided_by must be non-blank without control characters".to_owned(),
            ));
        }
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_current_tx(&tx, id.as_str(), decided_at_ms)?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM human_decision_requests WHERE request_id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(status) = status else {
            return Err(StorageError::NotFound(format!(
                "human decision {}",
                id.as_str()
            )));
        };
        if status != "PENDING" {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: status,
            });
        }
        let allowed: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM human_decision_options
             WHERE request_id = ?1 AND continuation = ?2)",
            params![id.as_str(), selected.as_str()],
            |row| row.get(0),
        )?;
        if !allowed {
            return Err(StorageError::ConstraintViolation(format!(
                "human decision {} does not allow {}",
                id.as_str(),
                selected.as_str()
            )));
        }
        let updated = tx.execute(
            "UPDATE human_decision_requests
             SET status = 'RESOLVED', selected_continuation = ?1,
                 decided_by = ?2, decided_at_ms = ?3
             WHERE request_id = ?4 AND status = 'PENDING'",
            params![
                selected.as_str(),
                decided_by,
                to_i64(decided_at_ms, "decided_at_ms")?,
                id.as_str(),
            ],
        )?;
        if updated != 1 {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: "concurrent transition".to_owned(),
            });
        }
        let record = load_request(&tx, id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished during resolve",
                id.as_str()
            ))
        })?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionDecided,
                outcome: SecurityAuditOutcome::Resolved,
                id,
                task_id: &record.task_id,
                policy_fingerprint: &record.precondition.policy_fingerprint,
                reason_code: selected.as_str(),
                occurred_at_ms: decided_at_ms,
            },
        )?;
        tx.commit()?;
        self.get_human_decision(id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished after resolve",
                id.as_str()
            ))
        })
    }

    pub fn consume_human_decision(
        &mut self,
        id: &HumanDecisionRequestId,
        selected: HumanContinuation,
        consumed_at_ms: u64,
    ) -> StorageResult<HumanDecisionRequest> {
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_current_tx(&tx, id.as_str(), consumed_at_ms)?;
        let updated = tx.execute(
            "UPDATE human_decision_requests SET status = 'CONSUMED', consumed_at_ms = ?1
             WHERE request_id = ?2 AND status = 'RESOLVED' AND selected_continuation = ?3",
            params![
                to_i64(consumed_at_ms, "consumed_at_ms")?,
                id.as_str(),
                selected.as_str()
            ],
        )?;
        if updated != 1 {
            let state: Option<String> = tx
                .query_row(
                    "SELECT status FROM human_decision_requests WHERE request_id = ?1",
                    params![id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            return Err(match state {
                None => StorageError::NotFound(format!("human decision {}", id.as_str())),
                Some(state) => StorageError::HumanDecisionState {
                    request_id: id.as_str().to_owned(),
                    state,
                },
            });
        }
        let record = load_request(&tx, id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished during consume",
                id.as_str()
            ))
        })?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionApplied,
                outcome: SecurityAuditOutcome::Consumed,
                id,
                task_id: &record.task_id,
                policy_fingerprint: &record.precondition.policy_fingerprint,
                reason_code: selected.as_str(),
                occurred_at_ms: consumed_at_ms,
            },
        )?;
        tx.commit()?;
        self.get_human_decision(id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished after consume",
                id.as_str()
            ))
        })
    }

    /// Marks a continuation consumed after its durable side effect committed.
    /// Used by REPLAN: the accepted PlanRevision may move the very head bound
    /// by the request, so freshness is checked before the planner call and
    /// must not be re-checked after successful publication. A crash after
    /// publication but before this marker leaves the decision stale and thus
    /// non-reusable; it never authorizes a second mutation.
    pub fn mark_human_decision_consumed_after_application(
        &mut self,
        id: &HumanDecisionRequestId,
        selected: HumanContinuation,
        consumed_at_ms: u64,
    ) -> StorageResult<HumanDecisionRequest> {
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE human_decision_requests SET status = 'CONSUMED', consumed_at_ms = ?1
             WHERE request_id = ?2 AND status = 'RESOLVED' AND selected_continuation = ?3",
            params![
                to_i64(consumed_at_ms, "consumed_at_ms")?,
                id.as_str(),
                selected.as_str()
            ],
        )?;
        if updated != 1 {
            let state =
                load_request(&tx, id.as_str())?.map(|record| record.status.as_str().to_owned());
            return Err(match state {
                None => StorageError::NotFound(format!("human decision {}", id.as_str())),
                Some(state) => StorageError::HumanDecisionState {
                    request_id: id.as_str().to_owned(),
                    state,
                },
            });
        }
        let record = load_request(&tx, id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished after consume",
                id.as_str()
            ))
        })?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionApplied,
                outcome: SecurityAuditOutcome::Consumed,
                id,
                task_id: &record.task_id,
                policy_fingerprint: &record.precondition.policy_fingerprint,
                reason_code: selected.as_str(),
                occurred_at_ms: consumed_at_ms,
            },
        )?;
        tx.commit()?;
        Ok(record)
    }

    pub fn validate_human_decision_current(
        &self,
        id: &HumanDecisionRequestId,
        now_ms: u64,
    ) -> StorageResult<HumanDecisionRequest> {
        validate_current_connection(self.connection(), id.as_str(), now_ms)?;
        self.get_human_decision(id)?
            .ok_or_else(|| StorageError::NotFound(format!("human decision {}", id.as_str())))
    }

    /// Fences an unusable request without erasing it.  This is deliberately
    /// narrower than ordinary replacement: callers must first have observed
    /// a stale/failed continuation at the trusted application boundary.
    ///
    /// Keeping the row (and its original selection, if any) provides durable
    /// evidence while allowing a later canonical planning pass to create a
    /// replacement for the same bounded owner scope.
    pub fn supersede_unusable_human_decision(
        &mut self,
        id: &HumanDecisionRequestId,
    ) -> StorageResult<HumanDecisionRequest> {
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM human_decision_requests WHERE request_id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(status) = status else {
            return Err(StorageError::NotFound(format!(
                "human decision {}",
                id.as_str()
            )));
        };
        if status != "PENDING" && status != "RESOLVED" {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: status,
            });
        }
        let updated = tx.execute(
            "UPDATE human_decision_requests SET status = 'SUPERSEDED'
             WHERE request_id = ?1 AND status IN ('PENDING', 'RESOLVED')",
            params![id.as_str()],
        )?;
        if updated != 1 {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: "concurrent transition".to_owned(),
            });
        }
        let record = load_request(&tx, id.as_str())?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished during supersession",
                id.as_str()
            ))
        })?;
        append_human_decision_audit(
            &tx,
            HumanDecisionAuditFact {
                event: SecurityAuditEvent::HumanDecisionSuperseded,
                outcome: SecurityAuditOutcome::Superseded,
                id,
                task_id: &record.task_id,
                policy_fingerprint: &record.precondition.policy_fingerprint,
                reason_code: record.reason.as_str(),
                occurred_at_ms: record.decided_at_ms.unwrap_or(record.created_at_ms),
            },
        )?;
        tx.commit()?;
        self.get_human_decision(id)?.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "human decision {} vanished after supersession",
                id.as_str()
            ))
        })
    }

    pub fn latest_task_event_seq(&self, task_id: &TaskId) -> StorageResult<u64> {
        latest_task_event_seq_connection(self.connection(), task_id)
    }
}

pub(crate) struct RiskDecisionPlanCommitBinding<'a> {
    pub(crate) task_ref: &'a str,
    pub(crate) branch: &'a PlanBranchId,
    pub(crate) plan_head: u64,
    pub(crate) snapshot_id: &'a PlanningInputSnapshotId,
    pub(crate) input_fingerprint: &'a str,
    pub(crate) policy_fingerprint: &'a str,
    pub(crate) proposal_hash: &'a str,
    pub(crate) consumed_at_ms: u64,
}

/// Consumes a V9 RESUME grant in the same transaction as Plan publication.
pub(crate) fn consume_risk_decision_for_plan_commit(
    tx: &Transaction<'_>,
    id: &HumanDecisionRequestId,
    binding: &RiskDecisionPlanCommitBinding<'_>,
) -> StorageResult<()> {
    let record = load_request(tx, id.as_str())?
        .ok_or_else(|| StorageError::NotFound(format!("human decision {}", id.as_str())))?;
    if record.status != HumanDecisionStatus::Resolved
        || record.reason != HumanDecisionReason::RiskEscalation
        || record.selected != Some(HumanContinuation::Resume)
    {
        return Err(StorageError::HumanDecisionState {
            request_id: id.as_str().to_owned(),
            state: record.status.as_str().to_owned(),
        });
    }
    if record
        .expires_at_ms
        .is_some_and(|expires_at| binding.consumed_at_ms > expires_at)
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "decision expired".to_owned(),
        });
    }
    let task_state = current_task_state(tx, &record.task_id)?;
    let task_event_seq = latest_task_event_seq_connection(tx, &record.task_id)?;
    if task_state != record.precondition.task_state
        || task_event_seq != record.precondition.task_event_seq
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "Task state or durable Task evidence changed".to_owned(),
        });
    }
    let durable_head: Option<i64> = tx
        .query_row(
            "SELECT head_revision FROM plan_branches WHERE plan_branch_id = ?1",
            params![binding.branch.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let durable_head = durable_head
        .map(|value| nonnegative_u64(value, "plan head"))
        .transpose()?
        .unwrap_or(0);
    if durable_head != record.precondition.plan_head || durable_head != binding.plan_head {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "Plan head moved".to_owned(),
        });
    }
    if record.task_id.as_str() != binding.task_ref
        || record.plan_branch_id != *binding.branch
        || record.precondition.policy_fingerprint != binding.policy_fingerprint
        || record.proposal_hash.as_deref() != Some(binding.proposal_hash)
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "V9 grant binding no longer matches proposal publication".to_owned(),
        });
    }

    // Retrying the exact V9 proposal after RESUME necessarily publishes a
    // new PlanningInputSnapshot because TASK-0078 includes risk_grant in the
    // snapshot fingerprint. That one expected transition must not be mistaken
    // for arbitrary input drift. Compare both persisted snapshots and allow
    // only addition of the exact decision-derived, unconsumed grant.
    let old_input =
        load_planning_input_binding(tx, record.precondition.planning_input_snapshot_id.as_str())?;
    let new_input = load_planning_input_binding(tx, binding.snapshot_id.as_str())?;
    if old_input.input_fingerprint != record.precondition.planning_input_fingerprint
        || new_input.input_fingerprint != binding.input_fingerprint
        || !same_planning_input_except_risk_grant(&old_input, &new_input)
        || old_input.risk_grant.is_some()
        || new_input.risk_grant
            != Some(PlanningRiskGrantBinding {
                proposal_hash: binding.proposal_hash.to_owned(),
                expires_at_ms: record.expires_at_ms.ok_or_else(|| {
                    StorageError::Corrupt(format!("risk decision {} has no expiry", id.as_str()))
                })?,
                consumed: false,
            })
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "PlanningInputSnapshot changed beyond the exact RESUME grant".to_owned(),
        });
    }
    let current_input: Option<(String, String)> = tx
        .query_row(
            "SELECT snapshot_id, input_fingerprint FROM planning_input_heads
             WHERE plan_branch_id = ?1",
            params![binding.branch.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if current_input
        .as_ref()
        .is_none_or(|(current_id, current_fingerprint)| {
            current_id != binding.snapshot_id.as_str()
                || current_fingerprint != binding.input_fingerprint
        })
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "PlanningInputSnapshot is no longer current".to_owned(),
        });
    }

    let stored_binding: Option<(String, String, String, String)> = tx
        .query_row(
            "SELECT task_id, plan_branch_id, reason, selected_continuation
             FROM human_decision_requests WHERE request_id = ?1",
            params![id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some(stored_binding) = stored_binding else {
        return Err(StorageError::NotFound(format!(
            "human decision {}",
            id.as_str()
        )));
    };
    if stored_binding.0 != binding.task_ref
        || stored_binding.1 != binding.branch.as_str()
        || stored_binding.2 != "RISK_ESCALATION"
        || stored_binding.3 != "RESUME"
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: id.as_str().to_owned(),
            reason: "V9 grant binding no longer matches proposal publication".to_owned(),
        });
    }
    let updated = tx.execute(
        "UPDATE human_decision_requests SET status = 'CONSUMED', consumed_at_ms = ?1
         WHERE request_id = ?2 AND status = 'RESOLVED' AND selected_continuation = 'RESUME'",
        params![
            to_i64(binding.consumed_at_ms, "consumed_at_ms")?,
            id.as_str()
        ],
    )?;
    if updated != 1 {
        return Err(StorageError::HumanDecisionState {
            request_id: id.as_str().to_owned(),
            state: "grant already consumed or no longer resolved".to_owned(),
        });
    }
    append_human_decision_audit(
        tx,
        HumanDecisionAuditFact {
            event: SecurityAuditEvent::HumanDecisionApplied,
            outcome: SecurityAuditOutcome::Consumed,
            id,
            task_id: &record.task_id,
            policy_fingerprint: &record.precondition.policy_fingerprint,
            reason_code: HumanContinuation::Resume.as_str(),
            occurred_at_ms: binding.consumed_at_ms,
        },
    )?;
    Ok(())
}

struct HumanDecisionAuditFact<'a> {
    event: SecurityAuditEvent,
    outcome: SecurityAuditOutcome,
    id: &'a HumanDecisionRequestId,
    task_id: &'a TaskId,
    policy_fingerprint: &'a str,
    reason_code: &'a str,
    occurred_at_ms: u64,
}

fn append_human_decision_audit(
    tx: &Transaction<'_>,
    fact: HumanDecisionAuditFact<'_>,
) -> StorageResult<()> {
    append_security_audit_in_transaction(
        tx,
        &NewSecurityAuditRecord {
            event: fact.event,
            author: SecurityAuditAuthor::HumanDecisionService,
            outcome: Some(fact.outcome),
            occurred_at_ms: fact.occurred_at_ms,
            correlation_id: format!("human-decision:{}", fact.id.as_str()),
            causation_id: None,
            operation_id: None,
            task_id: Some(fact.task_id.as_str().to_owned()),
            execution_id: None,
            attempt_id: None,
            subject: Some(SecurityAuditSubject {
                kind: SecurityAuditSubjectKind::HumanDecision,
                id: fact.id.as_str().to_owned(),
            }),
            policy_fingerprint: Some(fact.policy_fingerprint.to_owned()),
            rule_id: None,
            reason_code: Some(fact.reason_code.to_owned()),
            approval_request_id: None,
            capabilities: Vec::new(),
        },
    )?;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlanningRiskGrantBinding {
    proposal_hash: String,
    expires_at_ms: u64,
    consumed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlanningInputBinding {
    plan_branch_id: String,
    task_ref: String,
    expected_plan_head: u64,
    baseline_risk: String,
    policy_fingerprint: String,
    input_fingerprint: String,
    risk_grant: Option<PlanningRiskGrantBinding>,
    scope_roots: Vec<String>,
    criteria: Vec<(String, String)>,
    evidence: Vec<String>,
    lineage: Vec<String>,
    candidate_task_refs: Vec<String>,
}

fn load_planning_input_binding(
    connection: &Connection,
    snapshot_id: &str,
) -> StorageResult<PlanningInputBinding> {
    type Row = (
        String,
        String,
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let row: Option<Row> = connection
        .query_row(
            "SELECT plan_branch_id, task_ref, expected_plan_head, baseline_risk,
                    policy_fingerprint, input_fingerprint, risk_grant_proposal_hash,
                    risk_grant_expires_at_ms, risk_grant_consumed
             FROM planning_input_snapshots WHERE snapshot_id = ?1",
            params![snapshot_id],
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
                ))
            },
        )
        .optional()?;
    let Some(row) = row else {
        return Err(StorageError::NotFound(format!(
            "planning input snapshot {snapshot_id}"
        )));
    };
    let risk_grant = match (row.6, row.7, row.8) {
        (None, None, None) => None,
        (Some(proposal_hash), Some(expires_at_ms), Some(consumed)) => {
            Some(PlanningRiskGrantBinding {
                proposal_hash,
                expires_at_ms: nonnegative_u64(expires_at_ms, "risk grant expiry")?,
                consumed: match consumed {
                    0 => false,
                    1 => true,
                    other => {
                        return Err(StorageError::Corrupt(format!(
                            "planning input snapshot {snapshot_id} has invalid risk grant consumed flag {other}"
                        )))
                    }
                },
            })
        }
        _ => {
            return Err(StorageError::Corrupt(format!(
                "planning input snapshot {snapshot_id} has partial risk grant binding"
            )))
        }
    };
    Ok(PlanningInputBinding {
        plan_branch_id: row.0,
        task_ref: row.1,
        expected_plan_head: nonnegative_u64(row.2, "expected plan head")?,
        baseline_risk: row.3,
        policy_fingerprint: row.4,
        input_fingerprint: row.5,
        risk_grant,
        scope_roots: load_snapshot_values(
            connection,
            "planning_input_scope_roots",
            "scope_root",
            snapshot_id,
        )?,
        criteria: load_snapshot_pairs(connection, snapshot_id)?,
        evidence: load_snapshot_values(
            connection,
            "planning_input_evidence",
            "evidence_ref",
            snapshot_id,
        )?,
        lineage: load_snapshot_values(
            connection,
            "planning_input_lineage",
            "fingerprint",
            snapshot_id,
        )?,
        candidate_task_refs: load_snapshot_values(
            connection,
            "planning_input_candidate_tasks",
            "task_ref",
            snapshot_id,
        )?,
    })
}

fn same_planning_input_except_risk_grant(
    left: &PlanningInputBinding,
    right: &PlanningInputBinding,
) -> bool {
    left.plan_branch_id == right.plan_branch_id
        && left.task_ref == right.task_ref
        && left.expected_plan_head == right.expected_plan_head
        && left.baseline_risk == right.baseline_risk
        && left.policy_fingerprint == right.policy_fingerprint
        && left.scope_roots == right.scope_roots
        && left.criteria == right.criteria
        && left.evidence == right.evidence
        && left.lineage == right.lineage
        && left.candidate_task_refs == right.candidate_task_refs
}

fn load_snapshot_values(
    connection: &Connection,
    table: &str,
    column: &str,
    snapshot_id: &str,
) -> StorageResult<Vec<String>> {
    let sql = format!("SELECT {column} FROM {table} WHERE snapshot_id = ?1 ORDER BY position");
    let mut statement = connection.prepare(&sql)?;
    let values = statement
        .query_map(params![snapshot_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

fn load_snapshot_pairs(
    connection: &Connection,
    snapshot_id: &str,
) -> StorageResult<Vec<(String, String)>> {
    let mut statement = connection.prepare(
        "SELECT criterion_id, verification_method FROM planning_input_criteria
         WHERE snapshot_id = ?1 ORDER BY position",
    )?;
    let values = statement
        .query_map(params![snapshot_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

fn normalize_new(request: &mut NewHumanDecisionRequest) -> StorageResult<()> {
    request.options.sort();
    request.options.dedup();
    request.evidence.sort();
    request.evidence.dedup();
    if request.options.is_empty() {
        return Err(StorageError::ConstraintViolation(
            "human decision requires at least one continuation option".to_owned(),
        ));
    }
    if request
        .recommendation
        .is_some_and(|value| !request.options.contains(&value))
    {
        return Err(StorageError::ConstraintViolation(
            "human decision recommendation must be one of its options".to_owned(),
        ));
    }
    if request.policy_fingerprint.trim().is_empty()
        || request.policy_fingerprint.chars().any(char::is_control)
    {
        return Err(StorageError::ConstraintViolation(
            "human decision policy fingerprint is invalid".to_owned(),
        ));
    }
    if request.reason == HumanDecisionReason::RiskEscalation
        && !request
            .proposal_hash
            .as_deref()
            .is_some_and(canonical_sha256)
    {
        return Err(StorageError::ConstraintViolation(
            "risk-escalation decision requires canonical proposal_hash".to_owned(),
        ));
    }
    if !canonical_sha256(&request.planning_input_fingerprint) {
        return Err(StorageError::ConstraintViolation(
            "human decision requires canonical planning input fingerprint".to_owned(),
        ));
    }
    Ok(())
}

fn require_plan_precondition(
    connection: &Connection,
    branch: &PlanBranchId,
    expected_head: u64,
    snapshot_id: &PlanningInputSnapshotId,
    fingerprint: &str,
) -> StorageResult<()> {
    let current_head: Option<i64> = connection
        .query_row(
            "SELECT head_revision FROM plan_branches WHERE plan_branch_id = ?1",
            params![branch.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let found = current_head
        .map(|value| nonnegative_u64(value, "plan head"))
        .transpose()?
        .unwrap_or(0);
    if found != expected_head {
        return Err(StorageError::PlanStale {
            branch: branch.as_str().to_owned(),
            expected: expected_head,
            found,
        });
    }
    let current: Option<(String, String)> = connection
        .query_row(
            "SELECT snapshot_id, input_fingerprint FROM planning_input_heads
             WHERE plan_branch_id = ?1",
            params![branch.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if !current.is_some_and(|(id, hash)| id == snapshot_id.as_str() && hash == fingerprint) {
        return Err(StorageError::PlanningInputStale {
            branch: branch.as_str().to_owned(),
            expected_fingerprint: fingerprint.to_owned(),
        });
    }
    Ok(())
}

fn validate_current_connection(
    connection: &Connection,
    id: &str,
    now_ms: u64,
) -> StorageResult<()> {
    let record = load_request(connection, id)?
        .ok_or_else(|| StorageError::NotFound(format!("human decision {id}")))?;
    validate_record_current(connection, &record, now_ms)
}

fn validate_current_tx(tx: &Transaction<'_>, id: &str, now_ms: u64) -> StorageResult<()> {
    let record = load_request(tx, id)?
        .ok_or_else(|| StorageError::NotFound(format!("human decision {id}")))?;
    validate_record_current(tx, &record, now_ms)
}

fn validate_record_current(
    connection: &Connection,
    record: &HumanDecisionRequest,
    now_ms: u64,
) -> StorageResult<()> {
    if record.status == HumanDecisionStatus::Superseded {
        return Err(StorageError::HumanDecisionState {
            request_id: record.id.as_str().to_owned(),
            state: record.status.as_str().to_owned(),
        });
    }
    if record
        .expires_at_ms
        .is_some_and(|expires_at| now_ms > expires_at)
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: record.id.as_str().to_owned(),
            reason: "decision expired".to_owned(),
        });
    }
    let task_state = current_task_state(connection, &record.task_id)?;
    let event_seq = latest_task_event_seq_connection(connection, &record.task_id)?;
    if task_state != record.precondition.task_state
        || event_seq != record.precondition.task_event_seq
    {
        return Err(StorageError::HumanDecisionStale {
            request_id: record.id.as_str().to_owned(),
            reason: "Task state or durable Task evidence changed".to_owned(),
        });
    }
    let current_head: Option<i64> = connection
        .query_row(
            "SELECT head_revision FROM plan_branches WHERE plan_branch_id = ?1",
            params![record.plan_branch_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let found_head = current_head
        .map(|value| nonnegative_u64(value, "plan head"))
        .transpose()?
        .unwrap_or(0);
    if found_head != record.precondition.plan_head {
        return Err(StorageError::HumanDecisionStale {
            request_id: record.id.as_str().to_owned(),
            reason: "Plan head moved".to_owned(),
        });
    }

    let current_input: Option<(String, String)> = connection
        .query_row(
            "SELECT snapshot_id, input_fingerprint FROM planning_input_heads
             WHERE plan_branch_id = ?1",
            params![record.plan_branch_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let exact = current_input
        .as_ref()
        .is_some_and(|(snapshot, fingerprint)| {
            snapshot == record.precondition.planning_input_snapshot_id.as_str()
                && fingerprint == &record.precondition.planning_input_fingerprint
        });
    if !exact {
        let reason = current_input
            .and_then(|(snapshot, _)| current_snapshot_scope_risk(connection, &snapshot).ok())
            .map(|(scope, risk, policy)| {
                if risk > record.risk {
                    "risk increased".to_owned()
                } else if !scope.is_subset_of(&record.affected_scope) {
                    "affected scope widened".to_owned()
                } else if policy != record.precondition.policy_fingerprint {
                    "policy fingerprint changed".to_owned()
                } else {
                    "PlanningInputSnapshot changed".to_owned()
                }
            })
            .unwrap_or_else(|| "PlanningInputSnapshot is no longer current".to_owned());
        return Err(StorageError::HumanDecisionStale {
            request_id: record.id.as_str().to_owned(),
            reason,
        });
    }
    Ok(())
}

fn current_snapshot_scope_risk(
    connection: &Connection,
    snapshot_id: &str,
) -> StorageResult<(ScopeBound, RiskClass, String)> {
    let (risk, policy): (String, String) = connection.query_row(
        "SELECT baseline_risk, policy_fingerprint FROM planning_input_snapshots WHERE snapshot_id = ?1",
        params![snapshot_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut statement = connection.prepare(
        "SELECT scope_root FROM planning_input_scope_roots WHERE snapshot_id = ?1 ORDER BY position",
    )?;
    let roots = statement
        .query_map(params![snapshot_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        ScopeBound::new(roots)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
        RiskClass::parse(&risk)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
        policy,
    ))
}

fn load_request(connection: &Connection, id: &str) -> StorageResult<Option<HumanDecisionRequest>> {
    type Row = (
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
        i64,
        i64,
        String,
        String,
        String,
    );
    let row: Option<Row> = connection
        .query_row(
            "SELECT request_version, task_id, plan_branch_id, reason, risk_class,
                    recommendation, proposal_hash, status, selected_continuation,
                    created_at_ms, expires_at_ms, decided_at_ms, consumed_at_ms,
                    task_state, task_event_seq, plan_head, planning_input_snapshot_id,
                    planning_input_fingerprint, policy_fingerprint
             FROM human_decision_requests WHERE request_id = ?1",
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
    let decided_by: Option<String> = connection.query_row(
        "SELECT decided_by FROM human_decision_requests WHERE request_id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    let options = load_values(connection, "human_decision_options", "continuation", id)?
        .into_iter()
        .map(|value| HumanContinuation::parse(&value).map_err(domain_error))
        .collect::<StorageResult<Vec<_>>>()?;
    let scope = ScopeBound::new(load_values(
        connection,
        "human_decision_scope_roots",
        "scope_root",
        id,
    )?)
    .map_err(domain_error)?;
    let evidence = load_values(connection, "human_decision_evidence", "evidence_ref", id)?
        .into_iter()
        .map(|value| EvidenceRef::new(value).map_err(domain_error))
        .collect::<StorageResult<Vec<_>>>()?;
    let version = u32::try_from(row.0)
        .map_err(|_| StorageError::Corrupt(format!("human decision {id} version overflow")))?;
    if version != HUMAN_DECISION_VERSION {
        return Err(StorageError::Corrupt(format!(
            "human decision {id} has unsupported version {version}"
        )));
    }
    Ok(Some(HumanDecisionRequest {
        id: HumanDecisionRequestId::new(id.to_owned()).map_err(domain_error)?,
        version,
        task_id: TaskId::new(row.1)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
        plan_branch_id: PlanBranchId::new(row.2).map_err(domain_error)?,
        reason: HumanDecisionReason::parse(&row.3).map_err(domain_error)?,
        risk: RiskClass::parse(&row.4).map_err(domain_error)?,
        affected_scope: scope,
        options,
        recommendation: row
            .5
            .map(|value| HumanContinuation::parse(&value).map_err(domain_error))
            .transpose()?,
        evidence,
        proposal_hash: row.6,
        precondition: HumanDecisionPrecondition {
            task_state: decode_state(&row.13)?,
            task_event_seq: nonnegative_u64(row.14, "task_event_seq")?,
            plan_head: nonnegative_u64(row.15, "plan_head")?,
            planning_input_snapshot_id: PlanningInputSnapshotId::new(row.16)
                .map_err(domain_error)?,
            planning_input_fingerprint: row.17,
            policy_fingerprint: row.18,
        },
        created_at_ms: nonnegative_u64(row.9, "created_at_ms")?,
        expires_at_ms: row
            .10
            .map(|v| nonnegative_u64(v, "expires_at_ms"))
            .transpose()?,
        status: HumanDecisionStatus::parse(&row.7).map_err(domain_error)?,
        selected: row
            .8
            .map(|value| HumanContinuation::parse(&value).map_err(domain_error))
            .transpose()?,
        decided_by,
        decided_at_ms: row
            .11
            .map(|v| nonnegative_u64(v, "decided_at_ms"))
            .transpose()?,
        consumed_at_ms: row
            .12
            .map(|v| nonnegative_u64(v, "consumed_at_ms"))
            .transpose()?,
    }))
}

fn current_task_state(connection: &Connection, task_id: &TaskId) -> StorageResult<TaskState> {
    let state: Option<String> = connection
        .query_row(
            "SELECT state FROM tasks WHERE id = ?1",
            params![task_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    state
        .map(|value| decode_state(&value))
        .transpose()?
        .ok_or_else(|| StorageError::NotFound(format!("task {}", task_id.as_str())))
}

fn latest_task_event_seq_connection(
    connection: &Connection,
    task_id: &TaskId,
) -> StorageResult<u64> {
    let value: i64 = connection.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM event_journal WHERE task_id = ?1",
        params![task_id.as_str()],
        |row| row.get(0),
    )?;
    nonnegative_u64(value, "task_event_seq")
}

fn latest_task_event_seq_tx(tx: &Transaction<'_>, task_id: &TaskId) -> StorageResult<u64> {
    latest_task_event_seq_connection(tx, task_id)
}

fn insert_values(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    id: &str,
    values: &[String],
) -> StorageResult<()> {
    let sql = format!("INSERT INTO {table} (request_id, position, {column}) VALUES (?1, ?2, ?3)");
    for (position, value) in values.iter().enumerate() {
        tx.execute(
            &sql,
            params![
                id,
                i64::try_from(position).map_err(|_| StorageError::ConstraintViolation(
                    "human decision list position overflow".to_owned()
                ))?,
                value
            ],
        )?;
    }
    Ok(())
}

fn load_values(
    connection: &Connection,
    table: &str,
    column: &str,
    id: &str,
) -> StorageResult<Vec<String>> {
    let sql = format!("SELECT {column} FROM {table} WHERE request_id = ?1 ORDER BY position");
    let mut statement = connection.prepare(&sql)?;
    let values = statement
        .query_map(params![id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

fn canonical_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn to_i64(value: u64, field: &str) -> StorageResult<i64> {
    i64::try_from(value)
        .map_err(|_| StorageError::ConstraintViolation(format!("{field} exceeds SQLite INTEGER")))
}

fn nonnegative_u64(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::Corrupt(format!("{field} is negative")))
}

fn domain_error(error: plan_domain::PlanDomainError) -> StorageError {
    StorageError::DomainReconstruction(error.to_string())
}
