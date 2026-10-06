use evaluation::{
    candidate_sha256, subject_fingerprint, CandidateCheckpointPhase, CandidateDeletion,
    CandidateFreshnessCheckpoint, CandidateIntegrityAssessment, CandidateIntegrityOutcome,
    CaptureAuthority, EffectId, EffectKind, EffectPayload, EffectProvenance, EffectSubjectRef,
    ObservedEffect, SubjectFingerprint,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use security_audit::{SecurityAuditEvent, SecurityAuditOutcome, SecurityAuditSubjectKind};

use crate::{StorageError, StorageResult};

type CandidateOperationRow = (
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateEvidence {
    pub effect: ObservedEffect,
    pub integrity: CandidateIntegrityAssessment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSelectionOutcome {
    /// A new candidate operation row was inserted.
    Recorded,
    /// Same subject and already in RESULT_ADMITTED or COMPLETE state; can be reused.
    ReuseAdmitted,
    /// Different subject (integrity signal): candidate operation is bound to another subject.
    SubjectMismatch,
    /// Same subject but in an intermediate state (PENDING, BLOCKED, etc.); operation is incomplete or ambiguous.
    Incomplete,
}

impl crate::SqliteStore {
    pub fn candidate_operation_status(
        &self,
        execution_id: &str,
        attempt_id: &str,
    ) -> StorageResult<Option<String>> {
        Ok(self.connection().query_row(
            "SELECT status FROM candidate_input_operations WHERE execution_id = ?1 AND attempt_id = ?2",
            params![execution_id, attempt_id],
            |row| row.get(0),
        ).optional()?)
    }

    pub fn candidate_operation_history(
        &self,
        execution_id: &str,
        attempt_id: &str,
    ) -> StorageResult<Option<String>> {
        Ok(self.connection().query_row(
            "SELECT status_history FROM candidate_input_operations WHERE execution_id = ?1 AND attempt_id = ?2",
            params![execution_id, attempt_id],
            |row| row.get(0),
        ).optional()?)
    }

    /// Returns the invalidation timestamp and code if the operation is invalidated,
    /// None if it is still valid.
    pub fn candidate_operation_invalidation(
        &self,
        execution_id: &str,
        attempt_id: &str,
    ) -> StorageResult<Option<(u64, String)>> {
        candidate_operation_invalidation_on(self.connection(), execution_id, attempt_id)
    }

    /// Durably binds the selected subject before object graph resolution. An
    /// interrupted PENDING operation is intentionally ambiguous and cannot
    /// be silently replayed; only a fully admitted/completed identical
    /// operation may be reopened for restart reuse.
    #[allow(clippy::too_many_arguments)]
    pub fn record_candidate_selection(
        &mut self,
        task_id: &str,
        execution_id: &str,
        attempt_id: &str,
        operation_id: &str,
        base_revision: &str,
        selected_commit_oid: &str,
        requirement_revision: &str,
        policy_revision: u64,
        policy_fingerprint: &str,
        criterion_id: &str,
        selector: &str,
        selector_path: &str,
        selector_literal: &str,
        now_ms: u64,
    ) -> StorageResult<CandidateSelectionOutcome> {
        let existing: Option<CandidateOperationRow> = self
            .connection()
            .query_row(
                "SELECT operation_id, base_revision, selected_commit_oid, requirement_revision,
                    policy_revision, policy_fingerprint, criterion_id, selector,
                    selector_path, selector_literal, status
             FROM candidate_input_operations WHERE execution_id = ?1 AND attempt_id = ?2",
                params![execution_id, attempt_id],
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
                    ))
                },
            )
            .optional()?;
        if let Some(existing) = existing {
            let stored_task: String = self.connection().query_row(
                "SELECT task_id FROM candidate_input_operations WHERE execution_id = ?1 AND attempt_id = ?2",
                params![execution_id, attempt_id],
                |row| row.get(0),
            )?;
            let same_subject = existing.1 == base_revision
                && stored_task == task_id
                && existing.2 == selected_commit_oid
                && existing.3 == requirement_revision
                && existing.4 == policy_revision as i64
                && existing.5 == policy_fingerprint
                && existing.6 == criterion_id
                && existing.7 == selector
                && existing.8 == selector_path
                && existing.9 == selector_literal;
            if same_subject && matches!(existing.10.as_str(), "RESULT_ADMITTED" | "COMPLETE") {
                return Ok(CandidateSelectionOutcome::ReuseAdmitted);
            }
            if !same_subject {
                return Ok(CandidateSelectionOutcome::SubjectMismatch);
            }
            return Ok(CandidateSelectionOutcome::Incomplete);
        }
        self.connection().execute(
            "INSERT INTO candidate_input_operations (
                task_id, execution_id, attempt_id, operation_id, base_revision,
                selected_commit_oid, requirement_revision, policy_revision,
                policy_fingerprint, criterion_id, selector, selector_path,
                selector_literal, status, status_history, started_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'PENDING', 'PENDING', ?14, ?14)",
            params![task_id, execution_id, attempt_id, operation_id, base_revision,
                selected_commit_oid, requirement_revision, policy_revision as i64, policy_fingerprint,
                criterion_id, selector, selector_path, selector_literal, now_ms.min(i64::MAX as u64) as i64],
        )?;
        Ok(CandidateSelectionOutcome::Recorded)
    }

    pub fn update_candidate_operation_status(
        &mut self,
        execution_id: &str,
        attempt_id: &str,
        expected: &str,
        status: &str,
        detail_code: Option<&str>,
        now_ms: u64,
    ) -> StorageResult<()> {
        let changed = self.connection().execute(
            "UPDATE candidate_input_operations SET status = ?1, detail_code = ?2, updated_at_ms = ?3,
                    status_history = substr(status_history || char(10) || ?7, -8191)
             WHERE execution_id = ?4 AND attempt_id = ?5 AND status = ?6 AND invalidated_at_ms IS NULL",
            params![status, detail_code, now_ms.min(i64::MAX as u64) as i64, execution_id, attempt_id, expected, format!("{status}:{detail_code:?}:{}", now_ms)],
        )?;
        if changed != 1 {
            // Check if the row is invalidated to provide a better error message
            if let Some((_, code)) =
                self.candidate_operation_invalidation(execution_id, attempt_id)?
            {
                return Err(StorageError::ConstraintViolation(format!(
                    "candidate evidence invalidated: {code}"
                )));
            }
            return Err(StorageError::ConstraintViolation(
                "candidate operation status transition is stale or ambiguous".into(),
            ));
        }
        Ok(())
    }

    /// Journals a failure to status_history. Changes status and detail_code only
    /// for rows that are not in RESULT_ADMITTED or COMPLETE state; already-admitted
    /// evidence is never downgraded. R2 (owner review 2026-09-25, MAJOR): this used
    /// to run an unconditional `UPDATE`, so a failure classified from *any* later
    /// call — including one racing or following an already-admitted subject — could
    /// downgrade a durable `RESULT_ADMITTED` or `COMPLETE` row. Infrastructure and
    /// precondition errors are journaled only; they must NOT invalidate, so admitted
    /// evidence survives a transient failure and replay stays safe. TASK-0144: for
    /// integrity-signal failures, use `invalidate_candidate_operation` instead to
    /// permanently block reuse and completion.
    pub fn record_candidate_operation_failure(
        &mut self,
        execution_id: &str,
        attempt_id: &str,
        status: &str,
        detail_code: &str,
        now_ms: u64,
    ) -> StorageResult<()> {
        if !matches!(status, "MISMATCH" | "BLOCKED" | "TAMPERED") {
            return Err(StorageError::ConstraintViolation(
                "invalid candidate operation failure status".into(),
            ));
        }
        let now_ms = now_ms.min(i64::MAX as u64) as i64;
        let changed = self.connection().execute(
            "UPDATE candidate_input_operations SET
                    status = CASE WHEN status IN ('RESULT_ADMITTED','COMPLETE') THEN status ELSE ?1 END,
                    detail_code = CASE WHEN status IN ('RESULT_ADMITTED','COMPLETE') THEN detail_code ELSE ?2 END,
                    updated_at_ms = ?3,
                    status_history = substr(status_history || char(10) || ?6, -8191)
             WHERE execution_id = ?4 AND attempt_id = ?5",
            params![status, detail_code, now_ms, execution_id, attempt_id, format!("{status}:{detail_code}:{now_ms}")],
        )?;
        if changed != 1 {
            return Err(StorageError::ConstraintViolation(
                "candidate operation failure record is unavailable".into(),
            ));
        }
        Ok(())
    }

    /// Permanently invalidates candidate evidence with a MISMATCH or TAMPERED status.
    /// This records an integrity signal at the call site and prevents reuse or
    /// completion. Sets invalidated_at_ms and invalidation_code once (sticky),
    /// appends to status_history, and updates status/detail_code with the same
    /// CASE protection for admitted/complete rows as other terminal-state updates.
    /// Returns Ok(true) if the row was updated, Ok(false) if there is no row for
    /// (execution_id, attempt_id), and Err if status is invalid or a real storage error occurs.
    pub fn invalidate_candidate_operation(
        &mut self,
        execution_id: &str,
        attempt_id: &str,
        status: &str,
        detail_code: &str,
        now_ms: u64,
    ) -> StorageResult<bool> {
        if !matches!(status, "MISMATCH" | "TAMPERED") {
            return Err(StorageError::ConstraintViolation(
                "invalidate_candidate_operation requires status MISMATCH or TAMPERED".into(),
            ));
        }
        let now_ms = now_ms.min(i64::MAX as u64) as i64;
        let invalidation_code = format!("{status}:{detail_code}");
        let changed = self.connection().execute(
            "UPDATE candidate_input_operations SET
                    status = CASE WHEN status IN ('RESULT_ADMITTED','COMPLETE') THEN status ELSE ?1 END,
                    detail_code = CASE WHEN status IN ('RESULT_ADMITTED','COMPLETE') THEN detail_code ELSE ?2 END,
                    updated_at_ms = ?3,
                    invalidated_at_ms = COALESCE(invalidated_at_ms, ?3),
                    invalidation_code = COALESCE(invalidation_code, ?7),
                    status_history = substr(status_history || char(10) || ?6, -8191)
             WHERE execution_id = ?4 AND attempt_id = ?5",
            params![status, detail_code, now_ms, execution_id, attempt_id, format!("{status}:{detail_code}:{now_ms}"), invalidation_code],
        )?;
        Ok(changed == 1)
    }

    pub fn get_candidate_evidence(
        &self,
        execution_id: &str,
        attempt_id: &str,
    ) -> StorageResult<Option<CandidateEvidence>> {
        self.load_candidate_evidence(execution_id, attempt_id, true)
    }

    /// Completion-only product hook. It admits the result after the four
    /// pre-completion freshness checkpoints; the public reuse getter above
    /// remains gated on a durable matching Completion checkpoint.
    pub fn get_candidate_evidence_for_completion(
        &self,
        execution_id: &str,
        attempt_id: &str,
    ) -> StorageResult<Option<CandidateEvidence>> {
        self.load_candidate_evidence(execution_id, attempt_id, false)
    }

    fn load_candidate_evidence(
        &self,
        execution_id: &str,
        attempt_id: &str,
        require_completion: bool,
    ) -> StorageResult<Option<CandidateEvidence>> {
        // Check if the operation is invalidated before attempting to load evidence
        if let Some((_, code)) = self.candidate_operation_invalidation(execution_id, attempt_id)? {
            return Err(StorageError::ConstraintViolation(format!(
                "candidate evidence invalidated: {code}"
            )));
        }
        let row: Option<String> = self
            .connection()
            .query_row(
                "SELECT effect_id FROM candidate_input_assessments
                 WHERE execution_id = ?1 AND attempt_id = ?2
                 ORDER BY completed_at_ms DESC, assessment_id DESC LIMIT 1",
                params![execution_id, attempt_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(effect_id) = row else {
            return Ok(None);
        };
        let effect_id =
            EffectId::new(effect_id).map_err(|error| StorageError::Corrupt(error.to_string()))?;
        let effect = match self.get_observed_effect(&effect_id)? {
            Some(effect) => effect,
            None if self
                .candidate_operation_invalidation(execution_id, attempt_id)?
                .is_some() =>
            {
                return Err(StorageError::ConstraintViolation(
                    "candidate result effect is quarantined by invalidation".into(),
                ));
            }
            None => {
                return Err(StorageError::Corrupt(
                    "candidate result effect is missing".into(),
                ));
            }
        };
        let integrity = self
            .get_candidate_integrity_assessment(execution_id, attempt_id)?
            .ok_or_else(|| {
                StorageError::Corrupt("candidate integrity assessment is missing".into())
            })?;
        let required_phases = [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ];
        if required_phases.iter().any(|phase| {
            !integrity
                .checkpoints
                .iter()
                .any(|checkpoint| checkpoint.phase == *phase && checkpoint.matched)
        }) || (require_completion
            && !integrity.checkpoints.iter().any(|checkpoint| {
                checkpoint.phase == CandidateCheckpointPhase::Completion && checkpoint.matched
            }))
        {
            return Err(StorageError::Corrupt(
                "candidate evidence is missing a required freshness checkpoint".to_owned(),
            ));
        }
        let lineage_task: Option<String> = self
            .connection()
            .query_row(
                "SELECT e.task_id FROM executions e
                 JOIN execution_attempts a ON a.execution_id = e.id
                 WHERE e.id = ?1 AND a.attempt_id = ?2",
                params![execution_id, attempt_id],
                |row| row.get(0),
            )
            .optional()?;
        if lineage_task.as_deref() != Some(integrity.task_id.as_str()) {
            return Err(StorageError::Corrupt(
                "candidate assessment task lineage is invalid".to_owned(),
            ));
        }
        let expected_fingerprint = candidate_result_fingerprint(&integrity)
            .map_err(|error| StorageError::Corrupt(error.to_string()))?;
        if effect.effect_kind() != EffectKind::TestSuiteResult
            || effect.provenance().execution_id.as_str() != execution_id
            || effect.provenance().attempt_id.as_str() != attempt_id
            || effect.subject_ref().as_str() != integrity.subject_ref
            || effect.subject_fingerprint() != &expected_fingerprint
            || effect.provenance().runtime_ref != integrity.runtime_ref
            || effect.provenance().evaluator_ref.as_deref()
                != Some(integrity.evaluator_ref.as_str())
            || effect.provenance().receipt_ref.as_deref() != Some(integrity.invocation_id.as_str())
        {
            return Err(StorageError::Corrupt(
                "candidate effect and integrity assessment disagree".to_owned(),
            ));
        }
        let enforcement_audit = self
            .list_security_audit_by_correlation(&integrity.enforcement_ref)?
            .into_iter()
            .any(|record| {
                let fact = record.fact;
                fact.event == SecurityAuditEvent::IsolationEnforcement
                    && fact.outcome == Some(SecurityAuditOutcome::Enforced)
                    && fact.operation_id.as_deref() == Some(integrity.enforcement_ref.as_str())
                    && fact.task_id.as_deref() == Some(integrity.task_id.as_str())
                    && fact.execution_id.as_deref() == Some(integrity.execution_id.as_str())
                    && fact.attempt_id.as_deref() == Some(integrity.attempt_id.as_str())
                    && fact.subject.as_ref().is_some_and(|subject| {
                        subject.kind == SecurityAuditSubjectKind::RunnerBackend
                            && subject.id == integrity.runner_backend
                    })
            });
        if !enforcement_audit {
            return Err(StorageError::Corrupt(
                "candidate invocation has no matching durable Runner enforcement record".into(),
            ));
        }
        Ok(Some(CandidateEvidence { effect, integrity }))
    }

    pub fn append_candidate_result(
        &mut self,
        subject_ref: EffectSubjectRef,
        subject_fingerprint: SubjectFingerprint,
        payload: EffectPayload,
        captured_at: impl Into<String>,
        provenance: EffectProvenance,
        assessment: &CandidateIntegrityAssessment,
    ) -> StorageResult<ObservedEffect> {
        assessment
            .validate()
            .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        if subject_fingerprint
            != candidate_result_fingerprint(assessment)
                .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?
        {
            return Err(StorageError::ConstraintViolation(
                "candidate result fingerprint does not bind its assessment".to_owned(),
            ));
        }
        if payload.kind() != EffectKind::TestSuiteResult
            || assessment.execution_id != provenance.execution_id.as_str()
            || assessment.attempt_id != provenance.attempt_id.as_str()
            || assessment.subject_ref != subject_ref.as_str()
            || assessment.runtime_ref != provenance.runtime_ref
            || provenance.evaluator_ref.as_deref() != Some(assessment.evaluator_ref.as_str())
            || provenance.receipt_ref.as_deref() != Some(assessment.invocation_id.as_str())
        {
            return Err(StorageError::ConstraintViolation(
                "candidate assessment does not bind its captured result".to_owned(),
            ));
        }
        let captured_at = captured_at.into();
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let lineage: Option<(String, String)> = transaction
            .query_row(
                "SELECT e.task_id, a.execution_id FROM executions e
             JOIN execution_attempts a ON a.execution_id = e.id
             WHERE e.id = ?1 AND a.attempt_id = ?2",
                params![
                    provenance.execution_id.as_str(),
                    provenance.attempt_id.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if lineage.as_ref().is_none_or(|(task, execution)| {
            task != &assessment.task_id || execution != assessment.execution_id.as_str()
        }) {
            return Err(StorageError::Corrupt(
                "candidate result task/execution/attempt lineage mismatch".to_owned(),
            ));
        }
        // TASK-0151 finding 2: a result cannot land on an invalidated
        // operation. Checked inside this transaction, atomically with the
        // insert below — a concurrent invalidation either precedes this
        // read (refused here) or blocks on the write lock until commit.
        let invalidated: Option<String> = transaction
            .query_row(
                "SELECT invalidation_code FROM candidate_input_operations
                 WHERE execution_id = ?1 AND attempt_id = ?2",
                params![
                    provenance.execution_id.as_str(),
                    provenance.attempt_id.as_str()
                ],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if invalidated.is_some() {
            return Err(StorageError::ConstraintViolation(
                "candidate operation is invalidated; result refused".to_owned(),
            ));
        }
        let next: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(CAST(substr(effect_id, 8) AS INTEGER)), 0) + 1
             FROM observed_effects WHERE effect_id GLOB 'effect-[0-9]*'",
            [],
            |row| row.get(0),
        )?;
        let effect = ObservedEffect::restore(
            EffectId::new(format!("effect-{}", next.max(1)))
                .map_err(|error| StorageError::Corrupt(error.to_string()))?,
            subject_ref,
            subject_fingerprint,
            EffectKind::TestSuiteResult,
            CaptureAuthority::LocalHarness,
            captured_at,
            payload,
            provenance,
        )
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        insert_effect(&transaction, &effect)?;
        insert_assessment(&transaction, effect.effect_id(), assessment)?;
        transaction.commit()?;
        Ok(effect)
    }

    pub fn get_candidate_integrity_assessment(
        &self,
        execution_id: &str,
        attempt_id: &str,
    ) -> StorageResult<Option<CandidateIntegrityAssessment>> {
        get_candidate_integrity_assessment_on(self.connection(), execution_id, attempt_id)
    }

    /// Appends a matching completion or post-restart reuse checkpoint.
    pub fn append_candidate_checkpoint(
        &mut self,
        execution_id: &str,
        attempt_id: &str,
        checkpoint: &CandidateFreshnessCheckpoint,
    ) -> StorageResult<()> {
        if !matches!(
            checkpoint.phase,
            CandidateCheckpointPhase::Completion | CandidateCheckpointPhase::RestartReuse
        ) || !checkpoint.matched
        {
            return Err(StorageError::ConstraintViolation(
                "candidate admission requires a matching completion or restart checkpoint"
                    .to_owned(),
            ));
        }
        // Check if the operation is invalidated before attempting to append checkpoint
        if let Some((_, code)) = self.candidate_operation_invalidation(execution_id, attempt_id)? {
            return Err(StorageError::ConstraintViolation(format!(
                "candidate evidence invalidated: {code}"
            )));
        }
        self.connection_mut().execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut assessment = self
                .get_candidate_integrity_assessment(execution_id, attempt_id)?
                .ok_or_else(|| StorageError::Corrupt("candidate assessment is missing".into()))?;
            let duplicate_phase = assessment
                .checkpoints
                .iter()
                .find(|item| item.phase == checkpoint.phase);
            if checkpoint.phase == CandidateCheckpointPhase::Completion {
                if let Some(previous) = duplicate_phase {
                    if assessment.outcome == CandidateIntegrityOutcome::Intact
                        && previous.matched
                        && previous.git_metadata_fingerprint == checkpoint.git_metadata_fingerprint
                        && previous.live_worktree_manifest_sha256
                            == checkpoint.live_worktree_manifest_sha256
                    {
                        return Ok(());
                    }
                    return Err(StorageError::ConstraintViolation(
                        "repeated candidate completion does not match the durable subject".into(),
                    ));
                }
            }
            let invalid_phase_order = match checkpoint.phase {
                CandidateCheckpointPhase::Completion => [
                    CandidateCheckpointPhase::Selection,
                    CandidateCheckpointPhase::BeforeEvaluation,
                    CandidateCheckpointPhase::AfterEvaluation,
                    CandidateCheckpointPhase::ResultAdmission,
                ]
                .iter()
                .any(|phase| {
                    !assessment
                        .checkpoints
                        .iter()
                        .any(|item| item.phase == *phase && item.matched)
                }),
                CandidateCheckpointPhase::RestartReuse => !assessment
                    .checkpoints
                    .iter()
                    .any(|item| item.phase == CandidateCheckpointPhase::Completion && item.matched),
                _ => true,
            };
            if assessment.outcome != CandidateIntegrityOutcome::Intact || invalid_phase_order {
                return Err(StorageError::ConstraintViolation(
                    "candidate assessment phase transition is invalid".to_owned(),
                ));
            }
            assessment.checkpoints.push(checkpoint.clone());
            assessment
                .validate()
                .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
            let encoded = encode_checkpoints(&assessment.checkpoints);
            let changed = self.connection().execute(
                "UPDATE candidate_input_assessments SET checkpoints = ?1
                 WHERE execution_id = ?2 AND attempt_id = ?3 AND effect_id = (
                     SELECT effect_id FROM candidate_input_assessments
                     WHERE execution_id = ?2 AND attempt_id = ?3
                     ORDER BY completed_at_ms DESC, assessment_id DESC LIMIT 1
                 )",
                params![encoded, execution_id, attempt_id],
            )?;
            if changed != 1 {
                return Err(StorageError::Corrupt(
                    "candidate assessment disappeared during checkpoint append".to_owned(),
                ));
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.connection_mut().execute_batch("COMMIT")?,
            Err(error) => {
                let _ = self.connection_mut().execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(())
    }
}

fn candidate_result_fingerprint(
    assessment: &CandidateIntegrityAssessment,
) -> Result<SubjectFingerprint, evaluation::EffectError> {
    let config = candidate_sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            assessment.requirement_revision,
            assessment.policy_fingerprint,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_descriptor_sha256,
        )
        .as_bytes(),
    );
    subject_fingerprint(
        EffectKind::TestSuiteResult,
        &assessment.subject_ref,
        &[
            ("content", &assessment.candidate_manifest_sha256),
            ("toolchain", &assessment.runtime_ref),
            ("config", &config),
        ],
    )
}

fn insert_effect(
    transaction: &rusqlite::Transaction<'_>,
    effect: &ObservedEffect,
) -> StorageResult<()> {
    let provenance = effect.provenance();
    transaction
        .execute(
            "INSERT INTO observed_effects (
            effect_id, subject_ref, subject_fingerprint, effect_kind, captured_by,
            captured_at, payload_ref, execution_id, attempt_id, runtime_ref,
            evaluator_ref, receipt_ref
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            ],
        )
        .map_err(|error| crate::error::map_constraint(error, "store candidate result"))?;
    Ok(())
}

fn insert_assessment(
    transaction: &rusqlite::Transaction<'_>,
    effect_id: &EffectId,
    assessment: &CandidateIntegrityAssessment,
) -> StorageResult<()> {
    let checkpoints = encode_checkpoints(&assessment.checkpoints);
    let outcome = match assessment.outcome {
        CandidateIntegrityOutcome::Intact => "INTACT",
        CandidateIntegrityOutcome::Mismatch => "MISMATCH",
    };
    transaction
        .execute(
            "INSERT INTO candidate_input_assessments (
            assessment_id, effect_id, task_id, execution_id, attempt_id,
            subject_ref, requirement_revision, policy_revision, policy_fingerprint,
            criterion_id, selector, selector_path, selector_literal, invocation_id, evaluator_ref, runtime_ref,
            enforcement_ref, base_commit_oid, base_tree_oid, base_manifest_sha256,
            candidate_commit_oid, candidate_tree_oid, candidate_manifest_sha256,
            checkpoints, integrity_outcome, started_at_ms, completed_at_ms, deletions,
            invocation_descriptor_version, invocation_descriptor_sha256,
            evaluator_executable, toolchain_ref, runner_backend, runner_plan_sha256,
            runner_enforced_axes, runner_env_isolated
         ) VALUES (
            'candidate-assessment-' || lower(hex(randomblob(16))), ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29,
            ?30, ?31, ?32, ?33, ?34, ?35
         )",
            params![
                effect_id.as_str(),
                assessment.task_id,
                assessment.execution_id,
                assessment.attempt_id,
                assessment.subject_ref,
                assessment.requirement_revision,
                assessment.policy_revision as i64,
                assessment.policy_fingerprint,
            assessment.criterion_id,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_id,
                assessment.evaluator_ref,
                assessment.runtime_ref,
                assessment.enforcement_ref,
                assessment.base_commit_oid,
                assessment.base_tree_oid,
                assessment.base_manifest_sha256,
                assessment.candidate_commit_oid,
                assessment.candidate_tree_oid,
                assessment.candidate_manifest_sha256,
                checkpoints,
                outcome,
                assessment.started_at_ms as i64,
                assessment.completed_at_ms as i64,
                encode_deletions(&assessment.deletions),
                assessment.invocation_descriptor_version,
                assessment.invocation_descriptor_sha256,
                assessment.evaluator_executable,
                assessment.toolchain_ref,
                assessment.runner_backend,
                assessment.runner_plan_sha256,
                assessment.runner_enforced_axes,
                i64::from(assessment.runner_env_isolated),
            ],
        )
        .map_err(|error| crate::error::map_constraint(error, "store candidate assessment"))?;
    Ok(())
}

