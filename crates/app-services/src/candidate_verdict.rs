//! Candidate-only verdict minting (TASK-0125 phase c).
//!
//! The sole production path that mints durable `AgentClaim` /
//! `ClaimVerification` rows. Called only from the independent-evaluation
//! `Verified` branch of the shared orchestration concluding boundary,
//! after freshness revalidation and before completion admission. The
//! operator dispatch path never calls this: it only attempts admission
//! against already-durable rows.
//!
//! The mint never synthesizes measurement. It binds verdicts exclusively
//! to observed effects the capture pipeline already recorded for the
//! evaluated attempt, under the current trusted subject fingerprint.
//! With no bound evidence it succeeds vacuously with zero rows — missing
//! measurement is not an error here; the subsequent admission refusal is
//! the loud signal, owned by admission alone. Storage failures and corrupt
//! lineage propagate as errors, and the task stays `VERIFYING`.
//!
//! `HUMAN_APPROVAL` criteria are never minted: they discharge only via a
//! durable approval (CV-8). Criteria that already hold a current
//! `VERIFIED` verdict are skipped, so a retried cycle converges instead
//! of stacking duplicate rows.

use agent_runtime::{AttemptId, ExecutionId};
use evaluation::{
    requirement_revision, ClaimVerdict, ClaimVerification, ClaimedOutcome, ClaimedSubject,
    EvidenceRefs, TRUSTED_VERIFIER_IDENTITY,
};
use storage_sqlite::{SqliteStore, StorageError};
use task_domain::{RequirementKind, TaskId};

use crate::{ServiceError, ServiceResult};

/// Mints one durable claim plus one `VERIFIED` verdict per required
/// non-`HUMAN_APPROVAL` criterion that lacks a current `VERIFIED` verdict,
/// bound to the attempt's current-subject observed effects. Returns the
/// minted verdicts (empty when there is nothing mintable).
pub fn mint_candidate_verdicts(
    store: &mut SqliteStore,
    task_id: &TaskId,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    asserter: &str,
    now: &str,
) -> ServiceResult<Vec<ClaimVerification>> {
    if asserter.trim().is_empty() || asserter == TRUSTED_VERIFIER_IDENTITY {
        return Err(ServiceError::Storage(StorageError::ConstraintViolation(
            "candidate asserter must be a non-empty candidate identity, never the trusted verifier"
                .to_owned(),
        )));
    }
    let task = store.get_task(task_id)?.ok_or_else(|| {
        ServiceError::Storage(StorageError::NotFound(format!(
            "task {task_id} does not exist"
        )))
    })?;
    let mut pending: Vec<(String, RequirementKind, Option<String>, String)> = Vec::new();
    for criterion in task.acceptance() {
        let requirement = task
            .evidence_requirements()
            .iter()
            .find(|requirement| requirement.criterion_id == *criterion)
            .ok_or_else(|| {
                ServiceError::Storage(StorageError::Corrupt(format!(
                    "task {task_id} has no requirement for {criterion}"
                )))
            })?;
        if requirement.requirement_kind == RequirementKind::HumanApproval {
            continue;
        }
        pending.push((
            criterion.clone(),
            requirement.requirement_kind,
            requirement.policy_fingerprint.clone(),
            requirement_revision(requirement),
        ));
    }
    if pending.is_empty() {
        return Ok(Vec::new());
    }
    let Some(current) = store.latest_observed_subject_fingerprint_for_task(task_id)? else {
        return Ok(Vec::new());
    };
    let mut bound: Vec<String> = Vec::new();
    for effect in store.list_observed_effects_for_attempt(execution_id, attempt_id)? {
        let owned = store
            .get_execution(&effect.provenance().execution_id)?
            .is_some_and(|execution| execution.task_id() == task_id);
        if owned && effect.subject_fingerprint() == &current {
            bound.push(effect.effect_id().as_str().to_owned());
        }
    }
    if bound.is_empty() {
        return Ok(Vec::new());
    }
    let claims = store.list_agent_claims_for_task(task_id)?;
    let verdicts = store.list_claim_verifications_for_task(task_id)?;
    let already_verified = |criterion: &str, kind: &RequirementKind, revision: &str| {
        verdicts.iter().any(|verdict| {
            verdict.verdict == ClaimVerdict::Verified
                && verdict.superseded_by.is_none()
                && verdict.requirement_kind == *kind
                && verdict.subject_fingerprint == current
                && verdict.requirement_revision == revision
                && claims.iter().any(|claim| {
                    claim.claim_id == verdict.claim_id
                        && claim.criterion_id == criterion
                        && claim.asserted_by != verdict.verifier
                })
        })
    };
    let basis = EvidenceRefs::new(Vec::new(), bound, Vec::new()).map_err(|error| {
        ServiceError::Storage(StorageError::ConstraintViolation(error.to_string()))
    })?;
    let mut minted = Vec::new();
    for (criterion, kind, policy, revision) in &pending {
        if already_verified(criterion, kind, revision) {
            continue;
        }
        let claim = store.append_agent_claim(
            task_id,
            criterion,
            ClaimedOutcome::Satisfied,
            ClaimedSubject {
                subject_ref: format!("subject:{criterion}"),
                subject_fingerprint: current.clone(),
            },
            EvidenceRefs::default(),
            asserter,
            now,
        )?;
        let verdict = store.append_claim_verification(
            &claim.claim_id,
            *kind,
            current.clone(),
            revision.clone(),
            policy.clone(),
            ClaimVerdict::Verified,
            basis.clone(),
            None,
            now,
        )?;
        minted.push(verdict);
    }
    Ok(minted)
}
