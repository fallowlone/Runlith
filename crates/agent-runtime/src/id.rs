//! Strongly typed runtime identifiers. Validated opaque newtypes over
//! `String`, following the `task-domain` identifier conventions (trim,
//! non-blank, 256-byte bound); no UUID library, no raw strings in the
//! runtime API. Identity generation policy lives outside the domain:
//! callers supply identifiers, the domain validates them.

use std::fmt;

use crate::error::{RuntimeError, RuntimeResult};

/// Maximum identifier length, mirroring the task-domain convention.
const MAX_ID_LEN: usize = 256;

/// Shared validation for runtime identifiers.
fn validate_id(kind: &str, raw: &str) -> RuntimeResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::input(format!("{kind} must not be blank")));
    }
    if trimmed.len() > MAX_ID_LEN {
        return Err(RuntimeError::input(format!(
            "{kind} exceeds {MAX_ID_LEN} bytes"
        )));
    }
    Ok(trimmed.to_owned())
}

macro_rules! runtime_id {
    ($name:ident, $kind:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validates and normalizes (trims) the raw identifier.
            pub fn new(raw: impl Into<String>) -> RuntimeResult<Self> {
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

runtime_id!(
    ExecutionId,
    "execution id",
    "Durable execution identity, independent from the Task it executes."
);

runtime_id!(
    AttemptId,
    "attempt id",
    "One concrete try inside an execution. Never migrates between executions."
);

runtime_id!(
    ProviderRef,
    "provider reference",
    "Opaque executor reference (ADR-0005 `ProviderRef` vocabulary): names which \
     runtime performed an attempt without branching on it. Never a provider-specific \
     type, session transcript, or credential."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_accept_normal_values_and_trim() {
        assert_eq!(ExecutionId::new("  E-1 ").unwrap().as_str(), "E-1");
        assert_eq!(AttemptId::new("A-1").unwrap().to_string(), "A-1");
        assert_eq!(
            ProviderRef::new("fake-agent").unwrap().as_str(),
            "fake-agent"
        );
    }

    #[test]
    fn ids_reject_blank() {
        assert!(ExecutionId::new("").is_err());
        assert!(ExecutionId::new("   ").is_err());
        assert!(AttemptId::new("").is_err());
        assert!(AttemptId::new("\t\n ").is_err());
        assert!(ProviderRef::new("  ").is_err());
    }

    #[test]
    fn ids_reject_overlong() {
        let long = "x".repeat(MAX_ID_LEN + 1);
        assert!(ExecutionId::new(&long).is_err());
        assert!(AttemptId::new(&long).is_err());
        assert!(ProviderRef::new(&long).is_err());
        assert!(ExecutionId::new("x".repeat(MAX_ID_LEN)).is_ok());
    }

    #[test]
    fn runtime_identities_are_distinct_types() {
        // Compile-level separation matters: a TaskId must never flow where
        // an ExecutionId is required. TypeId inequality proves the three
        // identity types (plus the task-domain TaskId) are distinct.
        use std::any::TypeId;
        use task_domain::TaskId;
        let ids = [
            TypeId::of::<TaskId>(),
            TypeId::of::<ExecutionId>(),
            TypeId::of::<AttemptId>(),
        ];
        for (index, first) in ids.iter().enumerate() {
            for second in &ids[index + 1..] {
                assert_ne!(first, second, "identity types must stay distinct");
            }
        }
    }
}
