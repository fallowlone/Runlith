//! Domain-local error type. No product-wide taxonomy.

use std::fmt;

/// Plan-domain validation failure (constructor-level, not a V-rule refusal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanDomainError {
    /// Blank, control-character, or otherwise malformed identifier/input.
    InvalidInput(String),
}

impl PlanDomainError {
    pub(crate) fn input(detail: impl Into<String>) -> Self {
        Self::InvalidInput(detail.into())
    }
}

impl fmt::Display for PlanDomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
        }
    }
}

impl std::error::Error for PlanDomainError {}

pub(crate) fn checked_id(field: &'static str, value: String) -> Result<String, PlanDomainError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(PlanDomainError::input(format!(
            "{field} must be non-blank with no control characters"
        )));
    }
    Ok(value)
}
