//! Runtime-local error type. No product-wide taxonomy.

use std::fmt;

/// Runtime-domain validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// Blank, over-long, or otherwise malformed identifier/input/detail.
    InvalidInput(String),
    /// A new attempt was started while the execution still has an open
    /// one. Retries are sequential: conclude the open attempt first.
    AttemptAlreadyOpen(String),
    /// Two attempts of one execution share an identity.
    DuplicateAttempt(String),
    /// No attempt of that identity exists in the execution.
    AttemptNotFound(String),
    /// The attempt already concluded; history is never rewritten.
    AttemptAlreadyConcluded(String),
}

impl RuntimeError {
    pub(crate) fn input(detail: impl Into<String>) -> Self {
        Self::InvalidInput(detail.into())
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
            Self::AttemptAlreadyOpen(detail) => write!(f, "attempt already open: {detail}"),
            Self::DuplicateAttempt(detail) => write!(f, "duplicate attempt: {detail}"),
            Self::AttemptNotFound(detail) => write!(f, "attempt not found: {detail}"),
            Self::AttemptAlreadyConcluded(detail) => {
                write!(f, "attempt already concluded: {detail}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Runtime-domain result alias.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_failure_kind() {
        assert!(RuntimeError::input("x")
            .to_string()
            .starts_with("invalid input"));
        assert!(RuntimeError::AttemptAlreadyOpen("e".into())
            .to_string()
            .contains("already open"));
        assert!(RuntimeError::DuplicateAttempt("a".into())
            .to_string()
            .contains("duplicate"));
        assert!(RuntimeError::AttemptNotFound("a".into())
            .to_string()
            .contains("not found"));
        assert!(RuntimeError::AttemptAlreadyConcluded("a".into())
            .to_string()
            .contains("already concluded"));
    }
}
