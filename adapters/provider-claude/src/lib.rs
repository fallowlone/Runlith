//! Claude Code CLI provider adapter (TASK-0046, G5).
//!
//! Provider-neutral anti-corruption layer between orchestration and the
//! installed `claude` CLI, which remains authenticated through the user's
//! own subscription session. Translates canonical execution requests to
//! direct-argv CLI invocations and back, so no Claude-specific concept
//! (session ids, model names, JSON event shapes, CLI flags, auth state)
//! leaks into domain semantics (ADR-0009, providers.md).
//!
//! ```text
//! scheduler::ExecutionPort (canonical seam, owned by orchestration)
//!   ↑ implements
//! ClaudeExecutor (this crate: config + prompt + workdir + process)
//!   ↑ spawns (argv-only, cwd-bound, no shell)
//! claude CLI (provider-owned auth, provider-owned model selection)
//! ```
//!
//! Hard separations (tested): no Task mutation, no storage/SQLite, no
//! journal writes, no Git, no scheduling, no verification, no completion
//! authority — `cargo tree -p provider-claude` must show only
//! `agent-runtime`, `task-domain`, `scheduler`, `workspace` contract
//! crates. Session identifiers are opaque provider state surfaced only in
//! evidence references, never as durable identities.
//!
//! Authentication stays provider-owned: the adapter never reads credential
//! files, never persists tokens, never logs them, and never forwards them.
//! A missing CLI, an incompatible version, or an unauthenticated session
//! surfaces as bounded canonical failure — never as an automatic repair
//! attempt.

mod argv;
mod authentication;
mod capabilities;
mod config;
mod detect;
mod environment;
mod executor;
mod failure;
mod json;
mod output;
mod planner;
mod process;
mod prompt;
mod tracker;
mod usage;

pub use argv::build_argv;
pub use authentication::{probe_authentication, Authentication, AuthenticationState};
pub use capabilities::{probe_capabilities, ClaudeCapabilities};
pub use config::ClaudeConfig;
pub use detect::{detect, find_on_path, CliVersion, Detection, ProviderAvailability};
pub use environment::ProviderEnvironment;
pub use executor::ClaudeExecutor;
pub use failure::ClaudeFailure;
pub use output::{parse_provider_output, ParsedOutput};
pub use planner::ClaudePlanner;
pub use process::CancelFlag;
pub use prompt::build_task_prompt;
pub use tracker::LiveProcessTracker;
pub use usage::{UsageProvenance, UsageSample};

/// Environment name under which a brokered Claude credential reaches the
/// confined child (ADR-0039). Never part of the child environment plan.
pub const OAUTH_TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// Opaque executor reference recorded on durable Attempts created for
/// Claude-driven executions. Identity only — never branched on in domain
/// logic, never a credential, never completion evidence.
pub const CLAUDE_PROVIDER_REF: &str = "claude-code";
