//! Typed, versioned Event Journal records.
//!
//! The Task domain owns what a lifecycle fact means; this module owns the
//! persistence mechanics. Event types are stable canonical names (never
//! integer ordinals or Rust discriminants) and every record carries an
//! explicit event-schema version. Decoding an unknown type or version fails
//! explicitly instead of guessing.

use std::fmt;

use agent_runtime::{AttemptId, ExecutionId};
use task_domain::{ProjectId, TaskId, TaskState};

use crate::error::{StorageError, StorageResult};

/// Event-schema version written by this adapter. Distinct from the SQLite
/// schema version, the application build version, and any protocol version.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Canonical lifecycle, mutation, and runtime fact names.
/// Provider-neutral and runtime-neutral by construction: no provider-,
/// model-, or runner-specific type can exist here — the enum is closed.
/// The G2 creation/dependency types (`project.created`, `task.created`,
/// `task.dependency_added`) close the TASK-0027 audit gap; the G3
/// runtime types (`execution.created`, `attempt.created`,
/// `attempt.concluded`) materialize ADR-0022. No generic
/// `entity.changed`-style type exists, and there is deliberately no
/// event type per Attempt outcome, per FakeAgent scenario, per Session,
/// or per Execution terminal state.
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
    ProjectCreated,
    TaskCreated,
    DependencyAdded,
    ExecutionCreated,
    AttemptCreated,
    AttemptConcluded,
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
            Self::ProjectCreated => "project.created",
            Self::TaskCreated => "task.created",
            Self::DependencyAdded => "task.dependency_added",
            Self::ExecutionCreated => "execution.created",
            Self::AttemptCreated => "attempt.created",
            Self::AttemptConcluded => "attempt.concluded",
        }
    }

    /// Every canonical type, for exhaustive tests.
    #[must_use]
    pub const fn all() -> [Self; 15] {
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
            Self::ProjectCreated,
            Self::TaskCreated,
            Self::DependencyAdded,
            Self::ExecutionCreated,
            Self::AttemptCreated,
            Self::AttemptConcluded,
        ]
    }

    /// Whether this fact is project-scoped (no task subject): only
    /// `project.created`. Every other type is task-scoped.
    #[must_use]
    pub const fn is_project_scoped(self) -> bool {
        matches!(self, Self::ProjectCreated)
    }

    /// Whether this fact is runtime-scoped (carries Execution/Attempt
    /// references under ADR-0022): exactly the three G3 runtime types.
    /// Runtime facts never travel through the Task lifecycle commit
    /// path; lifecycle facts never carry runtime scope.
    #[must_use]
    pub const fn is_runtime_scoped(self) -> bool {
        matches!(
            self,
            Self::ExecutionCreated | Self::AttemptCreated | Self::AttemptConcluded
        )
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
            "project.created" => Self::ProjectCreated,
            "task.created" => Self::TaskCreated,
            "task.dependency_added" => Self::DependencyAdded,
            "execution.created" => Self::ExecutionCreated,
            "attempt.created" => Self::AttemptCreated,
            "attempt.concluded" => Self::AttemptConcluded,
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

/// Canonical terminal Attempt outcome as journal scope (ADR-0022).
///
/// This mirrors [`agent_runtime::AttemptOutcome`] exactly — four
/// variants, no per-scenario outcomes — serialized lowercase snake to
/// stay visually distinct from SCREAMING Task states. Stored text is
/// the authority (never an enum ordinal); unknown text is corruption.
/// Attempt `detail` is intentionally absent here: it is diagnostic
/// relational state, not event taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttemptOutcome {
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl AttemptOutcome {
    /// Stable persisted spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }

    /// Every canonical outcome, for exhaustive tests.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [
            Self::Succeeded,
            Self::Failed,
            Self::TimedOut,
            Self::Cancelled,
        ]
    }

    /// Strict decode: unknown text (including FakeAgent scenario names
    /// such as `crash`) is corruption, never a default.
    pub fn parse(raw: &str) -> StorageResult<Self> {
        Ok(match raw {
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "timed_out" => Self::TimedOut,
            "cancelled" => Self::Cancelled,
            other => {
                return Err(StorageError::Corrupt(format!(
                    "unknown persisted attempt outcome {other:?}"
                )));
            }
        })
    }
}

impl fmt::Display for AttemptOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<agent_runtime::AttemptOutcome> for AttemptOutcome {
    fn from(outcome: agent_runtime::AttemptOutcome) -> Self {
        match outcome {
            agent_runtime::AttemptOutcome::Succeeded => Self::Succeeded,
            agent_runtime::AttemptOutcome::Failed => Self::Failed,
            agent_runtime::AttemptOutcome::TimedOut => Self::TimedOut,
            agent_runtime::AttemptOutcome::Cancelled => Self::Cancelled,
        }
    }
}

