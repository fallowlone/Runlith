//! Service-local error model: domain validation vs storage failure stay
//! distinguishable. No application-wide taxonomy is invented here.

use std::fmt;

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

/// Service result alias.
pub type ServiceResult<T> = Result<T, ServiceError>;
