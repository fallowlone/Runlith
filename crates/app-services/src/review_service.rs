//! ADR-0033 review authority service (TASK-0145).
//!
//! The trusted boundary around durable review state: every setting
//! change and every B1 review mints a fresh presence proof for its
//! exact operation, consumes the proof nonce once through the store,
//! and chains supersession onto the live head. Production holds
//! [`DenyPresence`], so all of this refuses until the trusted control
//! host exists (fail closed, no fallback).

use sha2::{Digest, Sha256};
use storage_sqlite::{SqliteStore, StorageError};

use crate::clock::{Clock, SystemClock};
use crate::presence::{FreshPresenceProof, PresenceAuthority, PresenceError};
use crate::review_admission::{
    decide_review_admission, ReviewAdmissionDecision, ReviewAdmissionInput,
};

/// How long a minted presence proof stays consumable (5 minutes).
/// Proofs from the future always refuse.
pub const PRESENCE_FRESHNESS_MS: u64 = 300_000;

/// Caller-owned values for one B1 review. The service fills the
/// current setting binding, the minted proof nonce, and the capture
/// timestamp; `supersedes: None` auto-chains to the live head.
/// `reviewer_input_digest` must come from the trusted review UI (the
/// digest of the exact input the human judged), never from an IPC
/// caller or provider text; the service cannot re-verify its
/// provenance.
#[derive(Clone, Debug)]
pub struct B1ReviewInput {
    pub record_id: String,
    pub task_id: String,
    pub requirement_revision: String,
    pub base_manifest: String,
    pub candidate_manifest: String,
    pub changed_inputs: Vec<String>,
    pub affected_checks: Vec<String>,
    pub policy_revision: u64,
    pub policy_fingerprint: String,
    pub profile_id: String,
    pub profile_revision: u64,
    pub reviewer_input_digest: String,
    pub reviewer_verdict: evaluation::ReviewVerdictName,
    pub implementer_execution_id: String,
    pub implementer_attempt_id: String,
    pub implementer_admission_ref: String,
    pub rationale: String,
    pub evidence_refs: Vec<String>,
    pub supersedes: Option<String>,
}

/// Caller-owned values for one admission query. The service loads the
/// current setting and live head record, then decides.
#[derive(Clone, Debug)]
pub struct ReviewAdmissionQuery {
    pub task_id: String,
    pub policy: policy::TestMutationPolicyRevision,
    pub profile: policy::ReviewerProfile,
    pub path: String,
    pub check: String,
    pub requirement_revision: String,
    pub base_manifest: String,
    pub candidate_manifest: String,
    pub changed_inputs: Vec<String>,
    pub affected_checks: Vec<String>,
    pub max_age_ms: u64,
}

/// Review service failures. Absence (no setting, no record) is a
/// deny/block decision, never an error; errors mean the operation
/// itself cannot be trusted.
#[derive(Debug)]
pub enum ReviewServiceError {
    Presence(PresenceError),
    Storage(StorageError),
    StalePresence { proven_at_ms: u64, now_ms: u64 },
    TimestampOutOfRange,
    NoSettingForTask(String),
    SettingPathNotB1 { found: String },
    ParentNotLive(String),
    InvalidRecord(String),
    InvalidSetting(String),
}

impl std::fmt::Display for ReviewServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Presence(error) => write!(f, "presence refused: {error}"),
            Self::Storage(error) => write!(f, "review storage refused: {error}"),
            Self::StalePresence {
                proven_at_ms,
                now_ms,
            } => write!(
                f,
                "presence proof is stale (proven {proven_at_ms}, now {now_ms})"
            ),
            Self::TimestampOutOfRange => write!(f, "timestamp out of durable range"),
            Self::NoSettingForTask(task) => {
                write!(f, "no review path selected for task {task}")
            }
            Self::SettingPathNotB1 { found } => write!(
                f,
                "B1 review requires the B1 setting path (selected: {found})"
            ),
            Self::ParentNotLive(parent) => {
                write!(f, "superseded review is not live: {parent}")
            }
            Self::InvalidRecord(detail) => write!(f, "invalid review record: {detail}"),
            Self::InvalidSetting(detail) => write!(f, "invalid review setting: {detail}"),
        }
    }
}

impl std::error::Error for ReviewServiceError {}

impl From<StorageError> for ReviewServiceError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

pub type ReviewServiceResult<T> = Result<T, ReviewServiceError>;