fn encode_deletions(deletions: &[CandidateDeletion]) -> String {
    use std::fmt::Write;
    let mut encoded = String::from("candidate-deletions/v1\n");
    for deletion in deletions {
        for byte in &deletion.path {
            let _ = write!(encoded, "{byte:02x}");
        }
        let _ = writeln!(
            encoded,
            "|{:o}|{}",
            deletion.base_mode, deletion.base_blob_sha256
        );
    }
    encoded
}

fn decode_deletions(value: &str) -> StorageResult<Vec<CandidateDeletion>> {
    let body = value
        .strip_prefix("candidate-deletions/v1\n")
        .ok_or_else(|| {
            StorageError::Corrupt("candidate deletion manifest version is invalid".to_owned())
        })?;
    let mut deletions = Vec::new();
    for line in body.lines() {
        let mut fields = line.split('|');
        let path_hex = fields.next().unwrap_or_default();
        let mode = fields.next().unwrap_or_default();
        let blob_digest = fields.next().unwrap_or_default();
        if fields.next().is_some()
            || path_hex.is_empty()
            || !path_hex.len().is_multiple_of(2)
            || !path_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(StorageError::Corrupt(
                "candidate deletion manifest entry is malformed".to_owned(),
            ));
        }
        let path = path_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let value = std::str::from_utf8(pair).map_err(|_| {
                    StorageError::Corrupt("candidate deletion path is malformed".to_owned())
                })?;
                u8::from_str_radix(value, 16).map_err(|_| {
                    StorageError::Corrupt("candidate deletion path is malformed".to_owned())
                })
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let base_mode = u32::from_str_radix(mode, 8)
            .map_err(|_| StorageError::Corrupt("candidate deletion mode is invalid".to_owned()))?;
        deletions.push(CandidateDeletion {
            path,
            base_mode,
            base_blob_sha256: blob_digest.to_owned(),
        });
    }
    if encode_deletions(&deletions) != value {
        return Err(StorageError::Corrupt(
            "candidate deletion manifest is not canonical".to_owned(),
        ));
    }
    Ok(deletions)
}

