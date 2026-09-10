//! Deterministic FakeAgent adapter.
//!
//! [`FakeAgent`] is a per-scenario test double behind the provider-facing
//! execution seam: it takes a provider-neutral [`ExecutionRequest`] plus a
//! [`ExecutionControl`] coordination handle and returns deterministic
//! behavior — [`ExecutionOutput`] or [`FakeAgentError`] — exactly once per
//! call. No retry, no scheduling, no persistence, no Task access.
//!
//! Cancellation (providers.md §15: provider owns provider/session
//! cancellation semantics): [`ExecutionControl`] is the provider-neutral
//! coordination handle. Non-`hang` scenarios ignore it; `hang` blocks until
//! the test releases or cancels it, then exits through the canonical path
//! (`Ok` released output vs `Err(Cancelled)`). Cancellation is idempotent.
//!
//! Session semantics (ADR-0005, providers.md §6): [`FakeSessionHandle`] is
//! an opaque in-memory value minted per execution and dropped afterwards.
//! No session table, no transcript, no journal events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::request::ExecutionRequest;
use crate::result::{ExecutionOutput, FakeAgentError};
use crate::scenario::FakeScenario;

/// Opaque executor reference recorded on durable Attempts created for
/// FakeAgent-driven tests. Identity only — never branched on in domain
/// logic, never a credential.
pub const FAKE_PROVIDER_REF: &str = "fake-agent";

/// Short condvar poll quantum for `hang` coordination. Correctness never
/// depends on its value: the test decides via [`ExecutionControl`]
/// release/cancel, and this only bounds wake-up latency.
const HANG_POLL_QUANTUM: Duration = Duration::from_millis(5);

/// Deterministic simulation endpoints that are never touched.
const FORBIDDEN_PATH: &str = "/forbidden/fake-agent-write";
/// Command text that is reported, never launched.
const FORBIDDEN_COMMAND: &str = "rm -rf / --no-preserve-root (not executed)";

/// Provider-neutral coordination handle for one execution.
///
/// In-memory only: release/cancel flags plus a condvar. Shared across
/// threads via cheap [`Clone`] (`Arc` internals), so a test can hold one
/// handle while the execution blocks on its clone. Non-`hang` scenarios
/// ignore it; `hang` honors it.
#[derive(Debug, Clone)]
pub struct ExecutionControl {
    state: Arc<ControlState>,
}

#[derive(Debug)]
struct ControlState {
    released: Mutex<bool>,
    cancelled: AtomicBool,
    cvar: Condvar,
}

impl ExecutionControl {
    /// Fresh unreleased, uncancelled handle for one execution.
    #[must_use]
    pub fn fresh() -> Self {
        Self {
            state: Arc::new(ControlState {
                released: Mutex::new(false),
                cancelled: AtomicBool::new(false),
                cvar: Condvar::new(),
            }),
        }
    }

    /// Test-controlled release: a blocked `hang` execution returns
    /// deterministic released output. Idempotent.
    pub fn release(&self) {
        *self.state.released.lock().expect("hang gate poisoned") = true;
        self.state.cvar.notify_all();
    }

    /// Canonical cancellation: a blocked `hang` execution exits as
    /// [`FakeAgentError::Cancelled`]; an already-decided execution is
    /// unaffected. Idempotent: repeated calls keep the cancelled state.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::SeqCst);
        self.state.cvar.notify_all();
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    /// Blocks until release or cancellation, then reports the decision.
    /// Correctness comes from the explicit test decision, never from the
    /// poll quantum.
    fn await_decision(&self) -> HangDecision {
        let mut released = self.state.released.lock().expect("hang gate poisoned");
        loop {
            if self.state.cancelled.load(Ordering::SeqCst) {
                return HangDecision::Cancelled;
            }
            if *released {
                return HangDecision::Released;
            }
            let (guard, _) = self
                .state
                .cvar
                .wait_timeout(released, HANG_POLL_QUANTUM)
                .expect("hang gate poisoned");
            released = guard;
        }
    }
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self::fresh()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HangDecision {
    Released,
    Cancelled,
}

/// Opaque transient session handle for one execution.
///
/// Minted per [`FakeAgent::execute`] (also available via
/// [`FakeAgent::start_session`] for inspection), derived deterministically
/// from the request identities plus the scenario. Dropped after use:
/// never persisted, never journalled, never a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeSessionHandle {
    session_ref: String,
    scenario: FakeScenario,
}

impl FakeSessionHandle {
    /// Opaque external-session-style reference (diagnostic only).
    #[must_use]
    pub fn session_ref(&self) -> &str {
        &self.session_ref
    }

    /// The scenario this session simulates.
    #[must_use]
    pub fn scenario(&self) -> FakeScenario {
        self.scenario
    }
}

/// Deterministic FakeAgent test double, configured for one scenario.
///
/// Stateless and `Copy`: all per-execution coordination travels through
/// the [`ExecutionControl`] passed to [`execute`](Self::execute), so
/// concurrent executions never share scenario state. Replaceable by
/// future `ClaudeAdapter`/`CodexAdapter`/… without touching Task domain
/// semantics: this type never sees Task state, storage, or the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeAgent {
    scenario: FakeScenario,
}

impl FakeAgent {
    /// Configures an isolated double for exactly one scenario.
    #[must_use]
    pub fn for_scenario(scenario: FakeScenario) -> Self {
        Self { scenario }
    }

    /// The configured scenario.
    #[must_use]
    pub fn scenario(&self) -> FakeScenario {
        self.scenario
    }