/// Trusted review authority boundary over durable state.
pub struct ReviewService<C: Clock = SystemClock> {
    store: SqliteStore,
    clock: C,
}

impl<C: Clock> ReviewService<C> {
    pub fn attach(store: SqliteStore, clock: C) -> Self {
        Self { store, clock }
    }

    /// Exact operation string a setting-change proof binds. Mirrored
    /// by migration 0025 (`review_path_setting_proof_operation`); the
    /// service integration tests run both sides, so drift refuses.
    pub fn setting_change_operation(revision: u64, path: &policy::ReviewPath) -> String {
        format!(
            "review-setting-change:rev={revision}:path={}",
            path.serialize()
        )
    }

    /// Exact operation string a B1 review proof binds. Mirrored by
    /// migration 0025 (`review_record_proof_operation`).
    pub fn b1_review_operation(record_id: &str) -> String {
        format!("b1-review:record={record_id}")
    }

    fn consume_proof(&mut self, proof: &FreshPresenceProof) -> ReviewServiceResult<()> {
        let now = self.clock.now_millis();
        if proof.proven_at_ms() > now || now - proof.proven_at_ms() > PRESENCE_FRESHNESS_MS {
            return Err(ReviewServiceError::StalePresence {
                proven_at_ms: proof.proven_at_ms(),
                now_ms: now,
            });
        }
        let proven_at = i64::try_from(proof.proven_at_ms())
            .map_err(|_| ReviewServiceError::TimestampOutOfRange)?;
        self.store.record_presence_proof(
            proof.nonce(),
            proof.operation(),
            proof.input_digest(),
            proven_at,
        )?;
        Ok(())
    }

    /// Appends the next review-path setting revision for a task. The
    /// proof binds this exact change (revision + path); a denied or
    /// stale presence refuses before anything is stored. A switch
    /// invalidates the live head review, if any: prior reviews bind
    /// the old revision and must never admit under the new one.
    pub fn change_review_path(
        &mut self,
        presence: &dyn PresenceAuthority,
        task_id: &str,
        path: policy::ReviewPath,
    ) -> ReviewServiceResult<policy::ReviewPathSetting> {
        let revision = self
            .store
            .get_review_path_setting(task_id)?
            .map(|current| current.revision() + 1)
            .unwrap_or(1);
        let operation = Self::setting_change_operation(revision, &path);
        let input_digest = setting_input_digest(task_id, &operation);
        let proof = presence
            .prove(&operation, &input_digest)
            .map_err(ReviewServiceError::Presence)?;
        self.consume_proof(&proof)?;
        let setting = policy::ReviewPathSetting::new(revision, path)
            .map_err(|error| ReviewServiceError::InvalidSetting(error.to_string()))?;
        let now = i64::try_from(self.clock.now_millis())
            .map_err(|_| ReviewServiceError::TimestampOutOfRange)?;
        self.store
            .record_review_path_setting(task_id, &setting, proof.nonce(), now)?;
        if let Some(head) = self.store.live_review_record_head(task_id)? {
            self.store
                .invalidate_review_record(head.record_id(), "review-setting-changed", now)?;
        }
        Ok(setting)
    }

