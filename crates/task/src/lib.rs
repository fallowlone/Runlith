//! Pure Task-domain model and lifecycle state machine (G2 foundation).
//!
//! Deterministic and persistence-independent: no SQLite, no Event Journal,
//! no IPC, no providers, no scheduler. The entities here carry durable
//! business semantics only:
//!
//! ```text
//! Task != Execution != Attempt != Session
//! ```
//!
//! Completion authority is external to the implementing agent: the only path
//! to [`TaskState::Completed`] is the trusted verification transition
//! ([`Task::verification_succeeded`]); there is no `complete_task()` and no
//! unrestricted state setter. Canonical lifecycle and transition authority
//! follow `docs/architecture/task-engine.md` (§5–§7, §13); anything that
//! document marks unresolved (all incoming `FAILED` / `CANCELLED`
//! transitions, `SUBMITTED → VERIFYING` bypass, review-rejection paths) is
//! rejected here rather than guessed.
//!
//! State names are fixed by the accepted contract and mirrored by
//! `contracts/json-schema/task.schema.json` (representation shape only).

mod dependency;
mod error;
mod id;
mod project;
mod state;
mod task;

pub use dependency::validate_acyclic;
pub use error::DomainError;
pub use id::{ProjectId, TaskId};
pub use project::Project;
pub use state::{can_transition, TaskState, ACCEPTED_TRANSITIONS};
pub use task::{
    Task, TaskSnapshot, TransitionCommand, MAX_TASK_ACCEPTANCE, MAX_TASK_ALLOWED_PATHS,
    MAX_TASK_METADATA_ITEM_LEN,
};
