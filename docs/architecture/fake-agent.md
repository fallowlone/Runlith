# FakeAgent — Deterministic Test Double Contract

> Repository-local contract for FakeAgent, the mandatory deterministic test
> double that precedes any real provider. Documents accepted architecture;
> invents no scenarios; implements nothing.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Task lifecycle: [`task-engine.md`](./task-engine.md)
> - Event semantics: [`events.md`](./events.md)
> - Provider contract: [`providers.md`](./providers.md)
> - Runner contract: [`runners.md`](./runners.md)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§15–16,
>   G3, ZeroClaw-derived §§4–5

## 1. Purpose

FakeAgent is mandatory before real provider implementation. Its purpose is
NOT to simulate intelligence realistically. Its purpose is to
deterministically exercise orchestration, failure handling, Task lifecycle
integration, Runner/provider contracts, recovery, and verification routing —
without a real LLM, external provider, network dependency, or provider
subscription. The full orchestration pipeline must pass without real
intelligence.

## 2. Architectural role

FakeAgent conceptually satisfies the provider-facing execution seam required
by the orchestration pipeline, with required behavioral compatibility. The
exact physical placement is left open where current architecture does not
specify it: whether the final Rust implementation is an `AgentProviderPort`
implementation, a special deterministic runtime, or a test adapter wrapping
`RunnerPort` is NOT decided here.

## 3. Required scenarios

Accepted scenarios — exactly these, no renames, no removals, no additions
to the required set:

```text
success
crash
hang
timeout
malformed_result
forbidden_path_write
forbidden_command
quota_reached
network_failure
partial_result
review_rejection
```

Additional future scenarios may be suggested only under Open FakeAgent
Design Items (§8), never as accepted requirements.

## 4. Determinism

FakeAgent behavior is deterministic and configurable enough for reliable
automated tests: the same configured scenario must not randomly change
semantics. No random AI-like behavior as the default.

## 5. Scenario semantics

- **success** — exercises dispatch, execution, submission,
  review/verification, and completion decision through the normal pipeline.
  FakeAgent still never sets `Task = COMPLETED` directly; it only produces
  whatever result/evidence the pipeline consumes.
- **crash** — provider/runtime execution disappears or fails unexpectedly.
  Tests that Attempt failure != automatic Task failure, plus
  recovery/retry orchestration where policy permits. Retry policy itself is
  not defined here.
- **hang** — execution makes no progress. Enables timeout-policy testing;
  no timeout duration is chosen here — scheduler/runtime timeout policy
  belongs elsewhere.
- **timeout** — a bounded timeout result. Whether timeout causes retry,
  reroute, FAILED, or BLOCKED is Reaction/orchestration policy, not
  FakeAgent semantics.
- **malformed_result** — invalid provider/model output is not accepted as
  authoritative Task result. No final TaskResult schema is defined here.
- **forbidden_path_write** — attempted write outside authorized scope,
  testing architecture boundaries. The Policy/Runner/verification pipeline
  detects/rejects it per accepted architecture. FakeAgent is never taught
  to bypass enforcement, and no attack techniques are implemented.
- **forbidden_command** — command disallowed by Policy. A textual FakeAgent
  claim of `"I did not run it"` is not security evidence; trusted
  runtime/Policy evidence determines the result.
- **quota_reached** — provider-capacity/quota exhaustion exercising
  provider-neutral availability/failure handling. Task semantics are never
  mutated by this scenario.
- **network_failure** — provider/network failure, deterministic and local,
  never dependent on real internet failure.
- **partial_result** — incomplete work/result behavior. Partial output !=
  Task completion; the pipeline preserves useful evidence/artifacts without
  falsely marking completion.
- **review_rejection** — independent review rejection, exposing the
  event/result needed to test eventual policy. FakeAgent never decides how
  Task lifecycle transitions after rejection: TASK-0006 marked that
  transition unresolved, and this document does not resolve it.

## 6. Normal authority path

FakeAgent must NOT bypass normal architecture. It never directly writes
Task DB rows, marks Tasks complete, writes the Event Journal manually,
modifies Memory, changes Git state outside Workspace/Git interfaces, skips
the Policy Engine, or injects privileged state into tests. The point is to
test the real orchestration seams. FakeAgent output is untrusted like any
provider output: it cannot fabricate a valid trusted Tool Execution Receipt
(receipt generation belongs to the trusted runtime/tool boundary), mint
authoritative domain events, or grant completion.

## 7. Independence of failure layers

Test architecture must eventually permit fake provider behavior and Runner
behavior/failure to be tested independently where practical. Provider crash
and container-backend-unavailable are different failures and must not be
conflated unless a test explicitly models their interaction.

## 8. Open FakeAgent Design Items

Unresolved by accepted architecture; listed, not decided (future scenarios
may be proposed here, never as accepted requirements):

```text
physical crate/module location
whether FakeAgent directly implements AgentProviderPort
scenario configuration format
deterministic timing controls
hang cancellation mechanism
malformed-result representation
partial-result representation
how policy-violation scenarios are injected without bypassing enforcement
test artifact fixture model
provider-vs-runner failure injection separation
scenario scripting/composition
reproducibility seed concept if ever needed
```

## 9. Non-goals

This document does NOT define: any real provider integration, provider
credentials, containers or backends, Scheduler or routing implementation,
Task retry/cancellation policy, receipt or Artifact Store implementations.
