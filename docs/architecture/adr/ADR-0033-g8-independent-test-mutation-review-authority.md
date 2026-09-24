# ADR-0033 — G8 Independent Test-Mutation Review Authority

Status: Proposed
Gate: G8
Scope: trusted review records required by ADR-0032 CV-9

## Context

ADR-0032 requires a durable independent review before `ScopedReviewed` test,
check or evaluator changes can support CV-9. The record binds the Task,
immutable base, captured candidate, changed-input manifest, affected checks and
policy. It requires reviewer != implementer but leaves reviewer authentication
and record authorship open. ADR-0020 authenticates only the local OS user;
same-user IPC, caller-supplied identity, provider claims and fixtures cannot
prove a reviewer principal. G6 authorization is separate from reviewer identity
and review evidence (ADR-0026).

## Proposed decision

`Forbidden` remains the default. Both review paths are selectable by a
canonical trusted owner-managed setting inside Runlith settings:

| Path | Required authority |
|---|---|
| **B1 — human** | Independently authenticated human and implementation principals; enforce reviewer != implementer; G6 authorization for the exact review; read-only access to the captured candidate and affected checks; trusted product capture and record authorship. |
| **B2 — service** | Independently authenticated reviewer service and implementation principals; enforce reviewer != implementer; governed G5/G6 Runner execution with the same exact read-only scope; trusted product capture and record authorship. |

The setting selects B1 or B2 at review admission. Policy binds its value and
revision, the selected path, and relevant profile/mapping and Policy revisions
to the review record and integrity assessment. The product resolves reviewer
configuration; callers, providers, repositories and implementation agents
cannot supply it. Setting changes require trusted owner control beyond
same-user IPC. Assignment is outside implementation control. Missing or
unprovable implementation identity, reviewer identity, owner authority, or
selected-path controls denies review. There is no fallback between paths.

### B1 principal sources and comparison

B1 uses two distinct typed principal namespaces, populated only from trusted
product records:

- `HumanPrincipal`: the operating-system account identity plus fresh
  interactive platform user-presence authentication, obtained by the trusted
  Runlith desktop/control host for this exact review or owner-setting
  operation. The product-owned process binds the authentication to a fresh
  operation nonce and the exact input digest. Same-user IPC authentication,
  `client_kind`, caller/provider text, and prior authentication alone do not
  establish this principal. If the trusted host cannot obtain or bind fresh
  user presence, the operation fails closed.
- `ImplementationPrincipal`: a product-minted principal for the governed G5/G6
  execution `Attempt`, bound to its Runner process and admission provenance.
  It is derived from trusted execution records, never caller/provider text,
  model labels, or an asserted identity.

The product compares these typed principals using trusted provenance that
resolves whether the authenticated human is the actor responsible for the
implementation Attempt, and rejects a match before assignment and review
admission. The principal namespaces remain distinct; the comparison is based
on trusted attribution records, never string or caller-asserted identity
equality. Provider output cannot acquire owner proof, choose the reviewer or
assignment, or supply either principal. Required G5/G6 isolation cannot use an
unsandboxed fallback. This contract selects no particular OS API, vendor, or
cryptographic design.

Both paths review the exact captured base, candidate, changed-input manifest
and affected checks read-only. They cannot write the candidate, mutate Git,
access implementation sessions or secrets, or grant capabilities. Any process,
network or tool access is separately authorized and enforced under G6 and the
applicable G5 Runner contract. Human approval remains a separate Policy
decision and is required only when Policy returns `REQUIRE_APPROVAL`.

The record binds the Task and requirement revision; base and candidate
identity/manifests; changed-input manifest and affected check/evaluator IDs;
governed invocation; selected `ScopedReviewed` policy; B1/B2 setting and
revision; profile/mapping and Policy revisions; implementation Execution,
Attempt and authenticated principal; reviewer principal and its trusted
authentication/admission provenance; verdict, bounded rationale/evidence
references, trusted capture time, and supersession provenance. Bindings use
the canonical encoding and digest requirements established by ADR-0032 and
the relevant existing contracts. No particular digest algorithm or storage
replacement mechanism is added here.

At authoring and use, Evaluation validates the exact bindings, trusted
provenance, reviewer inequality, current setting and policy, and that the
record is current and fresh. Records remain durable historical evidence;
replay, superseded records, stale or changed bindings, missing evidence,
rejected verdicts, and uncertain freshness fail closed. Any changed binding
requires a new review and assessment. Evaluation persists history and
invalidation state through its ports; storage mechanics remain an
implementation choice. A failed validation leaves changed checks ineligible,
so dependent verification cannot be `VERIFIED` and completion refuses.

