# evaluation: derived navigation

> Generated: 2026-09-30. Source baseline: `5362f6e`.
> Derived guidance only; canonical policy: [AGENT.md](../../AGENT.md).

Provider-neutral evaluation, capture and immutable-candidate contracts.
[Cargo.toml](Cargo.toml) has domain/runtime and digest dependencies; concrete
capture, execution and persistence composition lives outside this crate.

## Source map

| Work | Primary source |
| --- | --- |
| Independent verdict and final freshness hook | [port.rs](src/port.rs), [verdict.rs](src/verdict.rs) |
| Task/Execution/Attempt-bound evaluation input | [request.rs](src/request.rs) |
| Capture intents, observations and adapter ports | [capture.rs](src/capture.rs) |
| Closed effect kinds, payloads, provenance and fingerprints | [observed_effect.rs](src/observed_effect.rs) |
| Verified Git bytes, manifests, selectors and integrity records | [candidate.rs](src/candidate.rs) |

## Authority and evidence

- `EvaluationPort::evaluate` returns Verified, Rejected or an error. The
  [orchestration service](../app-services/src/orchestration_service.rs) applies
  lifecycle changes through TaskService; an error leaves VERIFYING, Rejected
  routes to retry, and Verified first passes `revalidate_before_completion`.
  The generator must not evaluate its own output; the evaluator never writes
  Task state or journal records: [ADR-0018](../../docs/architecture/adr/ADR-0018-independent-evaluation.md).
- [ADR-0030](../../docs/architecture/adr/ADR-0030-g8-observed-effect-claim-verification-contract.md)
  separates untrusted AgentClaim, trusted EvidenceRequirement, captured
  ObservedEffect and durable ClaimVerification. An observed effect alone is
  not a completion verdict, and an execution receipt is not proof of correctness.
- `ObservedEffect::restore` validates kind/payload consistency and timestamp;
  constructing a value does not cross the persistence authority boundary.
  [ObservedEffectService](../app-services/src/observed_effect_service.rs) resolves
  lineage, captures facts and persists them through trusted storage operations.

## Immutable candidate limits

- `VerifiedCandidate` constructors verify Git object framing/hashes and build
  manifests from raw blob bytes. The parser supports regular/executable files
  and trees; symlink/gitlink modes, case collisions and unsafe paths fail closed.
- `CandidateSelector::from_requirement` accepts only the required TestSuite
  `candidate.file-contains.v1` selector with exact literal/path/selector parameters,
  content/toolchain/config binding and policy fingerprint. `CandidateTestResult::file_contains`
  inspects the manifest-bound file; it does not compile or run candidate tests.
- `CandidateIntegrityAssessment::validate` checks invocation digest, lineage,
  policy/requirement bindings, Runner enforcement and mandatory freshness
  checkpoints. Restart reuse additionally requires a completion checkpoint.
  The immutable-input contract and supported capture boundary are in
  [ADR-0035](../../docs/architecture/adr/ADR-0035-g8-immutable-git-candidate-proposal.md).

## Focused checks

Run from the repository root; the unit modules are in
[candidate.rs](src/candidate.rs) and [observed_effect.rs](src/observed_effect.rs).

```sh
rtk cargo test -p evaluation --lib candidate::tests
rtk cargo test -p evaluation --lib observed_effect::tests
```
