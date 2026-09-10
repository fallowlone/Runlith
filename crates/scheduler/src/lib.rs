//! Minimal deterministic single-task scheduling mechanism (TASK-0037, G3).
//!
//! Pure derived logic only: no SQLite, no storage adapter, no provider SDK,
//! no Task mutation, no runtime mutation. The trusted sequencing lives in
//! `app-services` (`OrchestrationService`), which owns the only path that
//! commits durable state; this crate decides nothing durable.
//!
//! Hard separations:
//!
//! ```text
//! persistent Task lifecycle (task-domain, authoritative)
//! !=
//! derived scheduling readiness (this crate, disposable)
//!
//! FakeAgent result / ExecutionProblem (executor behavior)
//! !=
//! AttemptOutcome (durable attempt conclusion)
//! !=
//! EvaluationVerdict (evaluation crate, independent authority)
//! !=
//! Task lifecycle (task-domain, authoritative)
//! ```
//!
//! Dependency direction: this crate depends on `task-domain` and
//! `agent-runtime` contracts only. It must never depend on `rusqlite`,
//! `storage-sqlite`, `agentd`, `agentctl`, Tauri, or any real provider SDK.

mod execution_port;
mod interpret;
mod outcome;
mod readiness;

pub use execution_port::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};
pub use interpret::{interpret, AttemptInterpretation};
pub use outcome::OrchestrationOutcome;
pub use readiness::{derive_readiness, ScheduleReadiness};
