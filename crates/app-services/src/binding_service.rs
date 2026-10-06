//! ADR-0036 operator binding control-surface path (TASK-0160 IMPL-C).
//!
//! The sole presence-authorized binding path: the service mints the
//! proof for the exact binding operation through a
//! [`PresenceAuthority`], binds one DRAFT criterion through the §1a
//! boundary, and persists the task image with the proof consumption
//! atomically. Production holds [`DenyPresence`], so operator binding
//! is impossible on every path — including IPC, which carries no
//! binding fields and no proof minter — until the trusted control
//! host exists. Timing is Option 2a only: binding lands at task
//! creation (DRAFT), never as a later update.

use policy::CandidateSelectorCatalogRevision;
use storage_sqlite::{BindingProofConsumption, SqliteStore, StorageError};
use task_domain::{TaskId, TaskState};

use crate::clock::{Clock, SystemClock};
use crate::presence::{PresenceAuthority, PresenceError, PRESENCE_FRESHNESS_MS};
use crate::requirement_binding::{
    bind_candidate_input_params, binding_input_digest, binding_operation, BoundEvidenceRequirement,
    ProposedCandidateBinding, RequirementBindingError,
};

/// Caller-owned values for one operator binding.
#[derive(Clone, Debug)]
pub struct OperatorBindingInput {
    pub task_id: String,
    pub criterion_id: String,
    pub selector: String,
    pub path: String,
    pub literal: String,
    pub policy_fingerprint: String,
}

/// Binding service failures. Every refusal leaves the task exactly
/// as it was: no params, no consumption.
#[derive(Debug)]
pub enum BindingServiceError {
    Presence(PresenceError),
    Storage(StorageError),
    StalePresence { proven_at_ms: u64, now_ms: u64 },
    TimestampOutOfRange,
    TaskNotFound(String),
    TaskNotDraft { task: String, state: TaskState },
    CriterionNotFound(String),
    Binding(RequirementBindingError),
}

impl std::fmt::Display for BindingServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Presence(error) => write!(f, "presence refused: {error}"),
            Self::Storage(error) => write!(f, "binding storage refused: {error}"),
            Self::StalePresence {
                proven_at_ms,
                now_ms,
            } => write!(
                f,
                "presence proof is stale (proven {proven_at_ms}, now {now_ms})"
            ),
            Self::TimestampOutOfRange => write!(f, "timestamp out of durable range"),
            Self::TaskNotFound(task) => write!(f, "task not found: {task}"),
            Self::TaskNotDraft { task, state } => write!(
                f,
                "binding lands at creation only (task {task} is {state:?})"
            ),
            Self::CriterionNotFound(criterion) => {
                write!(f, "criterion not found: {criterion}")
            }
            Self::Binding(error) => write!(f, "binding refused: {error}"),
        }
    }
}

impl std::error::Error for BindingServiceError {}

impl From<StorageError> for BindingServiceError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

pub type BindingServiceResult<T> = Result<T, BindingServiceError>;

/// Trusted operator binding boundary over durable state.
pub struct BindingService<C: Clock = SystemClock> {
    store: SqliteStore,
    clock: C,
    catalog: CandidateSelectorCatalogRevision,
}

impl<C: Clock> BindingService<C> {
    pub fn attach(store: SqliteStore, clock: C, catalog: CandidateSelectorCatalogRevision) -> Self {
        Self {
            store,
            clock,
            catalog,
        }
    }

    /// Binds one criterion of one DRAFT task. Order: task timing,
    /// criterion lookup, unbound check (fail fast before minting a
    /// proof), presence mint, boundary bind, atomic persist. Anything
    /// refuses with the task untouched.
    pub fn bind_for_task(
        &mut self,
        presence: &dyn PresenceAuthority,
        input: OperatorBindingInput,
    ) -> BindingServiceResult<BoundEvidenceRequirement> {
        let task_id = TaskId::new(input.task_id.clone())
            .map_err(|_| BindingServiceError::TaskNotFound(input.task_id.clone()))?;
        let mut task = self
            .store
            .get_task(&task_id)?
            .ok_or_else(|| BindingServiceError::TaskNotFound(input.task_id.clone()))?;
        if task.state() != TaskState::Draft {
            return Err(BindingServiceError::TaskNotDraft {
                task: input.task_id.clone(),
                state: task.state(),
            });
        }
        let requirement = task
            .evidence_requirements()
            .iter()
            .find(|requirement| requirement.criterion_id == input.criterion_id)
            .cloned()
            .ok_or_else(|| BindingServiceError::CriterionNotFound(input.criterion_id.clone()))?;
        if !requirement.params.is_empty() {
            return Err(BindingServiceError::Binding(
                RequirementBindingError::RequirementAlreadyBound,
            ));
        }
        let operation = binding_operation(&input.criterion_id, &input.selector, &input.path);
        let digest = binding_input_digest(
            &input.criterion_id,
            &input.selector,
            &input.path,
            &input.literal,
            &input.policy_fingerprint,
        );
        let proof = presence
            .prove(&operation, &digest)
            .map_err(BindingServiceError::Presence)?;
        let now = self.clock.now_millis();
        if proof.proven_at_ms() > now || now - proof.proven_at_ms() > PRESENCE_FRESHNESS_MS {
            return Err(BindingServiceError::StalePresence {
                proven_at_ms: proof.proven_at_ms(),
                now_ms: now,
            });
        }
        let proposed = ProposedCandidateBinding {
            selector: input.selector.clone(),
            path: input.path.clone(),
            literal: input.literal.clone(),
        };
        let bound = bind_candidate_input_params(
            &self.catalog,
            &requirement,
            &proposed,
            &proof,
            &input.policy_fingerprint,
            now,
        )
        .map_err(BindingServiceError::Binding)?;
        let proven_at = i64::try_from(proof.proven_at_ms())
            .map_err(|_| BindingServiceError::TimestampOutOfRange)?;
        let bound_at = i64::try_from(now).map_err(|_| BindingServiceError::TimestampOutOfRange)?;
        self.store.record_presence_proof(
            proof.nonce(),
            proof.operation(),
            proof.input_digest(),
            proven_at,
        )?;
        let mut requirements = task.evidence_requirements().to_vec();
        let position = requirements
            .iter()
            .position(|requirement| requirement.criterion_id == input.criterion_id)
            .ok_or_else(|| BindingServiceError::CriterionNotFound(input.criterion_id.clone()))?;
        requirements[position] = bound.requirement.clone();
        task.define_evidence_requirements(requirements)
            .map_err(|error| {
                BindingServiceError::Binding(RequirementBindingError::InvalidRequirementShape(
                    error.to_string(),
                ))
            })?;
        self.store.save_task_with_binding_proof(
            &task,
            &BindingProofConsumption {
                nonce: proof.nonce().to_owned(),
                task_id: input.task_id.clone(),
                criterion_id: input.criterion_id.clone(),
                operation,
                bound_at_ms: bound_at,
            },
        )?;
        Ok(bound)
    }
}
