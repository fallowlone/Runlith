//! Durable G7 planning guardrail counters (ADR-0027 §5 L3, §6 envelope).
//!
//! One row per `(plan branch, triggering-evidence key)`. The trusted planning
//! service reserves each planner call here *before* invoking the provider
//! (call count plus a conservative input-token reservation) and settles it
//! afterwards with the measured or conservative charge, atomically with any
//! measured receipt. A crash between reservation and settlement therefore
//! leaves the conservative charge, never invisible spend. Guardrail policy
//! (bounds, what to charge) stays in the application service.

use plan_domain::{PlanBranchId, PlanningCostReceipt, PlanningCostReceiptRef};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::error::{StorageError, StorageResult};
use crate::plan_cost_store::insert_plan_cost_receipt;
use crate::task_store::SqliteStore;

/// Durable guardrail counters for one branch + triggering-evidence event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanGuardrailEvent {
    /// Planner calls reserved for this event (provider-started or not).
    pub planner_calls: u32,
    /// Input tokens charged: measured usage, or conservative charges for
    /// calls that reported none, plus any in-flight reservation.
    pub charged_input_tokens: u64,
    /// Consecutive refused proposals since the last accepted revision.
    pub consecutive_rejections: u32,
}

/// Settlement of one previously reserved planner call.
#[derive(Clone, Copy, Debug)]
pub struct PlannerCallSettlement<'a> {
    /// Tokens reserved by [`SqliteStore::reserve_plan_planner_call`].
    pub reserved_input_tokens: u32,
    /// Tokens the call is finally charged.
    pub charged_input_tokens: u64,
    /// Exact provider-reported receipt, recorded in the same transaction.
    pub measured: Option<(&'a PlanningCostReceiptRef, &'a PlanningCostReceipt)>,
}

impl SqliteStore {
    /// Reads one event's counters. Corrupt stored values fail closed.
    pub fn get_plan_guardrail_event(
        &self,
        branch: &PlanBranchId,
        trigger_evidence_key: &str,
    ) -> StorageResult<Option<PlanGuardrailEvent>> {
        require_evidence_key(trigger_evidence_key)?;
        load_event(self.connection(), branch, trigger_evidence_key)
    }

    /// Reserves one planner call for the event if fewer than `max_calls`
    /// calls were reserved so far. Returns the updated counters, or `None`
    /// when the call bound is already reached (nothing is written).
    pub fn reserve_plan_planner_call(
        &mut self,
        branch: &PlanBranchId,
        trigger_evidence_key: &str,
        max_calls: u32,
        reserved_input_tokens: u32,
    ) -> StorageResult<Option<PlanGuardrailEvent>> {
        require_evidence_key(trigger_evidence_key)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = ensure_event(&transaction, branch, trigger_evidence_key)?;
        if current.planner_calls >= max_calls {
            transaction.rollback()?;
            return Ok(None);
        }
        let next = PlanGuardrailEvent {
            planner_calls: current
                .planner_calls
                .checked_add(1)
                .ok_or_else(|| overflow("planner_calls"))?,
            charged_input_tokens: current
                .charged_input_tokens
                .checked_add(u64::from(reserved_input_tokens))
                .ok_or_else(|| overflow("charged_input_tokens"))?,
            ..current
        };
        write_event(&transaction, branch, trigger_evidence_key, next)?;
        transaction.commit()?;
        Ok(Some(next))
    }

    /// Replaces one call's reservation with its final charge and records the
    /// measured receipt, if any, in the same transaction.
    pub fn settle_plan_planner_call(
        &mut self,
        branch: &PlanBranchId,
        trigger_evidence_key: &str,
        settlement: &PlannerCallSettlement<'_>,
    ) -> StorageResult<PlanGuardrailEvent> {
        require_evidence_key(trigger_evidence_key)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_event(&transaction, branch, trigger_evidence_key)?.ok_or_else(|| {
            StorageError::NotFound(format!(
                "plan branch {} has no reserved planner call for this evidence event",
                branch.as_str()
            ))
        })?;
        if current.planner_calls == 0 {
            return Err(StorageError::ConstraintViolation(
                "planner call settlement without a reserved call".to_owned(),
            ));
        }
        let charged_input_tokens = current
            .charged_input_tokens
            .checked_sub(u64::from(settlement.reserved_input_tokens))
            .ok_or_else(|| {
                StorageError::ConstraintViolation(
                    "planner call settlement releases more tokens than were reserved".to_owned(),
                )
            })?
            .checked_add(settlement.charged_input_tokens)
            .ok_or_else(|| overflow("charged_input_tokens"))?;
        if let Some((receipt_ref, receipt)) = settlement.measured {
            insert_plan_cost_receipt(&transaction, receipt_ref, receipt)?;
        }
        let next = PlanGuardrailEvent {
            charged_input_tokens,
            ..current
        };
        write_event(&transaction, branch, trigger_evidence_key, next)?;
        transaction.commit()?;
        Ok(next)
    }

