//! Provider-neutral durable runtime domain: Execution and Attempt.
//!
//! Structural foundation for G3 execution (ADR-0005: `Task != Execution
//! != Attempt != Session`). This crate owns the runtime shape only:
//!
//! ```text
//! Task (task-domain: durable desired work)
//!  └── Execution (one durable execution under an assignment)
//!       ├── Attempt 1 (one concrete try: concluded, immutable)
//!       └── Attempt 2 (retry: a NEW attempt, never a reset)
//! ```
//!
//! Deliberately absent: provider adapters, Scheduler, persistence,
//! Event Journal writes, IPC, runners, Git/worktrees, and any AI. An
//! `Attempt` records *that* an executor ran and *how it concluded*
//! (plus an opaque executor reference); provider transcripts, tool
//! outputs, and session mechanics stay outside this model.
//!
//! Completion authority: this crate cannot complete a Task. Runtime
//! outcomes are evidence that later flows through `submit_result` and
//! independent evaluation; no API here touches Task lifecycle state,
//! and the crate depends on `task-domain` for identifiers only.

mod attempt;
mod error;
mod execution;
mod id;
mod snapshot;

pub use attempt::{Attempt, AttemptOutcome};
pub use error::{RuntimeError, RuntimeResult};
pub use execution::Execution;
pub use id::{AttemptId, ExecutionId, ProviderRef};
pub use snapshot::{AttemptSnapshot, ExecutionSnapshot};
