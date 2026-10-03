use crate::{StorageError, StorageResult};
use evaluation::{
    CaptureAuthority, EffectId, EffectKind, EffectPayload, EffectProvenance, EffectSubjectRef,
    ObservedEffect, SubjectFingerprint,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

impl crate::SqliteStore {
    /// Persists one trusted captured effect. Identity allocation and insert
    /// share one IMMEDIATE transaction so concurrent writers cannot mint the
    /// same product ID.
    pub fn append_observed_effect(
        &mut self,
        subject_ref: EffectSubjectRef,
        subject_fingerprint: SubjectFingerprint,
        payload: EffectPayload,
        captured_at: impl Into<String>,
        provenance: EffectProvenance,
    ) -> StorageResult<ObservedEffect> {
        let captured_at = captured_at.into();
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let owner: Option<String> = transaction
            .query_row(
                "SELECT execution_id FROM execution_attempts WHERE attempt_id = ?1",
                params![provenance.attempt_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if owner.as_deref() != Some(provenance.execution_id.as_str()) {
            return Err(StorageError::Corrupt(
                "attempt/execution lineage mismatch".to_owned(),
            ));
        }

        let next: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(CAST(substr(effect_id, 8) AS INTEGER)), 0) + 1
             FROM observed_effects WHERE effect_id GLOB 'effect-[0-9]*'",
            [],
            |row| row.get(0),
        )?;
        let effect_id = EffectId::new(format!("effect-{}", next.max(1)))
            .map_err(|error| StorageError::Corrupt(error.to_string()))?;
        let effect = ObservedEffect::restore(
            effect_id,
            subject_ref,
            subject_fingerprint,
            payload.kind(),
            CaptureAuthority::LocalHarness,
            captured_at,
            payload,
            provenance,
        )
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        insert_effect(&transaction, &effect, "store observed effect")?;
        transaction.commit()?;
        Ok(effect)
    }

    pub fn get_observed_effect(&self, id: &EffectId) -> StorageResult<Option<ObservedEffect>> {
        load(self.connection(), id.as_str())
    }
}

/// Shared by every writer of `observed_effects` so the record fingerprint
/// is always stored with the row.
pub(crate) fn insert_effect(
    connection: &rusqlite::Connection,
    effect: &ObservedEffect,
    what: &'static str,
) -> StorageResult<()> {
    let provenance = effect.provenance();
    connection
        .execute(
            "INSERT INTO observed_effects (
                effect_id, subject_ref, subject_fingerprint, effect_kind,
                captured_by, captured_at, payload_ref, execution_id,
                attempt_id, runtime_ref, evaluator_ref, receipt_ref,
                record_fingerprint
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                effect.effect_id().as_str(),
                effect.subject_ref().as_str(),
                effect.subject_fingerprint().as_str(),
                effect.effect_kind().as_str(),
                effect.captured_by().as_str(),
                effect.captured_at(),
                effect.payload().encode(),
                provenance.execution_id.as_str(),
                provenance.attempt_id.as_str(),
                provenance.runtime_ref,
                provenance.evaluator_ref,
                provenance.receipt_ref,
                effect.record_fingerprint(),
            ],
        )
        .map_err(|error| crate::error::map_constraint(error, what))?;
    Ok(())
}

/// Migration 0025: fingerprints every pre-existing effect from its stored
/// (already restore-validated) content.
pub(crate) fn backfill_effect_fingerprints(connection: &rusqlite::Connection) -> StorageResult<()> {
    let ids: Vec<String> = connection
        .prepare("SELECT effect_id FROM observed_effects")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        let effect = load_row(connection, &id)?
            .ok_or_else(|| StorageError::Corrupt("observed effect vanished".to_owned()))?
            .0;
        connection.execute(
            "UPDATE observed_effects SET record_fingerprint = ?1 WHERE effect_id = ?2",
            params![effect.record_fingerprint(), id],
        )?;
    }
    Ok(())
}

fn load(connection: &rusqlite::Connection, id: &str) -> StorageResult<Option<ObservedEffect>> {
    let Some((effect, stored)) = load_row(connection, id)? else {
        return Ok(None);
    };
    if stored.as_deref() != Some(effect.record_fingerprint().as_str()) {
        return Err(StorageError::Corrupt(
            "observed effect record fingerprint mismatch".to_owned(),
        ));
    }
    Ok(Some(effect))
}

/// Decodes one row; returns the stored record fingerprint unverified.
fn load_row(
    connection: &rusqlite::Connection,
    id: &str,
) -> StorageResult<Option<(ObservedEffect, Option<String>)>> {
    type Row = (
        String,
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
        Option<String>,
        Option<String>,
    );
    let row: Option<Row> = connection
        .query_row(
            "SELECT effect_id, subject_ref, subject_fingerprint, effect_kind,
                    captured_by, captured_at, payload_ref, execution_id,
                    attempt_id, runtime_ref, evaluator_ref, receipt_ref, record_fingerprint
             FROM observed_effects WHERE effect_id = ?1",
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
                ))
            },
        )
        .optional()?;
    let Some((
        id,
        subject,
        fingerprint,
        kind,
        authority,
        captured_at,
        payload,
        execution,
        attempt,
        runtime,
        evaluator,
        receipt,
        stored_fingerprint,
    )) = row
    else {
        return Ok(None);
    };

    let kind = EffectKind::parse(&kind)
        .map_err(|_| StorageError::Corrupt("unknown observed effect kind".to_owned()))?;
    let authority = CaptureAuthority::parse(&authority)
        .map_err(|_| StorageError::Corrupt("unknown capture authority".to_owned()))?;
    let payload = EffectPayload::decode(kind, &payload)
        .map_err(|_| StorageError::Corrupt("invalid observed effect payload".to_owned()))?;
    let provenance = EffectProvenance::new(
        agent_runtime::ExecutionId::new(execution)
            .map_err(|_| StorageError::Corrupt("invalid execution lineage".to_owned()))?,
        agent_runtime::AttemptId::new(attempt)
            .map_err(|_| StorageError::Corrupt("invalid attempt lineage".to_owned()))?,
        runtime,
    )
    .and_then(|value| value.with_lineage(evaluator, receipt))
    .map_err(|_| StorageError::Corrupt("invalid effect provenance".to_owned()))?;
    let owner: Option<String> = connection
        .query_row(
            "SELECT execution_id FROM execution_attempts WHERE attempt_id = ?1",
            params![provenance.attempt_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if owner.as_deref() != Some(provenance.execution_id.as_str()) {
        return Err(StorageError::Corrupt(
            "attempt/execution lineage mismatch".to_owned(),
        ));
    }
    let effect = ObservedEffect::restore(
        EffectId::new(id).map_err(|_| StorageError::Corrupt("invalid effect id".to_owned()))?,
        EffectSubjectRef::new(subject)
            .map_err(|_| StorageError::Corrupt("invalid subject".to_owned()))?,
        SubjectFingerprint::new(fingerprint)
            .map_err(|_| StorageError::Corrupt("invalid fingerprint".to_owned()))?,
        kind,
        authority,
        captured_at,
        payload,
        provenance,
    )
    .map_err(|error| StorageError::Corrupt(error.to_string()))?;
    Ok(Some((effect, stored_fingerprint)))
}