    /// Records one refused proposal for the event (L3 input).
    pub fn record_plan_proposal_rejection(
        &mut self,
        branch: &PlanBranchId,
        trigger_evidence_key: &str,
    ) -> StorageResult<PlanGuardrailEvent> {
        require_evidence_key(trigger_evidence_key)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = ensure_event(&transaction, branch, trigger_evidence_key)?;
        let next = PlanGuardrailEvent {
            consecutive_rejections: current
                .consecutive_rejections
                .checked_add(1)
                .ok_or_else(|| overflow("consecutive_rejections"))?,
            ..current
        };
        write_event(&transaction, branch, trigger_evidence_key, next)?;
        transaction.commit()?;
        Ok(next)
    }
}

/// Canonical `sha256:<64 lowercase hex>` key check shared with revision
/// lineage. Keys are derived by `plan_domain::planning_evidence_key`.
pub(crate) fn require_evidence_key(key: &str) -> StorageResult<()> {
    let valid = key.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(StorageError::ConstraintViolation(
            "trigger evidence key must use canonical sha256:<hex> format".to_owned(),
        ))
    }
}

/// Resets the L3 counter after an accepted revision; runs inside the
/// revision commit transaction.
pub(crate) fn reset_rejections(
    connection: &Connection,
    branch: &PlanBranchId,
    trigger_evidence_key: &str,
) -> StorageResult<()> {
    connection.execute(
        "UPDATE plan_guardrail_events SET consecutive_rejections = 0
         WHERE plan_branch_id = ?1 AND trigger_evidence_key = ?2",
        params![branch.as_str(), trigger_evidence_key],
    )?;
    Ok(())
}

fn ensure_event(
    connection: &Connection,
    branch: &PlanBranchId,
    key: &str,
) -> StorageResult<PlanGuardrailEvent> {
    connection.execute(
        "INSERT INTO plan_guardrail_events (plan_branch_id, trigger_evidence_key)
         VALUES (?1, ?2)
         ON CONFLICT(plan_branch_id, trigger_evidence_key) DO NOTHING",
        params![branch.as_str(), key],
    )?;
    load_event(connection, branch, key)?
        .ok_or_else(|| StorageError::Corrupt("inserted planning guardrail row is missing".into()))
}

fn load_event(
    connection: &Connection,
    branch: &PlanBranchId,
    key: &str,
) -> StorageResult<Option<PlanGuardrailEvent>> {
    let row: Option<(i64, i64, i64)> = connection
        .query_row(
            "SELECT planner_calls, charged_input_tokens, consecutive_rejections
             FROM plan_guardrail_events
             WHERE plan_branch_id = ?1 AND trigger_evidence_key = ?2",
            params![branch.as_str(), key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((calls, charged, rejections)) = row else {
        return Ok(None);
    };
    Ok(Some(PlanGuardrailEvent {
        planner_calls: u32::try_from(calls).map_err(|_| corrupt("planner_calls"))?,
        charged_input_tokens: u64::try_from(charged)
            .map_err(|_| corrupt("charged_input_tokens"))?,
        consecutive_rejections: u32::try_from(rejections)
            .map_err(|_| corrupt("consecutive_rejections"))?,
    }))
}

fn write_event(
    connection: &Connection,
    branch: &PlanBranchId,
    key: &str,
    event: PlanGuardrailEvent,
) -> StorageResult<()> {
    let charged = i64::try_from(event.charged_input_tokens).map_err(|_| {
        StorageError::ConstraintViolation(
            "charged_input_tokens exceeds SQLite INTEGER range".to_owned(),
        )
    })?;
    let updated = connection.execute(
        "UPDATE plan_guardrail_events
         SET planner_calls = ?3, charged_input_tokens = ?4, consecutive_rejections = ?5
         WHERE plan_branch_id = ?1 AND trigger_evidence_key = ?2",
        params![
            branch.as_str(),
            key,
            i64::from(event.planner_calls),
            charged,
            i64::from(event.consecutive_rejections),
        ],
    )?;
    if updated != 1 {
        return Err(StorageError::Corrupt(
            "planning guardrail row vanished inside its write transaction".to_owned(),
        ));
    }
    Ok(())
}

fn corrupt(field: &str) -> StorageError {
    StorageError::Corrupt(format!("planning guardrail {field} is out of range"))
}

fn overflow(field: &str) -> StorageError {
    StorageError::ConstraintViolation(format!("planning guardrail {field} would overflow"))
}
