//! Storage-local error model. Typed, adapter-scoped — not the final
//! application-wide taxonomy.

use std::fmt;

/// SQLite persistence failure.
#[derive(Debug)]
pub enum StorageError {
    /// Underlying SQLite / I/O failure (source preserved).
    Sqlite(rusqlite::Error),
    /// Migration SQL could not be applied.
    Migration(String),
    /// Database schema is newer than this adapter supports. Fail closed:
    /// never guess at a newer layout.
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    /// Requested project/task/edge does not exist.
    NotFound(String),
    /// Identity or edge already exists.
    Duplicate(String),
    /// A relational constraint rejected the write.
    ConstraintViolation(String),
    /// Stored bytes cannot represent a valid domain object. Never repaired
    /// silently; surfaced for operator attention.
    Corrupt(String),
    /// Stored content passed SQL decoding but failed domain validation.
    DomainReconstruction(String),
    /// The durable database path or one of its SQLite sidecars is unsafe to
    /// trust. Storage never repairs an existing object in this state.
    UnsafePath(String),
    /// A write would persist a graph the domain rejects (self-edge,
    /// duplicate, cross-project, or cycle). Checked before touching SQL.
    InvalidGraph(String),
    /// A guarded lifecycle write found a different current state than the
    /// mutation was validated against: a concurrent writer moved first.
    /// The mutation fails instead of silently overwriting newer state.
    Stale {
        /// Task whose write was refused.
        task: String,
        /// State the mutation was validated against.
        expected: String,
        /// State actually stored.
        found: String,
    },
    /// A Plan revision was validated against a branch head that is no
    /// longer current. Storage never rebases or advances such a proposal.
    PlanStale {
        /// Plan branch whose durable head moved.
        branch: String,
        /// Revision the accepted proposal was validated against.
        expected: u64,
        /// Durable branch head observed inside the write transaction.
        found: u64,
    },
    /// A planner worker no longer owns the branch generation it started with.
    PlanningRunStale {
        branch: String,
        expected_generation: u64,
        found_generation: u64,
    },
    /// A Plan revision references a planning-input snapshot that is no longer
    /// the branch's current product-owned input.
    PlanningInputStale {
        branch: String,
        expected_fingerprint: String,
    },
    /// A HumanDecisionRequest no longer describes the canonical state the
    /// human saw. Continuation always fails closed before mutation.
    HumanDecisionStale { request_id: String, reason: String },
    /// A HumanDecisionRequest is durable but its lifecycle does not permit
    /// the requested resolve/consume transition (including replay).
    HumanDecisionState { request_id: String, state: String },
    /// A takeover return found live workspace bindings that no longer
    /// match the anchors captured at admission. Fail closed: the scope
    /// stays Blocked. The service maps this to `GitBackendFailure`.
    TakeoverAnchorMismatch { record_id: String, detail: String },
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "sqlite failure: {error}"),
            Self::Migration(detail) => write!(f, "migration failure: {detail}"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported schema version {found} (adapter supports {supported})"
            ),
            Self::NotFound(detail) => write!(f, "not found: {detail}"),
            Self::Duplicate(detail) => write!(f, "duplicate: {detail}"),
            Self::ConstraintViolation(detail) => write!(f, "constraint violation: {detail}"),
            Self::Corrupt(detail) => write!(f, "corrupt persisted data: {detail}"),
            Self::DomainReconstruction(detail) => {
                write!(f, "domain reconstruction failure: {detail}")
            }
            Self::UnsafePath(detail) => write!(f, "unsafe sqlite path: {detail}"),
            Self::InvalidGraph(detail) => {
                write!(f, "invalid graph: {detail}")
            }
            Self::Stale {
                task,
                expected,
                found,
            } => {
                write!(
                    f,
                    "stale write for task {task}: validated against {expected}, stored state is {found}"
                )
            }
            Self::PlanStale {
                branch,
                expected,
                found,
            } => write!(
                f,
                "stale plan write for branch {branch}: validated against revision {expected}, durable head is {found}"
            ),
            Self::PlanningRunStale {
                branch,
                expected_generation,
                found_generation,
            } => write!(
                f,
                "stale planning run for branch {branch}: expected generation {expected_generation}, current generation is {found_generation}"
            ),
            Self::PlanningInputStale {
                branch,
                expected_fingerprint,
            } => write!(
                f,
                "stale planning input for branch {branch}: snapshot fingerprint {expected_fingerprint} is no longer current"
            ),
            Self::HumanDecisionStale { request_id, reason } => write!(
                f,
                "stale human decision {request_id}: {reason}"
            ),
            Self::HumanDecisionState { request_id, state } => write!(
                f,
                "human decision {request_id} cannot transition from {state}"
            ),
            Self::TakeoverAnchorMismatch { record_id, detail } => write!(
                f,
                "takeover {record_id} workspace anchor mismatch: {detail}"
            ),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Storage result alias.
pub type StorageResult<T> = Result<T, StorageError>;

/// Maps a raw SQLite failure on a uniqueness-guarded write to the typed
/// duplicate error; every other SQLite failure keeps its source.
pub(crate) fn map_constraint(error: rusqlite::Error, what: &str) -> StorageError {
    use rusqlite::ErrorCode;
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == ErrorCode::ConstraintViolation =>
        {
            StorageError::ConstraintViolation(format!("{what}: {error}"))
        }
        _ => StorageError::Sqlite(error),
    }
}
