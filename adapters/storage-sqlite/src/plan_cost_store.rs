//! Durable plan-scoped cost receipts for ADR-0027 §6.
//!
//! This is intentionally narrower than the future Usage Ledger: one opaque
//! receipt reference maps to the measured token/time evidence of one planner
//! call. A receipt may exist for a call whose proposal was later refused or
//! whose output was unusable — spend happened, so the evidence stays. No
//! provider text or general execution usage lives here.

use plan_domain::{PlanningCostReceipt, PlanningCostReceiptRef};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{StorageError, StorageResult};
use crate::task_store::SqliteStore;

impl SqliteStore {
    /// Records one planning receipt idempotently. Reusing a receipt identity
    /// for different evidence fails closed instead of overwriting history.
    pub fn record_plan_cost_receipt(
        &mut self,
        receipt_ref: &PlanningCostReceiptRef,
        receipt: &PlanningCostReceipt,
    ) -> StorageResult<()> {
        insert_plan_cost_receipt(self.connection(), receipt_ref, receipt)
    }

    /// Reads one planning receipt. Corrupt numeric/domain values fail closed.
    pub fn get_plan_cost_receipt(
        &self,
        receipt_ref: &PlanningCostReceiptRef,
    ) -> StorageResult<Option<PlanningCostReceipt>> {
        load_plan_cost_receipt(self.connection(), receipt_ref)
    }
}

/// Inserts one immutable receipt on `connection` (which may be inside a
/// caller-owned transaction). Exact replay is idempotent; a different value
/// under the same identity is a constraint violation.
pub(crate) fn insert_plan_cost_receipt(
    connection: &Connection,
    receipt_ref: &PlanningCostReceiptRef,
    receipt: &PlanningCostReceipt,
) -> StorageResult<()> {
    let input_tokens = i64::from(receipt.input_tokens);
    let output_tokens = i64::from(receipt.output_tokens);
    let wall_time_ms = i64::try_from(receipt.wall_time_ms).map_err(|_| {
        StorageError::ConstraintViolation(
            "planning cost receipt wall_time_ms exceeds SQLite INTEGER range".to_owned(),
        )
    })?;
    connection.execute(
        "INSERT INTO plan_cost_receipts
         (receipt_ref, input_tokens, output_tokens, wall_time_ms, price_class)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(receipt_ref) DO NOTHING",
        params![
            receipt_ref.as_str(),
            input_tokens,
            output_tokens,
            wall_time_ms,
            receipt.price_class.as_str()
        ],
    )?;

    let stored = load_plan_cost_receipt(connection, receipt_ref)?.ok_or_else(|| {
        StorageError::Corrupt("inserted planning cost receipt is missing".to_owned())
    })?;
    if stored != *receipt {
        return Err(StorageError::ConstraintViolation(format!(
            "planning cost receipt {} already exists with different evidence",
            receipt_ref.as_str()
        )));
    }
    Ok(())
}

pub(crate) fn load_plan_cost_receipt(
    connection: &Connection,
    receipt_ref: &PlanningCostReceiptRef,
) -> StorageResult<Option<PlanningCostReceipt>> {
    let row: Option<(i64, i64, i64, String)> = connection
        .query_row(
            "SELECT input_tokens, output_tokens, wall_time_ms, price_class
             FROM plan_cost_receipts WHERE receipt_ref = ?1",
            params![receipt_ref.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((input, output, wall, price_class)) = row else {
        return Ok(None);
    };
    let input_tokens = u32::try_from(input).map_err(|_| {
        StorageError::Corrupt("planning cost receipt input_tokens is out of range".to_owned())
    })?;
    let output_tokens = u32::try_from(output).map_err(|_| {
        StorageError::Corrupt("planning cost receipt output_tokens is out of range".to_owned())
    })?;
    let wall_time_ms = u64::try_from(wall).map_err(|_| {
        StorageError::Corrupt("planning cost receipt wall_time_ms is negative".to_owned())
    })?;
    let receipt = PlanningCostReceipt::new(input_tokens, output_tokens, wall_time_ms, price_class)
        .map_err(|error| {
            StorageError::DomainReconstruction(format!(
                "planning cost receipt reconstruction failed: {error}"
            ))
        })?;
    Ok(Some(receipt))
}
