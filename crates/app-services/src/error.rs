//! Service-local error model: domain validation vs storage failure stay
//! distinguishable. No application-wide taxonomy is invented here.

use std::fmt;

use agent_runtime::RuntimeError;
use storage_sqlite::StorageError;
use task_domain::DomainError;

/// Trusted-service failure.
#[derive(Debug)]
pub enum ServiceError {
    /// The canonical domain operation rejected the mutation. Nothing was
    /// persisted and no event was appended.
    Domain(DomainError),
    /// SQLite/consistency failure, including stale-write conflicts.
    Storage(StorageError),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => write!(f, "task mutation rejected: {error}"),
            Self::Storage(error) => write!(f, "task mutation storage failure: {error}"),
        }
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Domain(error) => Some(error),
            Self::Storage(error) => Some(error),
        }
    }
}

impl From<DomainError> for ServiceError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

impl From<StorageError> for ServiceError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<workspace::WorkspaceFailure> for ServiceError {
    fn from(error: workspace::WorkspaceFailure) -> Self {
        // Same frozen-taxonomy rule as RuntimeError above: workspace
        // discipline rejections (ownership mismatch, dirty worktree,
        // live owner, lease conflicts) surface as explicit constraint
        // violations carrying the verbatim workspace reason. The durable
        // facts committed before the failure stay committed — the caller
        // sees a legitimate recoverable intermediate, never a hidden
        // rollback.
        Self::Storage(StorageError::ConstraintViolation(error.to_string()))
    }
}

impl From<RuntimeError> for ServiceError {
    fn from(error: RuntimeError) -> Self {
        // Deliberately no new variant: bins/agentd matches ServiceError
        // exhaustively under the frozen G2 wire taxonomy, so runtime
        // discipline rejections (open sibling, duplicate identity,
        // unknown or already-concluded attempt) surface as explicit
        // constraint violations carrying the verbatim domain reason.
        // Nothing is persisted and no event is appended either way.
        Self::Storage(StorageError::ConstraintViolation(error.to_string()))
    }
}

/// Service result alias.
pub type ServiceResult<T> = Result<T, ServiceError>;
