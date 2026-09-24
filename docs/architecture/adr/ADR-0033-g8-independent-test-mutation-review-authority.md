# ADR-0033 — G8 Independent Test-Mutation Review Authority

Status: Proposed
Gate: G8
Scope: trusted review records required by ADR-0032 CV-9

## Context

ADR-0032 selects `Forbidden` as the default `TestMutationPolicy` and permits
`ScopedReviewed` changes only with a durable independent review bound to the
Task, base, candidate, changed-input manifest, affected checks and policy
fingerprint. It does not say who can author that record or what proves reviewer
identity. The existing local IPC principal is only the OS user (ADR-0020); the
authenticated client can be arbitrary same-user code. G6 approval is a
separate, policy-triggered human decision (ADR-0026), not reviewer identity or
review evidence. A caller-supplied reviewer string, fixture, provider claim,
review result receipt, or test result therefore cannot close CV-9.

## Proposed decision

This ADR remains Proposed. Both B1 and B2 are selectable through one canonical
trusted user-managed setting inside Runlith settings. `ScopedReviewed` remains ineligible until the
setting is selected through trusted owner control, the implementation identity
source is established, and the selected path satisfies its G5/G6 controls.
ADR-0032's accepted reviewer-independence rule and `Forbidden` default remain
in force meanwhile.

### Reviewer authority

| Option | Assurance and consequence |
|---|---|
| **B1 — independently authenticated human reviewer** | A human authenticated independently of the implementation executor reviews through a trusted product control-plane flow. The control plane enforces reviewer principal != implementation principal, obtains G6 authorization for the exact review operation, presents only the captured candidate and affected checks read-only, captures the verdict, and the product authors the record. |
| **B2 — independently authenticated reviewer service** | A reviewer service authenticated independently of the implementation executor enforces reviewer principal != implementation principal and reviews through governed G5/G6 Runner execution with the same exact read-only scope. |

The trusted owner-managed setting inside Runlith settings has one canonical
value, `B1` or `B2`, and
is the only product setting that selects the review authority path. A fixed
product-managed reviewer profile/key (or minimal Policy mapping) supplies the
principal/configuration for that selected path; callers, providers,
repositories, and implementation agents cannot supply or override it. Policy
reads and resolves the setting at review admission and binds the setting
revision/value, profile or mapping revision, and Policy revision/fingerprint
to the admission and exact review record. Setting changes require the
authenticated trusted owner control path; same-user IPC authentication under
ADR-0020 alone does not establish owner authority. The selected review path
must independently authenticate its reviewer principal and prove its
inequality with the implementation principal. Assignment stays outside
implementation control.

Missing, malformed, ambiguous, disabled, revision-mismatched, or unavailable
selected setting/profile/path fails closed. There is no fallback to the other
path. Switching B1/B2 invalidates existing review records and integrity
assessments; they must be recreated and reassessed under the current bound
setting, profile/mapping, and Policy revisions. A setting chooses the path but
does not create reviewer identity, G6 authority, or an available execution
backend.

#### Rejected/future-amendment alternative: Option A

Product-controlled fresh execution may admit a new governed Runner execution
for the captured candidate with a distinct product-minted executor-instance
identity and no implementation process handle, Attempt, session identifier,
resume token, or implementation context. This establishes separate product
admission and process execution, but does not prove reviewer != implementing
agent. It is ineligible under accepted ADR-0032 and is not a selectable
setting. It could be reconsidered only by an explicit future ADR-0032
amendment defining and accepting a revised assurance standard.

The current Task/Execution/Attempt identities do not identify an agent
instance. Option A's product-controlled admission and distinct Runner process
cannot establish ADR-0032's accepted requirement that the reviewer is not the
implementing agent. It remains ineligible without an explicit ADR-0032
amendment. B1 and B2 must establish both principals and enforce their
inequality independently of implementation control. If the implementation
principal is absent or unprovable, deny. Distinct minted Task/Execution/Attempt
IDs alone satisfy neither path.
Provider/model labels, requested profile IDs, reviewer text and output claims
are not identity proof. Provider output remains untrusted opinion data.

