//! Strongly typed domain identifiers. Validated opaque newtypes over
//! `String`; no UUID library, no raw strings in the domain API.

use std::fmt;

use crate::error::{DomainError, DomainResult};

/// Maximum identifier length: generous enough for human-readable
/// `PROJECT-42`-style ids, small enough to stay a cheap domain value.
const MAX_ID_LEN: usize = 256;

/// Shared validation for domain identifiers.
fn validate_id(kind: &str, raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::input(format!("{kind} must not be blank")));
    }
    if trimmed.len() > MAX_ID_LEN {
        return Err(DomainError::input(format!(
            "{kind} exceeds {MAX_ID_LEN} bytes"
        )));
    }
    Ok(trimmed.to_owned())
}

macro_rules! domain_id {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validates and normalizes (trims) the raw identifier.
            pub fn new(raw: impl Into<String>) -> DomainResult<Self> {
                Ok(Self(validate_id($kind, &raw.into())?))
            }

            /// Borrows the normalized identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

domain_id!(
    ProjectId,
    "project id",
    "Durable project identity. Opaque to the Task domain beyond equality."
);
domain_id!(
    TaskId,
    "task id",
    "Durable task identity. Survives restarts, provider switches, and retries."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_accept_normal_values_and_trim() {
        assert_eq!(ProjectId::new("  P-1 ").unwrap().as_str(), "P-1");
        assert_eq!(TaskId::new("AUTH-42").unwrap().to_string(), "AUTH-42");
    }

    #[test]
    fn ids_reject_blank() {
        assert!(ProjectId::new("").is_err());
        assert!(ProjectId::new("   ").is_err());
        assert!(TaskId::new("").is_err());
        assert!(TaskId::new("\t\n ").is_err());
    }

    #[test]
    fn ids_reject_overlong() {
        let long = "x".repeat(MAX_ID_LEN + 1);
        assert!(TaskId::new(long).is_err());
        assert!(TaskId::new("x".repeat(MAX_ID_LEN)).is_ok());
    }

    #[test]
    fn ids_support_equality_ordering_hash() {
        use std::collections::HashSet;
        let a = TaskId::new("A").unwrap();
        let b = TaskId::new("B").unwrap();
        assert!(a < b);
        assert_ne!(a, b);
        assert_eq!(a.clone(), a);
        let mut set = HashSet::new();
        set.insert(a.clone());
        set.insert(a);
        assert_eq!(set.len(), 1);
    }
}