impl From<AttemptOutcome> for agent_runtime::AttemptOutcome {
    fn from(outcome: AttemptOutcome) -> Self {
        match outcome {
            AttemptOutcome::Succeeded => Self::Succeeded,
            AttemptOutcome::Failed => Self::Failed,
            AttemptOutcome::TimedOut => Self::TimedOut,
            AttemptOutcome::Cancelled => Self::Cancelled,
        }
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

/// A lifecycle, mutation, or runtime fact accepted for append. Identity
/// is assigned by the store at commit time (see [`EventId`]); everything
/// else is fixed beforehand by the trusted application service — never
/// by provider/model output.
///
/// Scope discipline (migrations 0003/0004): `project.created` is
/// project-scoped (`task_id`, `from_state`, `to_state` are all `None` —
/// never a fake or sentinel task id); G2 task/dependency/lifecycle
/// facts are task-scoped (`Some` throughout, no runtime scope);
/// runtime facts follow the ADR-0022 matrix (`execution.created`:
/// execution scope, no attempt, no outcome, no Task states;
/// `attempt.created`: execution + attempt scope, no outcome, no Task
/// states; `attempt.concluded`: execution + attempt scope plus the
/// canonical outcome, no Task states).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJournalEvent {
    /// What committed fact this is.
    pub event_type: EventType,
    /// Task the fact is about; `None` only for `project.created`.
    pub task_id: Option<TaskId>,
    /// Project the fact belongs to; always required.
    pub project_id: ProjectId,
    /// Lifecycle state before the mutation; `None` for
    /// `project.created` and for every runtime fact. Task-state
    /// semantics only — Attempt outcomes never live here.
    pub from_state: Option<TaskState>,
    /// Lifecycle state after the mutation; `None` for
    /// `project.created` and for every runtime fact.
    pub to_state: Option<TaskState>,
    /// Bounded metadata: block reason, submission summary, or verdict
    /// reference. Short human/operator text only — never prompts, secrets,
    /// transcripts, or raw tool output. Never runtime aggregate identity
    /// and never the authoritative Attempt outcome.
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
    /// Execution the fact is about; `Some` only for the three runtime
    /// types. First-class structured reference, never an encoding.
    pub execution_id: Option<ExecutionId>,
    /// Attempt the fact is about; `Some` only for `attempt.created` and
    /// `attempt.concluded`.
    pub attempt_id: Option<AttemptId>,
    /// Canonical terminal outcome; `Some` only for `attempt.concluded`.
    /// The authoritative outcome — `reason` never carries it.
    pub attempt_outcome: Option<AttemptOutcome>,
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
        assert!(EventType::parse("attempt.exploded").is_err());
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

    #[test]
    fn attempt_outcome_serialization_is_canonical() {
        // ADR-0022 spellings: lowercase snake, distinct from SCREAMING
        // Task states. Every variant round-trips; scenario names and
        // ordinals are corruption, never defaults.
        for outcome in AttemptOutcome::all() {
            let name = outcome.as_str();
            assert_eq!(AttemptOutcome::parse(name).unwrap(), outcome);
            assert_eq!(name, name.to_lowercase());
        }
        assert_eq!(AttemptOutcome::Succeeded.as_str(), "succeeded");
        assert_eq!(AttemptOutcome::Failed.as_str(), "failed");
        assert_eq!(AttemptOutcome::TimedOut.as_str(), "timed_out");
        assert_eq!(AttemptOutcome::Cancelled.as_str(), "cancelled");
        for bogus in [
            "crash",
            "quota_reached",
            "network_failure",
            "0",
            "Failed",
            "",
        ] {
            assert!(
                AttemptOutcome::parse(bogus).is_err(),
                "{bogus:?} must not decode as an outcome"
            );
        }
        // The journal codec mirrors the domain 1:1, both directions.
        for outcome in AttemptOutcome::all() {
            let domain: agent_runtime::AttemptOutcome = outcome.into();
            assert_eq!(AttemptOutcome::from(domain), outcome);
        }
    }

    #[test]
    fn runtime_event_names_are_stable_and_provider_neutral() {
        for event_type in [
            EventType::ExecutionCreated,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
        ] {
            assert!(event_type.is_runtime_scoped());
            assert!(!event_type.is_project_scoped());
            let name = event_type.as_str();
            assert_eq!(EventType::parse(name).unwrap(), event_type);
            for token in ["claude", "codex", "gemini", "gpt", "provider", "session"] {
                assert!(
                    !name.contains(token),
                    "event name {name:?} must stay provider-neutral"
                );
            }
        }
        for event_type in EventType::all() {
            if matches!(
                event_type,
                EventType::ExecutionCreated
                    | EventType::AttemptCreated
                    | EventType::AttemptConcluded
            ) {
                continue;
            }
            assert!(!event_type.is_runtime_scoped());
        }
    }
}