For either path, Policy resolves the permitted reviewer profile and
Evaluation resolves implementation provenance from product-authored
application records. Under B1,
the trusted control plane independently authenticates the human and
implementation principal, enforces their inequality and G6 authorization for
the exact review operation, presents the captured candidate and affected
checks read-only, captures the human verdict, and has Evaluation author the
bound record. Under Option B2, the reviewer service authenticates
independently, proves its principal differs from the implementation principal,
and uses governed G5/G6 Runner execution with the same read-only scope;
Evaluation captures its bounded verdict and authors the record. In both B
paths, implementation-controlled assignment is forbidden and an absent or
unprovable implementation principal denies review. IPC clients cannot create
review authority or author the record. Same-user IPC proves only OS-user
equality; it does not identify a reviewer principal.

### Policy profile and scope

Policy maps the setting's selected path to a fixed product-managed reviewer
profile/key and binds its revision. This minimal mapping has exactly one
enabled profile per path; it is not a caller-configurable policy language.
The profile means: review only the exact captured base, candidate,
changed-input manifest and affected checks; read-only source access; no
candidate write, Git mutation, implementation-session access, or secret
access. Any process, network or tool capability required by the adapter is
separately authorized by G6 for that exact operation; the profile cannot
grant it. Provider configuration stays behind its adapter.

### Freshness

The fixed maximum age is 24 hours from trusted review-record capture time to
integrity-assessment selection. Changes to Task, criterion/check set, base,
candidate, changed-input manifest, invocation, implementation attempt,
requirement revision, setting, profile/mapping, or Policy revision
invalidates the record immediately. Missing bound or uncertain clock fails
closed. No value is runtime-configurable by caller, repository, or provider.

### Governed execution prerequisite for automated review

The canonical setting must always allow the owner to select B1 or B2; selecting
B2 does not imply that its execution prerequisite is available. Option B2
(automated reviewer service) requires the accepted governed Runner/G6 path.
Current source does not establish that an authenticated reviewer service can
run there: on macOS, `agentd` explicitly refuses authenticated Claude execution
under mandatory confinement because its provider-owned Keychain path requires
withheld authority; the current NativeRunner reports no isolation axes and
cannot satisfy a confined launch on platforms without a supporting backend.
The stable prerequisite identifier is
`PREREQUISITE-G5-G6-AUTHENTICATED-GOVERNED-REVIEWER-SERVICE`: establish an
authenticated service principal and credential path usable by a supported,
confined Runner, with G5 execution provenance and G6 authorization. This names
the missing contract only; it selects no provider or credential mechanism and
creates no task. TASK-0145 must verify that end-to-end path. It must not fall
back to host execution, inherited ambient credentials, or provider-native
permission claims. Until this prerequisite is satisfied and verified,
TASK-0145 remains blocked and cannot be completed. Option B1 does not require a
provider or Runner process;
it still requires an independently authenticated human, G6 authorization for
the exact review operation, scoped read-only candidate access, and trusted
product capture/authorship. Failure of those controls also fails closed.

This review is separate from optional G6 human approval. Review verdict,
structured test result, execution receipt and approval remain distinct
evidence. Human approval is required only when Policy independently returns
`REQUIRE_APPROVAL`; it cannot supply a missing independent-review record.

The record binds, canonically and immutably:

- Task ID and immutable acceptance criterion IDs, affected check IDs and
  requirement revision;
- evaluated base commit and content manifest digest;
- captured candidate manifest digest, candidate snapshot identity, and
  governed invocation digest;
- complete changed-input manifest (path, entry type, content digest, and
  added/deleted/modified status) and the exact affected check/evaluator IDs;
- selected `ScopedReviewed` policy revision and fingerprint;
- selected B1/B2 path, canonical setting value/revision, profile or mapping
  revision, and Policy revision/fingerprint in both the review record and the
  resulting integrity assessment;
- implementation Execution/Attempt, product-authored executor provenance and
  authenticated implementation principal used to enforce reviewer inequality;
- the review profile key/revision, setting value/revision, Policy
  revision/fingerprint, and product-authored review provenance; under B1, the
  independently authenticated human principal and control-plane review
  operation; under B2, the
  independently authenticated service principal and governed Runner
  Execution/Attempt provenance;
- structured review verdict (`ACCEPT` or `REJECT`), bound to the authenticated
  reviewer and implementation principals, bounded rationale/evidence
  references, product capture time, and freshness/supersession links.

All digests use a versioned canonical encoding and SHA-256. The selected
review path receives read-only access to that captured base/candidate and
manifest. The trusted review composition validates the human or automated
verdict and binds it to the authenticated principal and exact review input; for
automated execution it also binds the result to the admitted Runner execution
and treats provider/model output as untrusted data. Only the application
service mints record identity, timestamps, bindings, and durable provenance.
Review output cannot choose or rewrite any binding field.

