# ADR-0052 - Extensions are out-of-process adapters under policy and sandbox

Status: Proposed. Needs owner ratification before any G17 implementation task
starts (ADR-0019: this adopts an ACTIVE DECISION only for the first slice; the
marketplace stays a DEFERRED ARCHITECTURE TARGET).
Date: 2026-11-08

## Context

The owner wants parts of Runlith to be easy to replace, third parties to add
their own intermediate stages or extend existing ones, and later a marketplace.

What already holds (checked against the code, not the documents):

- Ports and adapters are real. `ExecutionPort` has production implementations
  in `provider-claude` (`ClaudeExecutor`) and test doubles elsewhere; the
  `execution-contract` crate holds a conformance harness that takes
  `&dyn ExecutionPort`, which is the existing way to check a swapped-in
  adapter. INV-027, INV-069 and INV-082 require adding or replacing a
  provider, tool, runner or memory backend without touching unrelated domain
  logic.
- Composition is compile-time. `OrchestrationService` takes
  `&impl ExecutionPort` and `&impl EvaluationPort`; nothing loads code at run
  time (no dynamic library, no WebAssembly host). `dyn` ports appear only in
  the conformance harness.
- The IPC client set is closed. `ipc-control` recognizes exactly `tauri-host`
  and `agentctl` through one global check, and anything else is refused. The
  code documents `client_kind` as descriptive metadata, never authentication
  or authorization (ADR-0020).
- `TaskState` is a closed enum of 12 states. INV-083 says workflows never
  replace the durable Task lifecycle; adding a state would be a core change
  this ADR does not propose.
- ADR-0026 already states that plugins are untrusted, defines the capability
  vocabulary, monotonic delegation and fail-closed behavior, and says a runner
  security profile must never become a second authorization model.
- COVERAGE.md lists "Tools / Skills registry" and "Procedure Engine" as
  DEFERRED ARCHITECTURE TARGET with no ADR, and "Extension-contract
  architecture" as covered only while the surface fits the current ports.

So the gap is the mechanism: who may run third-party code, how it talks to
agentd, what it may touch, and how it is installed and trusted.

## Decision

1. **An extension is an out-of-process executable, never code loaded into
   agentd.** No dynamic library and no in-process WebAssembly host in v1.
   agentd starts it under the Runner/Sandbox contract (ADR-0026 `IsolationPlan`,
   INV-077). If the backend cannot enforce the manifest's capabilities, the
   start fails closed; there is no unsandboxed fallback.

2. **The channel is created by agentd, not discovered by the extension.**
   agentd makes a socket pair and gives the child its end as an inherited
   descriptor on a dedicated descriptor number (not stderr), borrowing the idea
   of the candidate launch (`candidate_authorization`: parent verified, nonce
   bound to arguments and input). That launch is a one-shot exchange, so a
   framed bidirectional protocol is new work. Two controls make the channel
   trustworthy, and `client_kind` is neither of them:
   a. the sandbox denies the extension any connection to the public agentd
      socket (a socket-connect denial that must be proven by a test, see the
      first slice), and the extension is never told its path;
   b. the listener on the agentd-created channel accepts the client kind
      `extension`, while the public listener keeps refusing it. This needs a
      per-listener registry, not a new entry in the global one, because a
      same-UID client on the public socket could otherwise send the string.
   ADR-0020 peer authentication and ADR-0050 user presence stay unchanged and
   extensions cannot trigger Run, Resume or Shutdown. The first slice is
   macOS-only, like the sandbox runner.

3. **The manifest is data, never code** (the `widget-spec` pattern): id,
   version, publisher, executable digest, supported extension-contract version
   range, extension points provided, and requested capabilities. Capabilities
   come only from the ADR-0026 vocabulary with exact scopes, no wildcards. A
   closed validator rejects anything else. The manifest compiles to a
   `PolicyRequest`. The grant is a root grant from the owner, not a delegation
   from a parent: the extension's authority is the resulting
   `EffectiveCapabilities`, bounded by the approved manifest, and it may further
   narrow but never widen what it was granted (INV-147). Hard policy denial
   cannot be weakened by approval or configuration (INV-148), and nothing the
   extension sends can grant authority (INV-146).

4. **Install and update need approval bound to the manifest.** First install
   and any change of executable digest or requested capabilities is
   `REQUIRE_APPROVAL` for the exact manifest hash. The digest covers an immutable
   bundle (the executable and everything it loads); agentd installs that bundle
   into a directory the sandbox can read but not write, verifies the digest on
   that copy and runs the copy, so a swap after the check is not possible. A
   bundle that needs files outside it is refused. Install and approval are
   audited (INV-010) (ADR-0026, TASK-0062
   semantics: not a bearer token, consumed once). The `BuiltInHard` and
   `UserManaged` layers decide; repository or workspace configuration can only
   restrict, so a cloned repository cannot enable an extension.

5. **Extension points are a closed, versioned list in the contracts layer**
   (INV-069), negotiated through the existing handshake mechanism as a
   capability identifier such as `extension-contract-v1`. The protocol number
   does not change for additive points. Points, in order of delivery:
   a. an existing port served from outside (ExecutionPort first), so a
      third party can add a provider, runner or memory backend;
   b. contribution of evidence to Evaluation as an input only (see 6);
   c. stage hooks and added steps, after the Procedure Engine ADR exists.

