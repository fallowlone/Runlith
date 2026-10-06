//! Durable ADR-0031 side-effect intents (TASK-0126).
//!
//! Intent rows carry the §3 bindings; every transition lands with its
//! audit row in one transaction. Reads rebuild through the domain
//! machine by replaying the audit trail, so stored state can never
//! disagree with its own history.

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use workspace::{
    SideEffectIntent, SideEffectIntentFields, SideEffectKind, SideEffectState, TransitionEvent,
};

use crate::review_authority_store::{decode_list, encode_list};
use crate::{StorageError, StorageResult};

fn corrupt(what: &str) -> StorageError {
    StorageError::Corrupt(format!("side-effect store: {what}"))
}

fn domain(error: impl std::fmt::Display) -> StorageError {
    StorageError::DomainReconstruction(format!("side-effect store: {error}"))
}

/// One durable escalation with its optional resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideEffectEscalation {
    pub intent_id: String,
    pub code: String,
    pub evidence_refs: Vec<String>,
    pub escalated_at_ms: i64,
    pub resolved_at_ms: Option<i64>,
    pub resolution: Option<String>,
    pub successor_intent_id: Option<String>,
}

/// One durable transition audit row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideEffectTransition {
    pub transition_id: i64,
    pub intent_id: String,
    pub from_state: SideEffectState,
    pub to_state: SideEffectState,
    pub evidence_refs: Vec<String>,
    pub transitioned_at_ms: i64,
}

type IntentRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
    i64,
);

type EscalationRow = (
    String,
    String,
    String,
    i64,
    Option<i64>,
    Option<String>,
    Option<String>,
);

const INTENT_COLUMNS: &str = "intent_id, kind, target_ref, params_hash, idempotency_key,
    state, task_id, execution_id, lease_id, pre_state_fingerprint,
    outcome_evidence_refs, policy_fingerprint, created_at_ms, updated_at_ms";

fn event_for_edge(from: SideEffectState, to: SideEffectState) -> StorageResult<TransitionEvent> {
    use SideEffectState as S;
    use TransitionEvent as E;
    match (from, to) {
        (S::Prepared, S::IntentRecorded) => Ok(E::RecordIntent),
        (S::IntentRecorded, S::Executing) => Ok(E::Dispatch),
        (S::Executing, S::Confirmed) => Ok(E::Confirm),
        (S::Executing, S::Failed) => Ok(E::Fail),
        (S::Executing, S::Ambiguous) => Ok(E::MarkAmbiguous),
        (S::Ambiguous, S::Reconcile) => Ok(E::BeginReconcile),
        (S::Reconcile, S::Confirmed) => Ok(E::ReconciledToConfirmed),
        (S::Reconcile, S::SafeToRetry) => Ok(E::ReconciledToRetry),
        (S::Reconcile, S::SafeToContinue) => Ok(E::ReconciledToContinue),
        (S::Reconcile, S::Escalate) => Ok(E::Escalate),
        (S::SafeToRetry, S::Executing) => Ok(E::Redispatch),
        (S::SafeToContinue, S::Executing) => Ok(E::DispatchContinuation),
        _ => Err(corrupt("transition audit disagrees with the §2 table")),
    }
}

fn intent_from_row(
    connection: &rusqlite::Connection,
    row: IntentRow,
) -> StorageResult<SideEffectIntent> {
    let (
        intent_id,
        kind,
        target_ref,
        params_hash,
        idempotency_key,
        state,
        task_id,
        execution_id,
        lease_id,
        pre_state_fingerprint,
        outcome_evidence_refs,
        policy_fingerprint,
        _created_at_ms,
        _updated_at_ms,
    ) = row;
    let mut intent = SideEffectIntent::new(SideEffectIntentFields {
        intent_id: intent_id.clone(),
        kind: SideEffectKind::parse(&kind).map_err(domain)?,
        target_ref,
        params_hash,
        idempotency_key,
        task_id,
        execution_id,
        lease_id,
        pre_state_fingerprint,
        policy_fingerprint,
    })
    .map_err(domain)?;
    let mut statement = connection.prepare(
        "SELECT from_state, to_state, evidence_refs FROM side_effect_transitions
         WHERE intent_id = ?1 ORDER BY transition_id",
    )?;
    let trail = statement.query_map(params![intent_id], |row| {
        let from: String = row.get(0)?;
        let to: String = row.get(1)?;
        let evidence: String = row.get(2)?;
        Ok((from, to, evidence))
    })?;
    for entry in trail {
        let (from, to, evidence) = entry?;
        let from = SideEffectState::parse(&from).map_err(|_| corrupt("transition state"))?;
        let to = SideEffectState::parse(&to).map_err(|_| corrupt("transition state"))?;
        if intent.state() != from {
            return Err(corrupt("transition audit is discontinuous"));
        }
        intent
            .apply(event_for_edge(from, to)?, &decode_list(&evidence)?)
            .map_err(domain)?;
    }
    let stored_state = SideEffectState::parse(&state).map_err(|_| corrupt("intent state"))?;
    if intent.state() != stored_state {
        return Err(corrupt("intent state disagrees with its audit trail"));
    }
    if encode_list(intent.outcome_evidence_refs()) != outcome_evidence_refs {
        return Err(corrupt("intent evidence disagrees with its audit trail"));
    }
    Ok(intent)
}