### Validation, freshness and replay

At authoring, Evaluation validates the trusted source for the selected path:
for B1, the independently authenticated human and implementation principals,
their enforced inequality, authorized control-plane operation and captured
verdict; for B2, the independently authenticated service and
implementation principals, their enforced inequality and governed Runner
result. It confirms exact captured input digests and setting/profile/Policy
revisions and validates the bounded structured verdict. Provider-session freshness is
not claimed absent trusted adapter evidence.
Failure at any check authors no acceptable record. At consumption, Evaluation
resolves product-authored provenance through ports and rechecks current
policy/profile, setting value/revision, profile/mapping revision, Policy
revision, implementation and reviewer principal/source,
their inequality, every binding, freshness, and supersession state. No caller
field can substitute for these reads. Missing, malformed, unprovable, denied,
rejected, stale, or mismatched record makes changed checks ineligible; the
assessment is blocked/unverified or tampered as appropriate, dependent
verification cannot be `VERIFIED`, and completion refuses. Receipts and
provider claims never substitute for review.

Review records are repeat-readable evidence, not consumable bearer authority.
For a binding tuple, storage maintains one current record reference. A new
review replaces it atomically: one transaction validates the new record,
marks the prior current record superseded, installs the new current reference,
and appends provenance; failure leaves the prior state intact. Evaluation
selects and pins the current record reference and the selected
setting/path/profile/Policy revisions in its integrity assessment; Task
completion revalidates those bindings, the record reference, and its current
status in the existing evidence snapshot transaction. Reading or
referencing a record does not delete or consume it. A superseded record remains historical and cannot
support a newly created assessment. Changes to Task/criterion/check, base, candidate, manifest, invocation,
setting/profile/Policy revision, or implementation attempt require a new bound
review. Expiry is the fixed maximum age from product capture time to
assessment selection; changed bindings invalidate
immediately regardless of age. Unknown age rule or uncertain clock fails
closed. Audit provenance retains record ID, all binding digests, authoring
service, reviewer admission/process provenance (and independently
authenticated principal), verdict, capture time,
and atomic supersession relation without secrets or unrestricted transcript
payloads.

### Ownership boundaries

| Responsibility | Owner |
|---|---|
| `Forbidden` / `ScopedReviewed`, allowed paths/checks, selected profile key/revision, selected B1/B2 setting value, and fixed freshness bound | Policy Engine; canonical policy revision/fingerprint |
| Review request admission, durable Task/Execution identity resolution, record validation and record authorship | Trusted Evaluation/application service through explicit ports |
| Automated review execution, Runner admission/process provenance and read-only candidate delivery; enforce the authorized runtime boundary | Runner via existing governed execution composition; no policy decisions or durable review authorship |
| Human review presentation and authenticated verdict capture through the exact G6-authorized control-plane operation; read-only candidate delivery | Trusted control-plane/application service; no provider or Runner execution and no durable review authorship |
| Candidate/base and invocation capture; integrity assessment and review record persistence | Trusted application composition and its storage adapter, each through explicit ports; one canonical owner per durable fact |
| Test result capture | Existing ADR-0032 trusted Evaluation/Runner producer; a test result cannot create a review record |
| Optional human approval | G6 Policy/Approval service, only for the exact bound operation when Policy requires it |
| Task completion admission | Trusted Task/application transaction consumes validated evidence references; it does not read Evaluation or Policy tables directly |

Evaluation obtains policy decisions and durable review references through
ports/contracts, never direct cross-context table reads. Task application
receives typed validated evidence references through its existing boundary;
Runner enforces an already-decided scope close to automated execution and
does not author policy or verdicts. No provider-specific review semantics enter
core types. Same-user IPC authentication allows a control request to be
considered but cannot itself authenticate a human reviewer or create a review.
Only the authenticated control-plane flow can capture a human verdict, and
only the Evaluation application service can author its bound record. Direct
caller fields, fixtures and repository instructions have no authoring
capability.

## Alternatives

- **Caller-declared reviewer identity or provider output as attestation:**
  rejected; the implementation agent or any same-user client could self-assert
  another reviewer, violating INV-114 and CV-9.
- **Require a human for every scoped test/check edit:** rejected; it adds a
  universal approval gate ADR-0032 explicitly avoids. Human approval remains
  a separate Policy decision.
