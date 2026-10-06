//! Durable G8 claim-verification seam (ADR-0030, TASK-0125).
//!
//! Append-only tables for agent claims, claim verifications (supersession
//! by reference, CV-5), policy exemptions (typed Low-risk gate repeated in
//! the schema, CV-4), human-approval discharges (guarded single-use
//! consume, CV-8), and the completion-admission records every COMPLETED
//! task carries (ADR-0018 consequence: replayable verdict-or-exemption
//! references). All records rebuild through the validated `evaluation`
//! constructors, so stored content can never bypass validation.

use evaluation::{
    AdmissionReport, AgentClaim, ClaimId, ClaimVerdict, ClaimVerification, ClaimedOutcome,
    ClaimedSubject, CriterionOutcome, EvidenceRefs, ExemptionId, PolicyExemption,
    SubjectFingerprint, VerificationId,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use task_domain::{RequirementKind, TaskId};

use crate::{StorageError, StorageResult};

fn corrupt(what: &str) -> StorageError {
    StorageError::Corrupt(format!("claim verification store: {what}"))
}

/// Mints the next numeric-suffixed identity inside one transaction, the
/// same allocation discipline as `observed_effects` (`effect-N`).
fn next_id(
    transaction: &Connection,
    table: &str,
    column: &str,
    prefix: &str,
) -> StorageResult<String> {
    let sql = format!(
        "SELECT COALESCE(MAX(CAST(substr({column}, {}) AS INTEGER)), 0) + 1
         FROM {table} WHERE {column} GLOB '{prefix}-[0-9]*'",
        prefix.len() + 2
    );
    let next: i64 = transaction.query_row(&sql, [], |row| row.get(0))?;
    Ok(format!("{prefix}-{}", next.max(1)))
}

/// A durable completion-admission record: the typed verdict the trusted
/// boundary committed with a `VERIFYING → COMPLETED` transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionAdmissionRecord {
    /// Durable identity (`admission-N`).
    pub admission_id: String,
    /// The completed task.
    pub task_id: String,
    /// Policy fingerprint the admission ran under (CV-7).
    pub policy_fingerprint: Option<String>,
    /// Typed per-criterion outcomes with evidence references.
    pub outcomes: Vec<CriterionOutcome>,
    /// RFC3339 decision timestamp.
    pub decided_at: String,
}

type ClaimRow = (
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
);

fn claim_from_row(row: ClaimRow) -> Result<AgentClaim, evaluation::EffectError> {
    let (
        claim_id,
        criterion_id,
        outcome,
        subject_ref,
        fingerprint,
        receipts,
        effects,
        sessions,
        asserted_by,
        asserted_at,
    ) = row;
    AgentClaim::new(
        ClaimId::new(claim_id)?,
        criterion_id,
        ClaimedOutcome::parse(&outcome)?,
        ClaimedSubject {
            subject_ref,
            subject_fingerprint: SubjectFingerprint::new(fingerprint)?,
        },
        EvidenceRefs::decode(&receipts, &effects, &sessions)?,
        asserted_by,
        asserted_at,
    )
}