impl crate::SqliteStore {
    /// Records one intent, moving `Prepared → INTENT_RECORDED` with
    /// its audit row in one transaction. The slot trigger refuses a
    /// second active intent for the same (lease, kind).
    pub fn record_side_effect_intent(
        &mut self,
        intent: &SideEffectIntent,
        at_ms: i64,
    ) -> StorageResult<SideEffectIntent> {
        if intent.state() != SideEffectState::Prepared {
            return Err(StorageError::ConstraintViolation(format!(
                "side-effect intent {} is not Prepared",
                intent.intent_id()
            )));
        }
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction
            .execute(
                "INSERT INTO side_effect_intents
                 (intent_id, kind, target_ref, params_hash, idempotency_key, state,
                  task_id, execution_id, lease_id, pre_state_fingerprint,
                  outcome_evidence_refs, policy_fingerprint, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'INTENT_RECORDED', ?6, ?7, ?8, ?9, '', ?10, ?11, ?11)",
                params![
                    intent.intent_id(),
                    intent.kind().serialize(),
                    intent.target_ref(),
                    intent.params_hash(),
                    intent.idempotency_key(),
                    intent.task_id(),
                    intent.execution_id(),
                    intent.lease_id(),
                    intent.pre_state_fingerprint(),
                    intent.policy_fingerprint(),
                    at_ms,
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "record side-effect intent"))?;
        transaction.execute(
            "INSERT INTO side_effect_transitions
             (intent_id, from_state, to_state, evidence_refs, transitioned_at_ms)
             VALUES (?1, 'PREPARED', 'INTENT_RECORDED', '', ?2)",
            params![intent.intent_id(), at_ms],
        )?;
        transaction.commit()?;
        let mut recorded = intent.clone();
        recorded
            .apply(TransitionEvent::RecordIntent, &[])
            .map_err(domain)?;
        Ok(recorded)
    }