6. **A stage is never a new `TaskState` and never completes a task.** Added or
   extended stages run as steps coordinated by the application layer (INV-083,
   INV-074/075), return untrusted data and artifact references (INV-093), and
   may propose but not mutate (INV-043). Completion stays with the Evaluation
   Engine (ADR-0018, INV-009). Evidence an extension contributes is an input the
   Evaluation Engine may weigh under the task's evidence requirements
   (ADR-0036); it never satisfies a mandatory gate by itself. Where owner policy
   binds such a check as a requirement, agentd must also attest that it ran in
   the sandbox with the bound inputs (INV-092), and a third party's own pass is
   still not enough without the independent verdict. Core stages cannot be removed or reordered by an
   extension; order among extensions is deterministic (declared position, then
   manifest id). Extension failure is isolated: it fails the task only when the
   policy marks that stage required. Hooks are time-bounded and retries bounded
   (INV-091). The channel is also bounded in message size, message rate and
   total output, and the runner limits memory and CPU; a violation closes the
   channel and kills the process tree, so a flood cannot starve the daemon.

7. **Marketplace is out of scope here and stays DEFERRED.** This ADR fixes what
   it will need so nothing blocks it: digest-pinned manifests from day one, so
   distribution later adds signing, a registry and review but no change to how
   an extension is run or authorized. Until then, installation is a local
   sideload approved by the owner.

## Capability mapping

| Manifest permission | Maps to | Status |
| --- | --- | --- |
| read or write files | filesystem read and write, managed workspace scope | existing |
| run a program | process execution, exact executable | existing |
| network | network access | not in v1: ADR-0040 is only Proposed and the macOS runner supports a single outbound scope (TCP 443, any host) |
| secret | secret access by opaque reference through the broker (ADR-0037, ADR-0039) | not in v1: a secret plus any-host 443 would let an extension send it anywhere; the two never combine until egress is pinned |
| invoke a tool | tool invocation | existing |
| read task journal | scoped read of journal facts | new, needs ADR-0026 amendment |
| propose a stage or step | typed proposal, no mutation | new, needs ADR-0026 amendment |
| contribute evidence | evidence submission, no verdict | new, needs ADR-0026 amendment |

## Threats (summary)

| Threat | Control |
| --- | --- |
| Malicious extension reads files or secrets | sandbox plus exact-scope capabilities; no secret or network capability in v1 |
| Extension impersonates a client or escalates | sandbox denies connecting to the public socket; per-listener client kinds; the kind string is never the control |
| Message flood or oversized frames | bounds on size, rate and output; violation closes the channel |
| Swapped binary after approval | digest of an immutable bundle in the approved hash; the verified copy is what runs |
| Extension forces task completion | no completion operation exists; verdict authority stays with Evaluation |
| Prompt or output injection through results | results are untrusted data (INV-093, INV-038) |
| Resource exhaustion, hung hook | time and retry bounds, process tree killed on timeout |
| Repository enables an extension | workspace and repository layers are restriction-only |
| Supply chain (future marketplace) | deferred; digest pinning and approval now, signing later |

## Alternatives considered

- **Dynamic library in process:** rejected. No isolation, ABI and memory-safety
  exposure, and the sandbox contract cannot apply.
- **WebAssembly component host:** deferred, not rejected. Strong isolation and
  portability, but a new runtime dependency and interface surface (WASI). Revisit
  if per-hook IPC cost or non-macOS sandbox parity makes out-of-process
  impractical. The extension contract is transport-neutral on purpose.
- **MCP servers as the extension model:** rejected as the model (INV-094: MCP
  adapters are thin control surfaces and cannot become orchestration
  authorities). An MCP server may still be one extension type behind a
  capability grant.
- **New TaskStates for stages:** rejected (INV-083, INV-047).
- **A workflow engine such as Temporal:** rejected (INV-141).

## Consequences

- agentd gains a supervised extension host and a new client kind, so the
  handshake, ADR-0020 and the sandbox runner are all on the critical path.
- Only platforms whose runner enforces the requested capabilities can host
  extensions. Today that is macOS Seatbelt; elsewhere the feature fails closed.
- Per-call IPC cost is paid on every hook. It must be measured before points
  b and c are promised.
- ADR-0026 needs an amendment for the three new capabilities before they ship,
  and network and secret access for extensions wait for ADR-0040 and a runner
  that scopes egress by address.
- The listener code in `ipc-control` changes (per-listener client kinds), so
  TASK-0262 touches that crate as well as the contracts.
- The Procedure Engine needs its own ADR before point c starts.

## What would show this decision is wrong

- A measured hook or port call cost that makes the first slice unusable.
- A way found for a sandboxed extension to reach the public socket or to
  obtain authority above its approved manifest.
- Owner demand for stages that must mutate task state, which would conflict
  with INV-083 and need a different model.

## First slice (acceptance for the follow-up task)

Serve `ExecutionPort` from an out-of-process sample extension (a small
executable that implements the contract, packaged as a bundle) with a
manifest, under the sandbox runner, on the agentd-created channel. The vehicle
is the `execution-contract` conformance harness, which already takes
`&dyn ExecutionPort`; the orchestration tests use compile-time ports and are
not the check. It passes when:

- a request for a capability outside the grant fails closed and is audited;
- a sandboxed process cannot connect to the public agentd socket, and a client
  on the public socket that sends client kind `extension` is refused;
- a bundle whose digest differs from the approved manifest is refused before
  start, and a file replaced after install does not change what runs;
- a crash or timeout kills the process tree and fails only the attempt, not
  the daemon, and an oversized or too-fast stream closes the channel;
- the extension protocol has no completion message and an injected one is
  rejected;
- the conformance harness passes against the extension, and per-call IPC cost
  is measured and recorded.

## Open questions for the owner

1. Ratify new gate G17 "Extensions", or attach this work to an existing gate?
2. Is a local sideload with owner approval an acceptable first install story?
3. Is macOS-only hosting acceptable for the first slice?
4. Should the Procedure Engine ADR be scheduled before or after the first slice?