fn encode_checkpoints(checkpoints: &[CandidateFreshnessCheckpoint]) -> String {
    let mut encoded = String::new();
    for checkpoint in checkpoints {
        use std::fmt::Write;
        let _ = writeln!(
            encoded,
            "{}|{}|{}|{}|{}",
            checkpoint.phase.as_str(),
            checkpoint.observed_at_ms,
            checkpoint.git_metadata_fingerprint,
            checkpoint.live_worktree_manifest_sha256,
            u8::from(checkpoint.matched),
        );
    }
    encoded
}

fn decode_checkpoints(value: &str) -> StorageResult<Vec<CandidateFreshnessCheckpoint>> {
    let mut result = Vec::new();
    for line in value.lines() {
        let mut fields = line.split('|');
        let phase = match fields.next() {
            Some("selection") => CandidateCheckpointPhase::Selection,
            Some("before-evaluation") => CandidateCheckpointPhase::BeforeEvaluation,
            Some("after-evaluation") => CandidateCheckpointPhase::AfterEvaluation,
            Some("result-admission") => CandidateCheckpointPhase::ResultAdmission,
            Some("completion") => CandidateCheckpointPhase::Completion,
            Some("restart-reuse") => CandidateCheckpointPhase::RestartReuse,
            _ => {
                return Err(StorageError::Corrupt(
                    "invalid candidate checkpoint phase".into(),
                ))
            }
        };
        let observed_at_ms = fields
            .next()
            .and_then(|field| field.parse().ok())
            .ok_or_else(|| StorageError::Corrupt("invalid candidate checkpoint time".into()))?;
        let git_metadata_fingerprint = fields.next().unwrap_or_default().to_owned();
        let live_worktree_manifest_sha256 = fields.next().unwrap_or_default().to_owned();
        let matched = match fields.next() {
            Some("0") => false,
            Some("1") => true,
            _ => {
                return Err(StorageError::Corrupt(
                    "invalid candidate checkpoint outcome".into(),
                ))
            }
        };
        if fields.next().is_some() {
            return Err(StorageError::Corrupt(
                "ambiguous candidate checkpoint".into(),
            ));
        }
        result.push(CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms,
            git_metadata_fingerprint,
            live_worktree_manifest_sha256,
            matched,
        });
    }
    Ok(result)
}

