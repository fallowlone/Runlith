//! Typed, versioned Event Journal records.
//!
//! The Task domain owns what a lifecycle fact means; this module owns the
//! persistence mechanics. Event types are stable canonical names (never
//! integer ordinals or Rust discriminants) and every record carries an
//! explicit event-schema version. Decoding an unknown type or version fails
//! explicitly instead of guessing.

use std::fmt;

use task_domain::{ProjectId, TaskId, TaskState};

use crate::error::{StorageError, StorageResult};

/// Event-schema version written by this adapter. Distinct from the SQLite
/// schema version, the application build version, and any protocol version.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Canonical lifecycle fact names. Provider-neutral and runtime-neutral by
/// construction: no provider-, model-, or runner-specific type can exist
/// here — the enum is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    MarkedReady,
    Claimed,
    Started,
    Blocked,
    Submitted,
    RoutedToReview,
    VerificationBegan,
    VerificationSucceeded,
    VerificationFailed,
}

impl EventType {
    /// Stable persisted name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MarkedReady => "task.marked_ready",
            Self::Claimed => "task.claimed",
            Self::Started => "task.started",
            Self::Blocked => "task.blocked",
            Self::Submitted => "task.submitted",
            Self::RoutedToReview => "task.routed_to_review",
            Self::VerificationBegan => "task.verification_began",
            Self::VerificationSucceeded => "task.verification_succeeded",
            Self::VerificationFailed => "task.verification_failed",
        }
    }

    /// Every canonical type, for exhaustive tests.
    #[must_use]
    pub const fn all() -> [Self; 9] {
        [
            Self::MarkedReady,
            Self::Claimed,
            Self::Started,
            Self::Blocked,
            Self::Submitted,
            Self::RoutedToReview,
            Self::VerificationBegan,
            Self::VerificationSucceeded,
            Self::VerificationFailed,
        ]
    }

    /// Strict decode: unknown names are corruption, never a default.
    pub fn parse(raw: &str) -> StorageResult<Self> {
        Ok(match raw {
            "task.marked_ready" => Self::MarkedReady,
            "task.claimed" => Self::Claimed,
            "task.started" => Self::Started,
            "task.blocked" => Self::Blocked,
            "task.submitted" => Self::Submitted,
            "task.routed_to_review" => Self::RoutedToReview,
            "task.verification_began" => Self::VerificationBegan,
            "task.verification_succeeded" => Self::VerificationSucceeded,
            "task.verification_failed" => Self::VerificationFailed,
            other => {
                return Err(StorageError::Corrupt(format!(
                    "unknown persisted event type {other:?}"
                )));
            }
        })
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stable journal identity, derived from the storage-local sequence:
/// `evt-<seq>`. The sequence is the journal's deterministic retrieval
/// order (storage mechanics, not a domain contract); the string form is a
/// pure function of it, unique and stable per database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EventId(i64);

impl EventId {
    /// Parses the canonical `evt-<seq>` form; anything else is not a
    /// journal identity.
    pub fn parse(raw: &str) -> StorageResult<Self> {
        let seq = raw
            .strip_prefix("evt-")
            .and_then(|digits| digits.parse::<i64>().ok());
        match seq {
            Some(seq) if seq > 0 => Ok(Self(seq)),
            _ => Err(StorageError::Corrupt(format!(
                "malformed event identity {raw:?}"
            ))),
        }
    }

    /// The storage-local sequence number.
    #[must_use]
    pub fn seq(self) -> i64 {
        self.0
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "evt-{}", self.0)
    }
}

/// A lifecycle fact accepted for append. Identity is assigned by the store
/// at commit time (see [`EventId`]); everything else is fixed beforehand by
/// the trusted application service — never by provider/model output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJournalEvent {
    /// What committed fact this is.
    pub event_type: EventType,
    /// Task the fact is about.
    pub task_id: TaskId,
    /// Project the task belongs to.
    pub project_id: ProjectId,
    /// Lifecycle state before the mutation.
    pub from_state: TaskState,
    /// Lifecycle state after the mutation.
    pub to_state: TaskState,
    /// Bounded metadata: block reason, submission summary, or verdict
    /// reference. Short human/operator text only — never prompts, secrets,
    /// transcripts, or raw tool output.
    pub reason: Option<String>,
    /// Chain this mutation belongs to (minted per trusted request).
    pub correlation_id: String,
    /// Prior request that directly caused this fact.
    pub causation_id: String,
    /// Wall-clock timestamp, RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
    pub occurred_at: String,
    /// Where the mutation came from, e.g. `task-application-service/start`.
    /// Never model narrative.
    pub provenance: String,
}

/// A committed journal record: the accepted fact plus its stable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEvent {
    /// Assigned identity (`evt-<seq>`).
    pub id: EventId,
    /// Storage-local sequence; journal retrieval order.
    pub seq: i64,
    /// Event-schema version (always [`EVENT_SCHEMA_VERSION`] from v1).
    pub event_version: u32,
    /// The accepted fact.
    pub fact: NewJournalEvent,
}

/// Canonical task-state codec shared by the task rows and the journal's
/// from/to columns: explicit names out, strict match back.
pub(crate) fn decode_state(raw: &str) -> StorageResult<TaskState> {
    Ok(match raw {
        "DRAFT" => TaskState::Draft,
        "READY" => TaskState::Ready,
        "CLAIMED" => TaskState::Claimed,
        "RUNNING" => TaskState::Running,
        "BLOCKED" => TaskState::Blocked,
        "SUBMITTED" => TaskState::Submitted,
        "AWAITING_REVIEW" => TaskState::AwaitingReview,
        "VERIFYING" => TaskState::Verifying,
        "RETRY_READY" => TaskState::RetryReady,
        "COMPLETED" => TaskState::Completed,
        "FAILED" => TaskState::Failed,
        "CANCELLED" => TaskState::Cancelled,
        other => {
            return Err(StorageError::Corrupt(format!(
                "unknown persisted task state {other:?}"
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_names_are_stable_and_provider_neutral() {
        for event_type in EventType::all() {
            let name = event_type.as_str();
            assert_eq!(EventType::parse(name).unwrap(), event_type);
            for token in ["claude", "codex", "gemini", "gpt", "provider"] {
                assert!(
                    !name.contains(token),
                    "event name {name:?} must stay provider-neutral"
                );
            }
        }
    }

    #[test]
    fn unknown_event_type_fails_explicitly() {
        assert!(EventType::parse("task.deleted").is_err());
        assert!(EventType::parse("").is_err());
        assert!(EventType::parse("7").is_err());
    }

    #[test]
    fn event_id_round_trip() {
        let id = EventId::parse("evt-42").unwrap();
        assert_eq!(id.seq(), 42);
        assert_eq!(id.to_string(), "evt-42");
        assert!(EventId::parse("evt-0").is_err());
        assert!(EventId::parse("42").is_err());
        assert!(EventId::parse("evt-abc").is_err());
    }
}