    /// Mints the transient session handle for `request`. Pure and
    /// deterministic: the same request always yields the same handle.
    #[must_use]
    pub fn start_session(&self, request: &ExecutionRequest) -> FakeSessionHandle {
        FakeSessionHandle {
            session_ref: format!(
                "fake-session:{}:{}",
                request.execution_id, request.attempt_id
            ),
            scenario: self.scenario,
        }
    }

    /// Runs one deterministic execution for `request`.
    ///
    /// Exactly one terminal behavior per call — never an internal retry.
    /// Success-family scenarios return [`ExecutionOutput`]; failure-family
    /// scenarios return a distinguishable [`FakeAgentError`]. `hang`
    /// blocks on `control` until the test releases or cancels it.
    /// No filesystem, process, network, Git, IPC, or persistence side
    /// effects on any path.
    pub fn execute(
        &self,
        request: &ExecutionRequest,
        control: &ExecutionControl,
    ) -> Result<ExecutionOutput, FakeAgentError> {
        // Transient session: minted, used for evidence derivation, dropped.
        let session = self.start_session(request);
        let evidence_ref = format!(
            "fake-evidence:{}:{}:{}",
            self.scenario.as_str(),
            request.execution_id,
            request.attempt_id
        );
        debug_assert_eq!(session.scenario(), self.scenario);

        match self.scenario {
            FakeScenario::Success => ExecutionOutput::new(
                format!(
                    "fake-agent success evidence for attempt {}",
                    request.attempt_id
                ),
                false,
                evidence_ref,
                false,
            )
            .map_err(|broken| FakeAgentError::MalformedOutput {
                detail: format!("adapter invariant broken: {broken}"),
            }),
            FakeScenario::Crash => Err(FakeAgentError::Terminated {
                reason: "simulated provider termination before result (no process crashed)"
                    .to_owned(),
            }),
            FakeScenario::Hang => match control.await_decision() {
                HangDecision::Released => ExecutionOutput::new(
                    format!(
                        "fake-agent hang released by test control for attempt {}",
                        request.attempt_id
                    ),
                    false,
                    evidence_ref,
                    false,
                )
                .map_err(|broken| FakeAgentError::MalformedOutput {
                    detail: format!("adapter invariant broken: {broken}"),
                }),
                HangDecision::Cancelled => Err(FakeAgentError::Cancelled),
            },
            FakeScenario::Timeout => Err(FakeAgentError::TimedOut {
                budget: "virtual timeout budget exhausted (no wall-clock sleep)".to_owned(),
            }),
            FakeScenario::MalformedResult => Err(FakeAgentError::MalformedOutput {
                detail: "simulated malformed provider output at adapter boundary (not trusted)"
                    .to_owned(),
            }),
            FakeScenario::ForbiddenPathWrite => Err(FakeAgentError::CapabilityDenied {
                capability: "fs.write".to_owned(),
                attempted: FORBIDDEN_PATH.to_owned(),
            }),
            FakeScenario::ForbiddenCommand => Err(FakeAgentError::CapabilityDenied {
                capability: "exec".to_owned(),
                attempted: FORBIDDEN_COMMAND.to_owned(),
            }),
            FakeScenario::QuotaReached => Err(FakeAgentError::QuotaExhausted {
                detail: "simulated quota exhausted (no retry performed)".to_owned(),
            }),
            FakeScenario::NetworkFailure => Err(FakeAgentError::NetworkFailure {
                detail: "simulated network failure (no connection attempted)".to_owned(),
            }),
            FakeScenario::PartialResult => ExecutionOutput::new(
                format!(
                    "fake-agent partial evidence for attempt {} (incomplete by construction)",
                    request.attempt_id
                ),
                true,
                evidence_ref,
                false,
            )
            .map_err(|broken| FakeAgentError::MalformedOutput {
                detail: format!("adapter invariant broken: {broken}"),
            }),
            FakeScenario::ReviewRejection => ExecutionOutput::new(
                format!(
                    "fake-agent evidence for attempt {} (evaluation fixture: rejectable)",
                    request.attempt_id
                ),
                false,
                evidence_ref,
                true,
            )
            .map_err(|broken| FakeAgentError::MalformedOutput {
                detail: format!("adapter invariant broken: {broken}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{AttemptId, ExecutionId};
    use task_domain::TaskId;

    fn request(task: &str, execution: &str, attempt: &str) -> ExecutionRequest {
        ExecutionRequest::new(
            TaskId::new(task).unwrap(),
            ExecutionId::new(execution).unwrap(),
            AttemptId::new(attempt).unwrap(),
        )
    }

    #[test]
    fn agent_is_stateless_copy() {
        let agent = FakeAgent::for_scenario(FakeScenario::Success);
        let copy = agent;
        assert_eq!(agent, copy);
        assert_eq!(agent.scenario(), FakeScenario::Success);
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FakeAgent>();
        assert_send_sync::<ExecutionControl>();
    }

    #[test]
    fn sessions_are_transient_deterministic_and_isolated() {
        let agent = FakeAgent::for_scenario(FakeScenario::Success);
        let first = agent.start_session(&request("T-1", "E-1", "A-1"));
        let replay = agent.start_session(&request("T-1", "E-1", "A-1"));
        let other = agent.start_session(&request("T-1", "E-1", "A-2"));
        assert_eq!(first, replay, "same request must mint the same handle");
        assert_ne!(first, other, "distinct attempts must not share a session");
        assert!(first.session_ref().contains("E-1"));
        assert!(first.session_ref().contains("A-1"));
    }

    #[test]
    fn cancellation_is_idempotent() {
        let control = ExecutionControl::fresh();
        assert!(!control.is_cancelled());
        control.cancel();
        control.cancel();
        assert!(control.is_cancelled());
    }
}
