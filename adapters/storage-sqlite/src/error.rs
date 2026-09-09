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
