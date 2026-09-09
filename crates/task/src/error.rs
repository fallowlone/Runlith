//! Small domain-local error type. No product-wide taxonomy.

use std::fmt;

/// Domain validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// Blank, over-long, or otherwise malformed identifier/input.
    InvalidInput(String),
    /// A lifecycle transition the accepted contract does not allow
    /// (including transitions into `FAILED` / `CANCELLED`, whose incoming
    /// edges are all unresolved per `task-engine.md` §7/§16).
    InvalidTransition(String),
    /// A task depending on itself.
    SelfDependency(String),
    /// A dependency edge that already exists.
    DuplicateDependency(String),
    /// A dependency set whose graph contains a cycle.
    DependencyCycle(String),
    /// A dependency edge across project boundaries.
    ProjectMismatch(String),
}

impl DomainError {
    pub(crate) fn input(detail: impl Into<String>) -> Self {
        Self::InvalidInput(detail.into())
    }

    pub(crate) fn transition(detail: impl Into<String>) -> Self {
        Self::InvalidTransition(detail.into())
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
            Self::InvalidTransition(detail) => write!(f, "invalid transition: {detail}"),
            Self::SelfDependency(detail) => write!(f, "self dependency: {detail}"),
            Self::DuplicateDependency(detail) => write!(f, "duplicate dependency: {detail}"),
            Self::DependencyCycle(detail) => write!(f, "dependency cycle: {detail}"),
            Self::ProjectMismatch(detail) => write!(f, "project mismatch: {detail}"),
        }
    }
}

impl std::error::Error for DomainError {}

/// Domain result alias.
pub type DomainResult<T> = Result<T, DomainError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_failure_kind() {
        assert!(DomainError::input("x")
            .to_string()
            .starts_with("invalid input"));
        assert!(DomainError::transition("x")
            .to_string()
            .starts_with("invalid transition"));
        assert!(DomainError::SelfDependency("x".into())
            .to_string()
            .contains("self"));
        assert!(DomainError::DuplicateDependency("x".into())
            .to_string()
            .contains("duplicate"));
        assert!(DomainError::DependencyCycle("x".into())
            .to_string()
            .contains("cycle"));
        assert!(DomainError::ProjectMismatch("x".into())
            .to_string()
            .contains("project"));
    }
}