    /// Applies one transition with its audit row in one transaction.
    /// The domain validates first (no write on refusal); the trigger
    /// double-enforces the edge table for raw writers.
    pub fn transition_side_effect_intent(
        &mut self,
        intent_id: &str,
        event: TransitionEvent,
        evidence: &[String],
        at_ms: i64,
    ) -> StorageResult<SideEffectIntent> {
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: IntentRow = transaction
            .query_row(
                format!("SELECT {INTENT_COLUMNS} FROM side_effect_intents WHERE intent_id = ?1")
                    .as_str(),
                params![intent_id],
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
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                StorageError::NotFound(format!("side-effect intent not found: {intent_id}"))
            })?;
        let mut intent = intent_from_row(&transaction, row)?;
        let from = intent.state();
        intent.apply(event, evidence).map_err(domain)?;
        transaction.execute(
            "UPDATE side_effect_intents
             SET state = ?1, outcome_evidence_refs = ?2, updated_at_ms = ?3
             WHERE intent_id = ?4",
            params![
                intent.state().serialize(),
                encode_list(intent.outcome_evidence_refs()),
                at_ms,
                intent_id,
            ],
        )?;
        transaction.execute(
            "INSERT INTO side_effect_transitions
             (intent_id, from_state, to_state, evidence_refs, transitioned_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                intent_id,
                from.serialize(),
                intent.state().serialize(),
                encode_list(evidence),
                at_ms,
            ],
        )?;
        transaction.commit()?;
        Ok(intent)
    }

    /// Reads one intent by id, rebuilding through the domain machine
    /// by replaying its audit trail.
    pub fn get_side_effect_intent(
        &self,
        intent_id: &str,
    ) -> StorageResult<Option<SideEffectIntent>> {
        let row: Option<IntentRow> = self
            .connection()
            .query_row(
                format!("SELECT {INTENT_COLUMNS} FROM side_effect_intents WHERE intent_id = ?1")
                    .as_str(),
                params![intent_id],
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
                    ))
                },
            )
            .optional()?;
        row.map(|row| intent_from_row(self.connection(), row))
            .transpose()
    }

    /// Reads the newest intent for one (lease, kind). The active slot
    /// guarantees at most one non-terminal, non-escalated row, so the
    /// newest row is the live one in every reachable state.
    pub fn get_side_effect_intent_for_lease(
        &self,
        lease_id: &str,
        kind: SideEffectKind,
    ) -> StorageResult<Option<SideEffectIntent>> {
        let row: Option<IntentRow> = self
            .connection()
            .query_row(
                format!(
                    "SELECT {INTENT_COLUMNS} FROM side_effect_intents
                     WHERE lease_id = ?1 AND kind = ?2 ORDER BY rowid DESC LIMIT 1"
                )
                .as_str(),
                params![lease_id, kind.serialize()],
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
                    ))
                },
            )
            .optional()?;
        row.map(|row| intent_from_row(self.connection(), row))
            .transpose()
    }

    /// Lists one intent's transition audit in order.
    pub fn list_side_effect_transitions(
        &self,
        intent_id: &str,
    ) -> StorageResult<Vec<SideEffectTransition>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT transition_id, intent_id, from_state, to_state, evidence_refs,
                    transitioned_at_ms
             FROM side_effect_transitions WHERE intent_id = ?1 ORDER BY transition_id",
        )?;
        let rows = statement.query_map(params![intent_id], |row| {
            let from: String = row.get(2)?;
            let to: String = row.get(3)?;
            let evidence: String = row.get(4)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                SideEffectState::parse(&from),
                SideEffectState::parse(&to),
                evidence,
                row.get(5)?,
            ))
        })?;
        let mut transitions = Vec::new();
        for row in rows {
            let (transition_id, intent_id, from, to, evidence, transitioned_at_ms) = row?;
            transitions.push(SideEffectTransition {
                transition_id,
                intent_id,
                from_state: from.map_err(|_| corrupt("transition state"))?,
                to_state: to.map_err(|_| corrupt("transition state"))?,
                evidence_refs: decode_list(&evidence)?,
                transitioned_at_ms,
            });
        }
        Ok(transitions)
    }

    /// Reads one escalation by intent id, if recorded.
    pub fn get_side_effect_escalation(
        &self,
        intent_id: &str,
    ) -> StorageResult<Option<SideEffectEscalation>> {
        let row: Option<EscalationRow> = self
            .connection()
            .query_row(
                "SELECT intent_id, code, evidence_refs, escalated_at_ms,
                        resolved_at_ms, resolution, successor_intent_id
                 FROM side_effect_escalations WHERE intent_id = ?1",
                params![intent_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(
                intent_id,
                code,
                evidence,
                escalated_at_ms,
                resolved_at_ms,
                resolution,
                successor_intent_id,
            )| {
                Ok(SideEffectEscalation {
                    intent_id,
                    code,
                    evidence_refs: decode_list(&evidence)?,
                    escalated_at_ms,
                    resolved_at_ms,
                    resolution,
                    successor_intent_id,
                })
            },
        )
        .transpose()
    }

    /// Records one escalation for an ESCALATE intent (the trigger
    /// refuses any other state).
    pub fn record_side_effect_escalation(
        &mut self,
        intent_id: &str,
        code: &str,
        evidence: &[String],
        at_ms: i64,
    ) -> StorageResult<()> {
        self.connection_mut()
            .execute(
                "INSERT INTO side_effect_escalations
                 (intent_id, code, evidence_refs, escalated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![intent_id, code, encode_list(evidence), at_ms],
            )
            .map_err(|error| {
                crate::error::map_constraint(error, "record side-effect escalation")
            })?;
        Ok(())
    }

    /// Resolves one escalation exactly once (sticky): `discarded`
    /// closes the intent, `successor` links the authorized fresh
    /// intent carrying the key forward.
    pub fn resolve_side_effect_escalation(
        &mut self,
        intent_id: &str,
        resolution: &str,
        successor_intent_id: Option<&str>,
        at_ms: i64,
    ) -> StorageResult<()> {
        let valid = (resolution == "discarded" && successor_intent_id.is_none())
            || (resolution == "successor" && successor_intent_id.is_some());
        if !valid {
            return Err(StorageError::ConstraintViolation(
                "escalation resolution must be discarded (no successor) or successor (linked)"
                    .to_owned(),
            ));
        }
        let changed = self
            .connection_mut()
            .execute(
                "UPDATE side_effect_escalations
                 SET resolved_at_ms = ?1, resolution = ?2, successor_intent_id = ?3
                 WHERE intent_id = ?4",
                params![at_ms, resolution, successor_intent_id, intent_id],
            )
            .map_err(|error| {
                crate::error::map_constraint(error, "resolve side-effect escalation")
            })?;
        if changed == 0 {
            return Err(StorageError::NotFound(format!(
                "side-effect escalation not found: {intent_id}"
            )));
        }
        Ok(())
    }
}