### B2 availability

B2 may be selected even when its execution prerequisite is unavailable. Its
execution remains disabled until
`PREREQUISITE-G5-G6-AUTHENTICATED-GOVERNED-REVIEWER-SERVICE` is separately
demonstrated: an authenticated service principal and credential path usable by
a supported confined Runner, with G5 provenance and G6 authorization. No host
execution, ambient-credential, unconfined, or B1 fallback is allowed.

TASK-0145 must be completable with B1 implemented as a secure working path,
`Forbidden` unchanged, and B2 selectable but visibly blocked when its
prerequisite is absent. TASK-0145 does not have to demonstrate B2 execution.
If tracking B2 activation requires a task, it is a distinct future task
dependent on the named prerequisite and does not block TASK-0145 or G8.

### Ineligible alternative

Product-controlled fresh execution with a distinct Runner instance (Option A)
does not prove reviewer != implementing agent under ADR-0032. It is not
selectable absent an explicit ADR-0032 amendment. Task, Execution or Attempt
identity alone does not prove agent identity. Provider/model labels, requested
profile IDs, reviewer text and output claims are not identity proof.

## Alternatives

- Caller-declared reviewer identity or provider output as attestation: rejected
  because implementation-controlled claims cannot establish independence.
- Human review for every test/check change: rejected; it adds a universal
  approval gate ADR-0032 does not require.
- Option A fresh execution: ineligible for the identity reason above.

## Consequences and security impact

- `Forbidden` remains effective by default; unavailable or invalid selected
  review authority leaves `ScopedReviewed` ineligible.
- B1 and B2 require independent principal proof, exact review binding, G6
  authorization and read-only scope. B1 obtains fresh human presence through
  the trusted host and derives the implementation principal from governed
  Attempt records. B2 additionally requires governed G5 execution; selection
  alone grants no authority.
- Review evidence, test results and optional human approvals remain distinct.
  Review cannot satisfy a separate approval requirement.
- Evaluation authors trusted records; Runner enforces already-authorized
  execution scope and does not author policy or verdicts. Task completion
  consumes validated evidence through existing boundaries.
- No provider-specific review semantics, direct cross-context table reads, or
  secret payloads are introduced.

## Migration and implementation obligations

This proposal makes no code or schema change. TASK-0144 retains its objective,
scope, exit criteria and TASK-0146 dependency. It may implement the governed
test producer and integrity assessment with `Forbidden` effective and
`ScopedReviewed` failing closed until a valid review exists.

After ADR-0033 acceptance, TASK-0145 implements the canonical setting, the
secure B1 path, B2 selection and blocked state, durable review evidence,
validation, replay/invalidation behavior and focused checks. It depends on
TASK-0144's candidate-manifest, invocation and integrity-assessment contract,
not on B2 activation. B2 remains disabled until its named prerequisite is
separately demonstrated; this does not prevent TASK-0145 completion. No
fallback is permitted. TASK-0125 consumes only validated current integrity
assessments and typed review references; it cannot synthesize review records
or use review to discharge distinct approval requirements.

## Relationship to accepted decisions

This proposal specifies authorship and identity for ADR-0032's independent
review requirement without changing CV-9, the `Forbidden` default, the
`ScopedReviewed` policy state, or G6 approval rules. ADR-0018's separation of
Evaluation and Policy and ADR-0026's authority contracts remain in force. This
proposal is not owner acceptance; its status remains Proposed.

## References

- [ADR-0018 — Independent Evaluation](./ADR-0018-independent-evaluation.md)
- [ADR-0020 — Local IPC Peer Authentication](./ADR-0020-local-ipc-peer-authentication.md)
- [ADR-0024 — G5 Provider Execution Seam](./ADR-0024-g5-provider-execution-seam.md)
- [ADR-0026 — G6 Security Authority and Capability Contracts](./ADR-0026-g6-security-authority-and-capability-contracts.md)
- [ADR-0030 — G8 Observed Effect / Claim Verification](./ADR-0030-g8-observed-effect-claim-verification-contract.md)
- [ADR-0032 — G8 Test Integrity and Trusted Evidence](./ADR-0032-g8-test-integrity-evidence-authoring.md)
- [TASK-0144 and TASK-0145](../../../planning/tasks/queue.yaml)