- **Keep `Forbidden` effective until review is available:** required fail-closed
  behavior. If an automated reviewer is selected and no supported governed
  execution path exists, `TASK-0145` remains blocked by
  `PREREQUISITE-G5-G6-AUTHENTICATED-GOVERNED-REVIEWER-SERVICE`; G6 requirements
  are not relaxed. B2 remains selectable while execution is blocked.

## Migration and implementation obligations

This proposal makes no code or schema change. TASK-0144 retains its current
objective, scope, exit criteria and TASK-0146 dependency. It may implement the governed test
producer and integrity assessment while keeping `Forbidden` as the effective
default and making `ScopedReviewed` fail closed until an acceptable review
record exists. TASK-0145 implements the canonical setting, B1/B2 selection,
review records, validation and focused checks. It depends on TASK-0144's
candidate-manifest, invocation and integrity-assessment contract. TASK-0144
depends on TASK-0146 and does not depend on TASK-0145. TASK-0145 implementation requires ADR-0033
acceptance. Option A remains ineligible under current ADR-0032. B2 requires an
available authenticated governed G5/G6 path. The current macOS provider-auth
path is unavailable, so B2 remains selectable but blocked by
`PREREQUISITE-G5-G6-AUTHENTICATED-GOVERNED-REVIEWER-SERVICE`; TASK-0145 cannot
complete until that prerequisite is satisfied and an end-to-end B2 path passes
G5/G6. It cannot fall back. B1 requires independent human authentication,
implementation-principal inequality, G6 authorization, assignment outside
implementation control and scoped read-only access. Both require owner-setting
spoof refusal, exact setting/profile/Policy and review-input binding, path
switch invalidation, replay and atomic replacement checks, missing/rejected
review blocking `ScopedReviewed`, unchanged `Forbidden`, restart durability,
and no host-auth or unconfined fallback.

TASK-0125 later consumes only the current durable integrity assessment and
typed review evidence reference when validating CV-9 and claim verification.
It must not synthesize records in tests, read review storage directly, accept
review output as a verdict, or let review discharge a distinct human approval
requirement.

## Relationship to accepted decisions and owner decision

This proposal clarifies ADR-0032's required independent record and reviewer
inequality; it does not weaken CV-9, alter either policy state, make review a
test result, or add universal human approval. ADR-0018's separation of
Evaluation from Policy and ADR-0026's authority/approval rules remain in
force. The owner decision selects B1 or B2 through the canonical setting. The
fixed profile mapping and 24-hour freshness bound are proposed contract details;
this does not constitute owner acceptance of the ADR. `ScopedReviewed` remains
fail-closed until the ADR is accepted and the selected path's identity, G6 and
read-only controls are implemented. B2 remains unavailable on the current
macOS authentication path. Its named prerequisite must be satisfied, and
TASK-0145 must demonstrate a functioning authenticated governed B2 path under
G5/G6 before completion; selecting B2 never permits fallback to B1. B1 likewise cannot be
enabled until its independent human authentication, G6 authorization and
read-only control-plane scope are established.

## References

- [ADR-0018 — Independent Evaluation](./ADR-0018-independent-evaluation.md)
- [ADR-0020 — Local IPC Peer Authentication](./ADR-0020-local-ipc-peer-authentication.md)
- [ADR-0024 — G5 Provider Execution Seam](./ADR-0024-g5-provider-execution-seam.md)
- [ADR-0026 — G6 Security Authority and Capability Contracts](./ADR-0026-g6-security-authority-and-capability-contracts.md)
- [ADR-0030 — G8 Observed Effect / Claim Verification Contract](./ADR-0030-g8-observed-effect-claim-verification-contract.md)
- [ADR-0032 — G8 Test Integrity and Trusted Verification Evidence](./ADR-0032-g8-test-integrity-evidence-authoring.md)
- [INVARIANTS.md](../INVARIANTS.md): INV-009, INV-025/026, INV-070/072/092/114, INV-146..153; ADR-0030 CV-1..CV-9
- [TASK-0112 provider-auth diagnosis](../../../docs/development/G7_TASK_0112_PROVIDER_AUTH_DIAGNOSIS.md): macOS provider-owned Keychain constraint
- [TASK-0115 G7 closure re-audit](../../../docs/development/G7_TASK_0115_FINAL_CLOSURE_REAUDIT.md): fail-closed production provider-auth result
- `bins/agentd/src/execution.rs`: mandatory-confinement authentication refusal; `crates/runner/src/lib.rs`: NativeRunner reports no isolation axes
- [TASK-0144](../../../planning/tasks/queue.yaml)
- [TASK-0145](../../../planning/tasks/queue.yaml)