    /// Records one B1 review under the task's current setting. Refuses
    /// without a setting, under a B2 setting, on denied/stale
    /// presence, and when the explicit parent is not live. `None`
    /// chains to the live head when one exists.
    pub fn record_b1_review(
        &mut self,
        presence: &dyn PresenceAuthority,
        input: B1ReviewInput,
    ) -> ReviewServiceResult<evaluation::ReviewRecord> {
        let setting = self
            .store
            .get_review_path_setting(&input.task_id)?
            .ok_or_else(|| ReviewServiceError::NoSettingForTask(input.task_id.clone()))?;
        if *setting.path() != policy::ReviewPath::B1 {
            return Err(ReviewServiceError::SettingPathNotB1 {
                found: setting.path().serialize(),
            });
        }
        let supersedes = match input.supersedes.clone() {
            Some(parent) => {
                let live = self.store.live_review_record_head(&input.task_id)?;
                if live.as_ref().is_some_and(|head| head.record_id() == parent) {
                    Some(parent)
                } else {
                    return Err(ReviewServiceError::ParentNotLive(parent));
                }
            }
            None => self
                .store
                .live_review_record_head(&input.task_id)?
                .map(|head| head.record_id().to_owned()),
        };
        let operation = Self::b1_review_operation(&input.record_id);
        let proof = presence
            .prove(&operation, &input.reviewer_input_digest)
            .map_err(ReviewServiceError::Presence)?;
        self.consume_proof(&proof)?;
        // Typed actors from independently proven values: the reviewer
        // from the minted presence proof (never caller text) and the
        // implementer from product refs whose execution/attempt rows
        // the store FKs verify. Distinct types (human/v1 vs
        // implementation/v1) make reviewer == implementer
        // unrepresentable in a B1 pair. The admission ref is bound
        // opaquely: no durable runner-admission registry exists yet
        // to cross-verify it against.
        let reviewer = policy::HumanPrincipal::new(
            proof.nonce(),
            proof.input_digest(),
            match input.reviewer_verdict {
                evaluation::ReviewVerdictName::Approved => policy::ReviewVerdict::Approved,
                evaluation::ReviewVerdictName::Rejected => policy::ReviewVerdict::Rejected,
            },
        )
        .map_err(|error| ReviewServiceError::InvalidRecord(error.to_string()))?;
        let implementer = policy::ImplementationPrincipal::new(
            &input.implementer_execution_id,
            &input.implementer_attempt_id,
            &input.implementer_admission_ref,
        )
        .map_err(|error| ReviewServiceError::InvalidRecord(error.to_string()))?;
        debug_assert_ne!(reviewer.namespace(), implementer.namespace());
        let captured_at_ms = self.clock.now_millis();
        let fields = evaluation::ReviewRecordFields {
            record_id: input.record_id,
            task_id: input.task_id,
            requirement_revision: input.requirement_revision,
            base_manifest: input.base_manifest,
            candidate_manifest: input.candidate_manifest,
            changed_inputs: input.changed_inputs,
            affected_checks: input.affected_checks,
            policy_revision: input.policy_revision,
            policy_fingerprint: input.policy_fingerprint,
            profile_id: input.profile_id,
            profile_revision: input.profile_revision,
            setting_revision: setting.revision(),
            setting_path: evaluation::ReviewPathName::B1,
            reviewer_nonce: proof.nonce().to_owned(),
            reviewer_input_digest: input.reviewer_input_digest,
            reviewer_verdict: input.reviewer_verdict,
            implementer_execution_id: input.implementer_execution_id,
            implementer_attempt_id: input.implementer_attempt_id,
            implementer_admission_ref: input.implementer_admission_ref,
            rationale: input.rationale,
            evidence_refs: input.evidence_refs,
            captured_at_ms,
            supersedes,
        };
        let record = evaluation::ReviewRecord::new(fields)
            .map_err(|error| ReviewServiceError::InvalidRecord(error.to_string()))?;
        self.store.record_review(&record)?;
        Ok(record)
    }

    /// Decides admission for a task from durable state: current
    /// setting plus live head record. No setting denies; no record
    /// blocks on review.
    pub fn admission_for_task(
        &self,
        query: ReviewAdmissionQuery,
    ) -> ReviewServiceResult<ReviewAdmissionDecision> {
        let Some(setting) = self.store.get_review_path_setting(&query.task_id)? else {
            return Ok(ReviewAdmissionDecision::Deny {
                reason: "no review path selected",
            });
        };
        let record = self.store.live_review_record_head(&query.task_id)?;
        Ok(decide_review_admission(ReviewAdmissionInput {
            policy: query.policy,
            setting,
            profile: query.profile,
            path: query.path,
            check: query.check,
            record,
            task_id: query.task_id,
            requirement_revision: query.requirement_revision,
            base_manifest: query.base_manifest,
            candidate_manifest: query.candidate_manifest,
            changed_inputs: query.changed_inputs,
            affected_checks: query.affected_checks,
            now_ms: self.clock.now_millis(),
            max_age_ms: query.max_age_ms,
        }))
    }

    /// Marks one review record invalidated (sticky; identical retries
    /// are harmless no-ops).
    pub fn invalidate_review(
        &mut self,
        record_id: &str,
        invalidation_code: &str,
    ) -> ReviewServiceResult<()> {
        let now = i64::try_from(self.clock.now_millis())
            .map_err(|_| ReviewServiceError::TimestampOutOfRange)?;
        self.store
            .invalidate_review_record(record_id, invalidation_code, now)?;
        Ok(())
    }
}

fn setting_input_digest(task_id: &str, operation: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"review-setting-change-input/v1\0");
    hash.update(task_id.as_bytes());
    hash.update(b"\0");
    hash.update(operation.as_bytes());
    format!("sha256:{:x}", hash.finalize())
}
