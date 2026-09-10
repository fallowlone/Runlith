//! Deterministic FakeAgent test double (TASK-0036, G3).
//!
//! Adapter-scoped simulation of the provider-facing execution seam: given a
//! provider-neutral [`ExecutionRequest`] plus an adapter-owned [`FakeScenario`]
//! selector, it returns deterministic execution behavior — a valid
//! [`ExecutionOutput`] or a distinguishable [`FakeAgentError`] — without any
//! real process, shell, filesystem mutation, network, Git, LLM, or provider
//! SDK.
//!
//! Explicitly non-canonical: [`FakeScenario`] names never enter
//! `AttemptOutcome`, `EventType`, Task lifecycle, or SQLite. [`ExecutionRequest`]
//! is the same minimal shape future real adapters receive (durable
//! identities only); the scenario selector wraps it at the adapter/test
//! layer. Mapping to [`agent_runtime::AttemptOutcome`] belongs to future
//! orchestration (`RuntimeService::conclude_attempt`), never to this crate.
//!
//! Hard separations (tested): no dependency on storage, SQLite,
//! application services, IPC, or Task mutation — `cargo tree -p fake-agent`
//! must show only `agent-runtime` and `task-domain`. Session handles are
//! opaque in-memory values; nothing here persists.

mod agent;
mod request;
mod result;
mod scenario;

pub use agent::{ExecutionControl, FakeAgent, FakeSessionHandle, FAKE_PROVIDER_REF};
pub use request::ExecutionRequest;
pub use result::{ExecutionOutput, FakeAgentError};
pub use scenario::FakeScenario;