pub(crate) fn candidate_operation_invalidation_on(
    connection: &Connection,
    execution_id: &str,
    attempt_id: &str,
) -> StorageResult<Option<(u64, String)>> {
    match connection.query_row(
        "SELECT invalidated_at_ms, invalidation_code FROM candidate_input_operations WHERE execution_id = ?1 AND attempt_id = ?2",
        params![execution_id, attempt_id],
        |row| {
            let invalidated_at_ms: Option<i64> = row.get(0)?;
            let invalidation_code: Option<String> = row.get(1)?;
            Ok((invalidated_at_ms, invalidation_code))
        },
    ) {
        Ok((Some(ms), Some(code))) => Ok(Some((u64::try_from(ms).unwrap_or(0), code))),
        Ok(_) => Ok(None),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn get_candidate_integrity_assessment_on(
    connection: &Connection,
    execution_id: &str,
    attempt_id: &str,
) -> StorageResult<Option<CandidateIntegrityAssessment>> {
    // Check if the operation is invalidated before attempting to load assessment
    if let Some((_, code)) =
        candidate_operation_invalidation_on(connection, execution_id, attempt_id)?
    {
        return Err(StorageError::ConstraintViolation(format!(
            "candidate evidence invalidated: {code}"
        )));
    }
    type Row = (
        String,
        String,
        String,
        i64,
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
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
    );
    let row: Option<Row> = connection
        .query_row(
            "SELECT task_id, subject_ref, requirement_revision, policy_revision,
                policy_fingerprint, criterion_id, selector, selector_path,
                selector_literal, invocation_id,
                evaluator_ref, runtime_ref, enforcement_ref, base_commit_oid,
                base_tree_oid, base_manifest_sha256, candidate_commit_oid,
                candidate_tree_oid, candidate_manifest_sha256, checkpoints,
                integrity_outcome, started_at_ms, completed_at_ms, deletions,
                invocation_descriptor_version, invocation_descriptor_sha256,
                evaluator_executable, toolchain_ref, runner_backend,
                runner_plan_sha256, runner_enforced_axes, runner_env_isolated
         FROM candidate_input_assessments WHERE execution_id = ?1 AND attempt_id = ?2
         ORDER BY completed_at_ms DESC, assessment_id DESC LIMIT 1",
            params![execution_id, attempt_id],
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
                    row.get(19)?,
                    row.get(20)?,
                    row.get(21)?,
                    row.get(22)?,
                    row.get(23)?,
                    row.get(24)?,
                    row.get(25)?,
                    row.get(26)?,
                    row.get(27)?,
                    row.get(28)?,
                    row.get(29)?,
                    row.get(30)?,
                    row.get(31)?,
                ))
            },
        )
        .optional()?;
    let Some((
        task_id,
        subject_ref,
        requirement_revision,
        policy_revision,
        policy_fingerprint,
        criterion_id,
        selector,
        selector_path,
        selector_literal,
        invocation_id,
        evaluator_ref,
        runtime_ref,
        enforcement_ref,
        base_commit_oid,
        base_tree_oid,
        base_manifest_sha256,
        candidate_commit_oid,
        candidate_tree_oid,
        candidate_manifest_sha256,
        checkpoints,
        outcome,
        started_at_ms,
        completed_at_ms,
        deletions,
        invocation_descriptor_version,
        invocation_descriptor_sha256,
        evaluator_executable,
        toolchain_ref,
        runner_backend,
        runner_plan_sha256,
        runner_enforced_axes,
        runner_env_isolated,
    )) = row
    else {
        return Ok(None);
    };
    let policy_revision = u64::try_from(policy_revision)
        .map_err(|_| StorageError::Corrupt("invalid candidate policy revision".into()))?;
    let started_at_ms = u64::try_from(started_at_ms)
        .map_err(|_| StorageError::Corrupt("invalid candidate start time".into()))?;
    let completed_at_ms = u64::try_from(completed_at_ms)
        .map_err(|_| StorageError::Corrupt("invalid candidate completion time".into()))?;
    let invocation_descriptor_version = u32::try_from(invocation_descriptor_version)
        .map_err(|_| StorageError::Corrupt("invalid candidate descriptor version".into()))?;
    let runner_enforced_axes = u8::try_from(runner_enforced_axes)
        .map_err(|_| StorageError::Corrupt("invalid candidate Runner axes".into()))?;
    let runner_env_isolated = match runner_env_isolated {
        0 => false,
        1 => true,
        _ => {
            return Err(StorageError::Corrupt(
                "invalid Runner env isolation flag".into(),
            ))
        }
    };
    let outcome = match outcome.as_str() {
        "INTACT" => CandidateIntegrityOutcome::Intact,
        "MISMATCH" => CandidateIntegrityOutcome::Mismatch,
        _ => return Err(StorageError::Corrupt("invalid candidate outcome".into())),
    };
    let assessment = CandidateIntegrityAssessment {
        task_id,
        execution_id: execution_id.to_owned(),
        attempt_id: attempt_id.to_owned(),
        subject_ref,
        requirement_revision,
        policy_revision,
        policy_fingerprint,
        criterion_id,
        selector,
        selector_path,
        selector_literal,
        invocation_id,
        evaluator_ref,
        runtime_ref,
        enforcement_ref,
        invocation_descriptor_version,
        invocation_descriptor_sha256,
        evaluator_executable,
        toolchain_ref,
        runner_backend,
        runner_plan_sha256,
        runner_enforced_axes,
        runner_env_isolated,
        base_commit_oid,
        base_tree_oid,
        base_manifest_sha256,
        candidate_commit_oid,
        candidate_tree_oid,
        candidate_manifest_sha256,
        deletions: decode_deletions(&deletions)?,
        checkpoints: decode_checkpoints(&checkpoints)?,
        outcome,
        started_at_ms,
        completed_at_ms,
    };
    assessment
        .validate()
        .map_err(|error| StorageError::Corrupt(error.to_string()))?;
    Ok(Some(assessment))
}