impl crate::SqliteStore {
    /// Persists one agent claim. Claims are audit data, never authority
    /// (CV-1, INV-146); identity allocation and insert share one IMMEDIATE
    /// transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn append_agent_claim(
        &mut self,
        task_id: &TaskId,
        criterion_id: &str,
        claimed_outcome: ClaimedOutcome,
        claimed_subject: ClaimedSubject,
        claimed_evidence: EvidenceRefs,
        asserted_by: &str,
        asserted_at: &str,
    ) -> StorageResult<AgentClaim> {
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task_exists: Option<String> = transaction
            .query_row(
                "SELECT id FROM tasks WHERE id = ?1",
                params![task_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if task_exists.is_none() {
            return Err(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )));
        }
        let claim_id = next_id(&transaction, "agent_claims", "claim_id", "claim")?;
        let claim = AgentClaim::new(
            ClaimId::new(&claim_id).map_err(|error| corrupt(&error.to_string()))?,
            criterion_id,
            claimed_outcome,
            claimed_subject,
            claimed_evidence,
            asserted_by,
            asserted_at,
        )
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO agent_claims (
                    claim_id, task_id, criterion_id, claimed_outcome,
                    subject_ref, subject_fingerprint, receipt_refs,
                    effect_refs, session_refs, asserted_by, asserted_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    claim.claim_id.as_str(),
                    task_id.as_str(),
                    claim.criterion_id,
                    claim.claimed_outcome.as_str(),
                    claim.claimed_subject.subject_ref,
                    claim.claimed_subject.subject_fingerprint.as_str(),
                    claim.claimed_evidence.encode_receipts(),
                    claim.claimed_evidence.encode_effects(),
                    claim.claimed_evidence.encode_sessions(),
                    claim.asserted_by,
                    claim.asserted_at,
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "store agent claim"))?;
        transaction.commit()?;
        Ok(claim)
    }

    /// Lists a task's durable claims in insertion order.
    pub fn list_agent_claims_for_task(&self, task_id: &TaskId) -> StorageResult<Vec<AgentClaim>> {
        let mut statement = self.connection().prepare(
            "SELECT claim_id, criterion_id, claimed_outcome, subject_ref,
                    subject_fingerprint, receipt_refs, effect_refs,
                    session_refs, asserted_by, asserted_at
             FROM agent_claims WHERE task_id = ?1 ORDER BY rowid",
        )?;
        let rows = statement
            .query_map(params![task_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| claim_from_row(row).map_err(|error| corrupt(&error.to_string())))
            .collect()
    }

    /// Persists one trusted claim verdict. The verifier identity is the
    /// trusted constant — callers cannot supply another (CV-2); the schema
    /// repeats the constraint. `supersedes` chains the new verdict to the
    /// current one it replaces (same claim, still unsuperseded) inside one
    /// transaction; nothing is ever deleted (CV-5).
    #[allow(clippy::too_many_arguments)]
    pub fn append_claim_verification(
        &mut self,
        claim_id: &ClaimId,
        requirement_kind: RequirementKind,
        subject_fingerprint: SubjectFingerprint,
        requirement_revision: String,
        policy_fingerprint: Option<String>,
        verdict: ClaimVerdict,
        basis: EvidenceRefs,
        supersedes: Option<VerificationId>,
        decided_at: &str,
    ) -> StorageResult<ClaimVerification> {
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claim_exists: Option<String> = transaction
            .query_row(
                "SELECT claim_id FROM agent_claims WHERE claim_id = ?1",
                params![claim_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if claim_exists.is_none() {
            return Err(StorageError::NotFound(format!(
                "claim {claim_id} does not exist"
            )));
        }
        let verification_id = next_id(
            &transaction,
            "claim_verifications",
            "verification_id",
            "verification",
        )?;
        let verification = ClaimVerification::new(
            VerificationId::new(&verification_id).map_err(|error| corrupt(&error.to_string()))?,
            claim_id.clone(),
            evaluation::TRUSTED_VERIFIER_IDENTITY,
            requirement_kind,
            subject_fingerprint,
            requirement_revision,
            policy_fingerprint,
            verdict,
            basis,
            None,
            decided_at,
        )
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO claim_verifications (
                    verification_id, claim_id, verifier, requirement_kind,
                    subject_fingerprint, requirement_revision,
                    policy_fingerprint, verdict, receipt_refs, effect_refs,
                    session_refs, superseded_by, decided_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12)",
                params![
                    verification.verification_id.as_str(),
                    verification.claim_id.as_str(),
                    verification.verifier,
                    verification.requirement_kind.as_str(),
                    verification.subject_fingerprint.as_str(),
                    verification.requirement_revision,
                    verification.policy_fingerprint,
                    verification.verdict.as_str(),
                    verification.basis.encode_receipts(),
                    verification.basis.encode_effects(),
                    verification.basis.encode_sessions(),
                    verification.decided_at,
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "store claim verification"))?;
        // Supersede the previous verdict only after the replacement row
        // exists: `superseded_by` is a self-referencing FK, so the chain
        // must point at an already-inserted verdict. A guard failure rolls
        // the whole transaction back, including the insert.
        if let Some(old) = &supersedes {
            let updated = transaction.execute(
                "UPDATE claim_verifications SET superseded_by = ?1
                 WHERE verification_id = ?2 AND claim_id = ?3 AND superseded_by IS NULL",
                params![
                    verification.verification_id.as_str(),
                    old.as_str(),
                    claim_id.as_str()
                ],
            )?;
            if updated != 1 {
                return Err(StorageError::ConstraintViolation(format!(
                    "cannot supersede verdict {old}: missing, foreign claim, or already superseded"
                )));
            }
        }
        transaction.commit()?;
        Ok(verification)
    }

    /// Lists every verdict on a task's claims (superseded included, CV-5)
    /// in insertion order.
    pub fn list_claim_verifications_for_task(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Vec<ClaimVerification>> {
        let mut statement = self.connection().prepare(
            "SELECT v.verification_id, v.claim_id, v.verifier,
                    v.requirement_kind, v.subject_fingerprint,
                    v.requirement_revision, v.policy_fingerprint, v.verdict,
                    v.receipt_refs, v.effect_refs, v.session_refs,
                    v.superseded_by, v.decided_at
             FROM claim_verifications v
             JOIN agent_claims c ON c.claim_id = v.claim_id
             WHERE c.task_id = ?1 ORDER BY v.rowid",
        )?;
        let rows = statement
            .query_map(params![task_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, String>(12)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| {
                let (
                    verification_id,
                    claim_id,
                    verifier,
                    kind,
                    fingerprint,
                    requirement_revision,
                    policy_fingerprint,
                    verdict,
                    receipts,
                    effects,
                    sessions,
                    superseded_by,
                    decided_at,
                ) = row;
                (|| {
                    let superseded_by = superseded_by
                        .map(VerificationId::new)
                        .transpose()
                        .map_err(|error| corrupt(&error.to_string()))?;
                    ClaimVerification::new(
                        VerificationId::new(verification_id)
                            .map_err(|error| corrupt(&error.to_string()))?,
                        ClaimId::new(claim_id).map_err(|error| corrupt(&error.to_string()))?,
                        verifier,
                        RequirementKind::parse(&kind)
                            .map_err(|error| corrupt(&error.to_string()))?,
                        SubjectFingerprint::new(fingerprint)
                            .map_err(|error| corrupt(&error.to_string()))?,
                        requirement_revision,
                        policy_fingerprint,
                        ClaimVerdict::parse(&verdict)
                            .map_err(|error| corrupt(&error.to_string()))?,
                        EvidenceRefs::decode(&receipts, &effects, &sessions)
                            .map_err(|error| corrupt(&error.to_string()))?,
                        superseded_by,
                        decided_at,
                    )
                    .map_err(|error| corrupt(&error.to_string()))
                })()
            })
            .collect()
    }

    /// Persists one policy exemption (CV-4). `risk` arrives already typed;
    /// the schema refuses anything but `low` fail-closed.
    pub fn append_policy_exemption(
        &mut self,
        task_id: &TaskId,
        criterion_id: Option<&str>,
        subject_fingerprint: SubjectFingerprint,
        policy_fingerprint: &str,
        risk: plan_domain::RiskClass,
        created_at: &str,
    ) -> StorageResult<PolicyExemption> {
        if risk != plan_domain::RiskClass::Low {
            return Err(StorageError::ConstraintViolation(
                "policy exemption requires a typed low risk class".to_owned(),
            ));
        }
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exemption_id = next_id(
            &transaction,
            "policy_exemptions",
            "exemption_id",
            "exemption",
        )?;
        let exemption = PolicyExemption::new(
            ExemptionId::new(&exemption_id).map_err(|error| corrupt(&error.to_string()))?,
            criterion_id.map(str::to_owned),
            subject_fingerprint,
            policy_fingerprint,
            created_at,
        )
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO policy_exemptions (
                    exemption_id, task_id, criterion_id, subject_fingerprint,
                    policy_fingerprint, risk_class, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    exemption.exemption_id.as_str(),
                    task_id.as_str(),
                    exemption.criterion_id,
                    exemption.subject_fingerprint.as_str(),
                    exemption.policy_fingerprint,
                    risk.as_str(),
                    exemption.created_at,
                ],
            )
            .map_err(|error| crate::error::map_constraint(error, "store policy exemption"))?;
        transaction.commit()?;
        Ok(exemption)
    }

    /// Lists a task's durable policy exemptions in insertion order.
    pub fn list_policy_exemptions_for_task(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Vec<PolicyExemption>> {
        let mut statement = self.connection().prepare(
            "SELECT exemption_id, criterion_id, subject_fingerprint,
                    policy_fingerprint, created_at
             FROM policy_exemptions WHERE task_id = ?1 ORDER BY rowid",
        )?;
        let rows = statement
            .query_map(params![task_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, criterion, fingerprint, policy, created_at)| {
                (|| {
                    PolicyExemption::new(
                        ExemptionId::new(id).map_err(|error| corrupt(&error.to_string()))?,
                        criterion,
                        SubjectFingerprint::new(fingerprint)
                            .map_err(|error| corrupt(&error.to_string()))?,
                        policy,
                        created_at,
                    )
                    .map_err(|error| corrupt(&error.to_string()))
                })()
            })
            .collect()
    }

    /// Persists one human-approval discharge (CV-8). Single-use is guarded
    /// by the completion commit; this insert only records the bound,
    /// windowed, unconsumed discharge.
    #[allow(clippy::too_many_arguments)]
    pub fn append_approval_discharge(
        &mut self,
        task_id: &TaskId,
        criterion_id: &str,
        fp: SubjectFingerprint,
        risk: plan_domain::RiskClass,
        approval_ref: &str,
        nvb: &str,
        nva: &str,
        created_at: &str,
    ) -> StorageResult<evaluation::ApprovalDischarge> {
        use evaluation::{ApprovalDischarge, DischargeId};
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<String> = tx
            .query_row(
                "SELECT id FROM tasks WHERE id = ?1",
                params![task_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if found.is_none() {
            return Err(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )));
        }
        let did = next_id(&tx, "approval_discharges", "discharge_id", "discharge")?;
        let d = ApprovalDischarge::new(
            DischargeId::new(&did).map_err(|e| corrupt(&e.to_string()))?,
            criterion_id,
            fp,
            risk.as_str(),
            approval_ref,
            nvb,
            nva,
            None,
            created_at,
        )
        .map_err(|e| StorageError::ConstraintViolation(e.to_string()))?;
        if d.not_valid_before >= d.not_valid_after {
            return Err(StorageError::ConstraintViolation(
                "approval discharge window is empty".to_owned(),
            ));
        }
        tx.execute(
            "INSERT INTO approval_discharges (discharge_id, task_id, criterion_id,
              subject_fingerprint, risk_class, approval_ref, not_valid_before,
              not_valid_after, consumed_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9)",
            params![
                d.discharge_id.as_str(),
                task_id.as_str(),
                d.criterion_id,
                d.subject_fingerprint.as_str(),
                d.risk,
                d.approval_ref,
                d.not_valid_before,
                d.not_valid_after,
                d.created_at,
            ],
        )
        .map_err(|e| crate::error::map_constraint(e, "store approval discharge"))?;
        tx.commit()?;
        Ok(d)
    }

    /// Lists a task's durable approval discharges in insertion order.
    pub fn list_approval_discharges_for_task(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Vec<evaluation::ApprovalDischarge>> {
        use evaluation::{AdmissionId, ApprovalDischarge, DischargeId};
        let mut stmt = self.connection().prepare(
            "SELECT discharge_id, criterion_id, subject_fingerprint, risk_class,
                    approval_ref, not_valid_before, not_valid_after,
                    consumed_by, created_at
             FROM approval_discharges WHERE task_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map(params![task_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, crit, fp, risk, aref, nb, na, consumed, created)| {
                (|| {
                    let consumed = consumed
                        .map(AdmissionId::new)
                        .transpose()
                        .map_err(|e| corrupt(&e.to_string()))?;
                    ApprovalDischarge::new(
                        DischargeId::new(id).map_err(|e| corrupt(&e.to_string()))?,
                        crit,
                        SubjectFingerprint::new(fp).map_err(|e| corrupt(&e.to_string()))?,
                        risk,
                        aref,
                        nb,
                        na,
                        consumed,
                        created,
                    )
                    .map_err(|e| corrupt(&e.to_string()))
                })()
            })
            .collect()
    }

    /// Latest trusted subject fingerprint observed for a task, if any.
    pub fn latest_observed_subject_fingerprint_for_task(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Option<SubjectFingerprint>> {
        let fp: Option<String> = self
            .connection()
            .query_row(
                "SELECT o.subject_fingerprint FROM observed_effects o
                 JOIN executions e ON e.id = o.execution_id
                 WHERE e.task_id = ?1 ORDER BY o.rowid DESC LIMIT 1",
                params![task_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        fp.map(SubjectFingerprint::new)
            .transpose()
            .map_err(|e| corrupt(&e.to_string()))
    }
    /// Atomically commits one admitted completion: guarded state write +
    /// journal record + completion-admission row, consuming exactly the
    /// discharges the report names, in one SQLite transaction. Refusals
    /// never reach this boundary (nothing is written for them).
    #[allow(clippy::too_many_arguments)]
    pub fn admit_completion(
        &mut self,
        task_id: &TaskId,
        expected: task_domain::TaskState,
        next: &task_domain::Task,
        event: &crate::NewJournalEvent,
        policy_fp: Option<&str>,
        outcomes: &[CriterionOutcome],
        consumed: &[evaluation::DischargeId],
        decided_at: &str,
    ) -> StorageResult<(crate::JournalEvent, CompletionAdmissionRecord)> {
        use crate::journal_store::{insert_journal_event, require_envelope};
        use evaluation::AdmissionId;
        if next.id() != task_id {
            return Err(StorageError::ConstraintViolation(format!(
                "lifecycle write targets task {task_id} but carries task {}",
                next.id()
            )));
        }
        if event.task_id.as_ref() != Some(task_id)
            || event.project_id.as_ref() != Some(next.project_id())
        {
            return Err(StorageError::ConstraintViolation(
                "lifecycle event references a different task".to_owned(),
            ));
        }
        if event.from_state != Some(expected) || event.to_state != Some(next.state()) {
            return Err(StorageError::ConstraintViolation(
                "lifecycle event states do not match the guarded write".to_owned(),
            ));
        }
        require_envelope(event)?;
        if decided_at.trim().is_empty()
            || decided_at.chars().any(char::is_control)
            || decided_at.len() > 1024
        {
            return Err(StorageError::ConstraintViolation(
                "admission timestamp is invalid".to_owned(),
            ));
        }
        let encoded = {
            let tmp = AdmissionReport {
                decision: evaluation::AdmissionDecision::Admit,
                outcomes: outcomes.to_vec(),
            };
            tmp.encode_outcomes()
        };
        if encoded.len() > 16384 {
            return Err(StorageError::ConstraintViolation(
                "admission outcomes exceed the durable bound".to_owned(),
            ));
        }
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE tasks SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![next.state().as_str(), task_id.as_str(), expected.as_str()],
        )?;
        if updated == 0 {
            let cur: Option<String> = tx
                .query_row(
                    "SELECT state FROM tasks WHERE id = ?1",
                    params![task_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            return Err(match cur {
                None => StorageError::NotFound(format!("task {task_id} does not exist")),
                Some(found) => StorageError::Stale {
                    task: task_id.to_string(),
                    expected: expected.as_str().to_owned(),
                    found,
                },
            });
        }
        let committed = insert_journal_event(&tx, event)?;
        let aid = next_id(&tx, "completion_admissions", "admission_id", "admission")?;
        AdmissionId::new(&aid).map_err(|e| corrupt(&e.to_string()))?;
        tx.execute(
            "INSERT INTO completion_admissions
             (admission_id, task_id, decision, policy_fingerprint, outcomes, decided_at)
             VALUES (?1, ?2, 'ADMIT', ?3, ?4, ?5)",
            params![aid, task_id.as_str(), policy_fp, encoded, decided_at,],
        )
        .map_err(|e| crate::error::map_constraint(e, "store completion admission"))?;
        for d in consumed {
            let n = tx.execute(
                "UPDATE approval_discharges SET consumed_by = ?1
                 WHERE discharge_id = ?2 AND task_id = ?3 AND consumed_by IS NULL",
                params![aid, d.as_str(), task_id.as_str()],
            )?;
            if n != 1 {
                return Err(StorageError::ConstraintViolation(format!(
                    "discharge {} missing, foreign, or consumed",
                    d.as_str()
                )));
            }
        }
        tx.commit()?;
        let record = CompletionAdmissionRecord {
            admission_id: aid,
            task_id: task_id.as_str().to_owned(),
            policy_fingerprint: policy_fp.map(str::to_owned),
            outcomes: outcomes.to_vec(),
            decided_at: decided_at.to_owned(),
        };
        Ok((committed, record))
    }
    /// Reads one completion-admission record by identity, or `None`.
    pub fn get_completion_admission(
        &self,
        admission_id: &str,
    ) -> StorageResult<Option<CompletionAdmissionRecord>> {
        let row: Option<(String, String, Option<String>, String, String)> = self
            .connection()
            .query_row(
                "SELECT admission_id, task_id, policy_fingerprint, outcomes, decided_at
                 FROM completion_admissions WHERE admission_id = ?1",
                params![admission_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(admission_id, task_id, policy_fingerprint, outcomes, decided_at)| {
                (|| {
                    let outcomes = AdmissionReport::decode_outcomes(&outcomes)
                        .map_err(|e| corrupt(&e.to_string()))?;
                    Ok(CompletionAdmissionRecord {
                        admission_id,
                        task_id,
                        policy_fingerprint,
                        outcomes,
                        decided_at,
                    })
                })()
                .map_err(|e: StorageError| e)
            },
        )
        .transpose()
    }
    /// Lists a task's completion-admission records in commit order.
    pub fn list_completion_admissions_for_task(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Vec<CompletionAdmissionRecord>> {
        let mut stmt = self.connection().prepare(
            "SELECT admission_id, task_id, policy_fingerprint, outcomes, decided_at
             FROM completion_admissions WHERE task_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map(params![task_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(admission_id, task_id, policy_fingerprint, outcomes, decided_at)| {
                    (|| {
                        let outcomes = AdmissionReport::decode_outcomes(&outcomes)
                            .map_err(|e| corrupt(&e.to_string()))?;
                        Ok(CompletionAdmissionRecord {
                            admission_id,
                            task_id,
                            policy_fingerprint,
                            outcomes,
                            decided_at,
                        })
                    })()
                    .map_err(|e: StorageError| e)
                },
            )
            .collect()
    }

    // __ADMIT__
}
