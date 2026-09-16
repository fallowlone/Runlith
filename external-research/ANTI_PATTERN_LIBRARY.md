# External Anti-Pattern Library

Reusable security, reliability and architecture failure modes confirmed by source-level external evidence. These entries describe lessons for Runlith; they are not claims that every occurrence is an exploitable vulnerability.

## AP-001 — Content package grants runtime authority

**Anti-pattern:** Skill/plugin/workflow metadata directly widens filesystem/process/tool authority.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/capabilities/skills/use-skill.ts:18-20`; `src/capabilities/permissions.ts:339-349,507-509`.

**Failure mode:** A reusable instruction package becomes an authorization input; weaker-trust content can silently expand effective authority.

**Security/reliability impact:** privilege expansion, policy bypass by provenance confusion, difficult auditing of why an operation became allowed.

**Runlith protection:** G6 canonical Policy owns authority; skill/repository/provider content cannot self-grant; child authority is monotonic.

**Remaining exposure:** Future Skill/Tool Registry implementation must preserve this separation explicitly.

**Rule:** Content may declare/request required capabilities. Only canonical Policy/approval may grant effective authority.

## AP-002 — Permission blocklist/parser used as canonical authorization

**Anti-pattern:** Decide security from command strings, allowlists/blocklists and shell parsing heuristics instead of typed capability plus runtime enforcement.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/capabilities/permissions.ts:481-576`.

**Failure mode:** Shell grammar, indirection, nested execution and platform-specific behavior create an ever-growing parser/bypass surface.

**Security/reliability impact:** authorization can diverge from actual process effects; security fixes become reactive additions to a string classifier.

**Runlith protection:** typed Policy capability algebra plus operation-boundary Runner/OS isolation (`ADR-0026`, TASK-0064/0069).

**Remaining exposure:** Provider/tool-specific command classification may still exist as defense in depth; it must never be the canonical allow decision.

**Rule:** String classification may narrow or warn. It cannot substitute for typed authorization and enforced isolation.

## AP-003 — Direct host shell spawn from orchestration

**Anti-pattern:** Orchestration code launches shell-interpreted commands directly on the host without a mandatory isolation/enforcement contract.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/core/background-tasks.ts:132-210` (`spawnShell`, Node `spawn(..., { shell: true })`).

**Failure mode:** Runtime effects inherit whatever host authority is available to the process; timeouts and output limits do not constrain filesystem/network/process authority.

**Security/reliability impact:** sandbox escape is unnecessary if no sandbox is the boundary; host state and credentials can become ambient authority.

**Runlith protection:** Policy → `IsolationPlan` → concrete Runner backend; unsupported mandatory restriction fails before process spawn.

**Remaining exposure:** Trusted Host adapters remain appropriate only when the selected execution contract explicitly permits trusted-host authority.

**Rule:** Autonomous privileged process creation crosses the Runner enforcement boundary.

## AP-004 — Model-authored provenance upgrades its own memory claim

**Anti-pattern:** A model-created memory can assign itself an evidence class such as "direct" while durable provenance is only free-form model/user-summary text.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/capabilities/system/save-memory.ts:5-49`; `src/memory/user-memory.ts:430-450`.

**Failure mode:** confidence/provenance labels become stronger than their underlying evidence and survive as durable memory.

**Security/reliability impact:** stale or hallucinated claims can acquire false epistemic authority and contaminate later context/decisions.

**Runlith protection:** ADR-0012 requires source refs, validation, supersession and treats agent-generated annotations as untrusted until promoted (`INV-029`).

**Remaining exposure:** Memory implementation is future work; the source-ref and promotion contract must remain executable, not prompt-only.

**Rule:** Model confidence is metadata. Provenance names independently addressable evidence.

## AP-005 — Lease expires but stale writer is not fenced

**Anti-pattern:** A work lease/token controls queue bookkeeping, yet an old worker can continue publication after its lease/session authority is superseded.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Evidence:** `src/lib/fold/reconciliation-worker.ts:127-134` explicitly documents that lease replacement removes queue authority while neither described mechanism fences writes after session loss.

**Failure mode:** Two generations can both publish effects: the reclaimer is current owner while a stale worker finishes later.

**Security/reliability impact:** lost-update races, stale canonical state, duplicate side effects, audit ambiguity about which owner was authoritative.

**Runlith protection:** Narrow G4/G5 flows already bind workspace/execution ownership and fail closed; generic external-signal reconciler publication fencing is still open.

**Remaining exposure:** Future generic Scheduler/Reconciler work ownership needs an ADR-level fencing contract.

**Rule:** Every authoritative publication from leased work validates the current durable ownership generation/fence, not only the queue row.

## AP-006 — Process-local duplicate exclusion treated as correctness

**Anti-pattern:** An in-memory set/lock prevents duplicate work only inside one process/store instance and is treated as sufficient ownership.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Evidence:** `src/lib/fold/reconciliation-worker.ts:180-199` documents store/process-local exclusion and bypass by another process/direct reconciliation path.

**Failure mode:** restart or multi-process execution erases/bypasses exclusion while both workers still reach shared authoritative state.

**Security/reliability impact:** duplicate work, race-dependent publication, attempt accounting drift and false assumptions about single ownership.

**Runlith protection:** process-local live-owner tracking is explicitly distinguished from durable state in G5; durable leases/records own recovery truth.

**Remaining exposure:** Generic external-signal reconciliation has no accepted work-claim/fencing contract yet.

**Rule:** Process-local exclusion is an optimization/liveness aid. Durable state plus fencing owns correctness.

## AP-007 — Provider-native hidden/session state treated as durable execution truth

**Anti-pattern:** Treat an opaque provider session, hidden state, or KV/cache snapshot as sufficient durable continuation state without proving that it still matches the current history and execution semantics.

**Source:** `yifanzhang-pro/recurrent-looped-tranformer`

**Revision:** `9c40dec78a6d7da73e1226310fd6c8edaa32577e`

**Evidence:** `Recurrent_Looped_Transformer.pdf` §5.4 and §6 state that old parameter-dependent hidden/KV state is not an exact substitute after changes and that text-only restoration requires replay; Appendix C requires the complete cache to match current parameters, consumed prefix, positions, initialization, window semantics and execution settings. This anti-pattern is a lesson derived from the report's explicit safeguards, not an accusation that RLT violates them.

**Failure mode:** Resume succeeds mechanically while conditioning on stale, removed or semantically incompatible state; the system cannot reproduce why the resumed execution differs from the current canonical inputs.

**Security/reliability impact:** stale context can survive policy/configuration/history changes, provider migration becomes brittle, recovery becomes vendor-dependent, and a native session can accidentally become a shadow source of truth.

**Existing Runlith protection:** Provider session/native context is non-authoritative (`INV-019`, `INV-022`, `INV-023`, `INV-110`); stateful failover cannot blindly replay side effects (`INV-066`); Context Continuity is application-owned.

**Additional lesson:** Before native resume becomes a production optimization, define a provider-neutral compatibility/invalidation contract. On mismatch, discard native state and reconstruct from Runlith-owned continuity; re-evaluate Policy/approval independently.

## Invalidated previous anti-pattern attribution

The previous research attributed **"external event as canonical truth"** to Overflow. This re-audit did not find source-level evidence sufficient to support that repository-specific claim. The reviewed webhook/reconciliation code instead shows authenticated delivery and explicit reconciliation evidence/checkpoints. The general Runlith rule remains valid independently: external events are evidence and require application/domain validation before authoritative mutation.

## AP-008 — Verification unavailability fails open into lifecycle handoff

**Anti-pattern:** A required completion/review judge throws or becomes unavailable and the workflow treats that failure as permission to continue the lifecycle handoff.

**Source:** `NousResearch/hermes-agent`

**Revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `tools/kanban_tools.py:395-408` (`_goal_gate` logs judge failure and allows lifecycle handoff); goal-mode block escape restrictions at `629-640` show the judge is intended to be a meaningful gate.

**Failure mode:** Verification availability, rather than evidence, determines whether completion/review progress is allowed.

**Security/reliability impact:** Work may cross a completion/review boundary without the check the mode intended to require. This is a source-confirmed fail-open policy behavior; practical exploitability is not asserted.

**Existing Runlith protection:** ADR-0018 independent evaluation; provider/model/agent cannot grant Task completion and verifier failure cannot manufacture a verified verdict.

**Additional lesson:** Verification may become `UNKNOWN`, `BLOCKED`, retryable or escalated, but required-verification unavailability never becomes success.

## AP-009 — Durable learned-state approval disappears when enforcement cannot load

**Anti-pattern:** A security-sensitive write-approval layer treats missing/unloadable policy code or invalid configuration as gate disabled and allows the durable write path to continue.

**Source:** `NousResearch/hermes-agent`

**Revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `tools/memory_tool.py:66-73`; `tools/skill_manager_tool.py:583-591`; `tools/write_approval.py:43-59`.

**Failure mode:** Operator intent and effective enforcement can diverge exactly when the approval mechanism is unhealthy.

**Security/reliability impact:** Durable memory/skill state may be mutated without the approval gate the operator expected. The finding is a confirmed policy behavior, not a proven module-loading exploit path.

**Existing Runlith protection:** G6 Policy/Approval exact-binding and trusted operation-boundary enforcement fail closed for required authorization.

**Additional lesson:** Optional feature disablement must be explicit configuration. Failure to evaluate a required policy is an error/deny state, not implicit allow.

## AP-010 — Generated memory promoted to authoritative prompt context

**Anti-pattern:** Previously generated or learned memory is inserted into a later prompt with textual authority stronger than its provenance/validation state warrants.

**Source:** `NousResearch/hermes-agent`

**Revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `agent/memory_manager.py:274-279` wraps recalled memory with language calling it `authoritative reference data`; `163-187` separately implements an internal-context streaming scrubber.

**Failure mode:** Stale, mistaken or poisoned learned content becomes a shadow instruction/reference authority simply because it was persisted.

**Security/reliability impact:** Trust confusion survives across sessions and can influence later tool/planning decisions beyond what the original evidence supports.

**Existing Runlith protection:** `INV-029`, `INV-030`, `INV-038`, ADR-0012 and Context Engine trust/provenance separation.

**Additional lesson:** Output fencing/scrubbing is useful hygiene, but transport secrecy does not upgrade the epistemic or instruction authority of the fenced content.

## AP-011 — Process-local delegation state treated as durable continuity

**Anti-pattern:** An in-memory subagent/delegation handle is treated as sufficient workflow continuity across process restart.

**Source:** `NousResearch/hermes-agent` as contrast evidence; Hermes itself explicitly documents the limitation rather than claiming restart durability.

**Revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `agent/subagent_lifecycle.py`, `SubagentLifecycleService` class documentation around `237+`: children run in-process only and a serialized handle cannot reconnect after restart.

**Failure mode:** Restart loses live delegation ownership/result access while higher-level orchestration still assumes the child can be resumed.

**Security/reliability impact:** orphaned work, duplicate relaunch, provider/runtime-dependent recovery and ambiguous completion ownership.

**Existing Runlith protection:** `Task != Execution != Attempt != Session`; durable Task/Execution/Attempt/checkpoint state owns continuity, while provider/subagent sessions are replaceable execution machinery.

**Additional lesson:** Process-local registries are valid implementation aids when their loss is explicit. Never promote them to canonical workflow truth.

## AP-012 — Unbounded autonomous provider/agent retry loop

**Anti-pattern:** An autonomous execution mode retries provider failures or continues model/tool steps without a finite lifecycle budget and explicit exhaustion outcome.

**Source:** `deepseek-ai/deepseek-harness`

**Revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `packages/llm/llm/src/retry-policy.ts` defines `mode: 'always'` as retry-until-success/cancellation/disposal with no retry-count maximum; `packages/llm/llm-retry/src/index.ts` schedules the next retry for every failure in that mode. Separately, `packages/core/agent-loop/src/agent.ts` has repeated turn/step loops and no reviewed `maxSteps` lifecycle bound.

**Failure mode:** Persistent provider faults or a model that repeatedly emits non-concluding tool calls can consume requests, time and money until an external cancellation/resource limit stops the process.

**Security/reliability impact:** Work can remain indefinitely active instead of reaching a deterministic escalation, retry-exhausted, blocked or failed state; stateful side effects are especially dangerous if a future layer couples such looping with automatic replay.

**Existing Runlith protection:** `INV-050`, `INV-091`; AGENT.md explicitly prohibits unbounded retry/debate/council loops and requires typed, bounded retry plus reconciliation of ambiguous side effects.

**Additional lesson:** Bounded backoff delay is not bounded retry. Every autonomous recurrence needs a finite count/time/cost or policy budget and a durable exhaustion destination.

## AP-013 — Process-holder workflow object treated as canonical orchestration

**Anti-pattern:** Treat a live script/workflow object whose control state exists only in one process as durable long-running workflow truth.

**Source:** `deepseek-ai/deepseek-harness`

**Revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `packages/workflow/workflow/src/index.ts` defines a holder-owned live `WorkflowRun` and observe-only `workflow/*` events; `packages/workflow/workflow-ptc/src/host.ts` keeps run/child/cancellation control in the live host. No durable workflow-resume state machine was found in the reviewed workflow packages.

**Failure mode:** Process restart loses orchestration script position/control even if some child sessions are independently durable, forcing relaunch, manual reconstruction or duplicate work.

**Security/reliability impact:** recovery/completion semantics become dependent on runtime residency and can drift from durable work ownership.

**Existing Runlith protection:** durable Task Graph and `Task != Execution != Attempt != Provider Session`; product-owned Reconciler/Verification state remains canonical across provider/runtime loss.

**Additional lesson:** Ephemeral orchestration DSLs may be useful inside an Attempt, but their holder object must never become the durable Task/Execution state owner.

## AP-014 — Unprobed custom sandbox wrapper asserted as full enforcement

**Anti-pattern:** Report a custom confinement wrapper as full enforcement solely because trusted configuration selected it, without functionally proving that it enforces the promised restrictions.

**Source:** `deepseek-ai/deepseek-harness`

**Revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `packages/sandbox/sandbox-local/src/index.ts:305-326` calls configured `runnerCommand` “the operator's assertion, no probe” and returns `enforcement: 'full'` while relying on configured failure signatures.

**Failure mode:** A misconfigured wrapper can execute successfully while under-enforcing filesystem/process restrictions and still be represented as full confinement.

**Security/reliability impact:** deployment configuration becomes an unverified security attestation; policy believes an enforcement boundary exists that the wrapper may not actually provide.

**Existing Runlith protection:** governed Runner + IsolationPlan/capability model; unsupported mandatory restrictions fail closed and G6 hardening treats enforcement as a concrete runtime boundary.

**Additional lesson:** Trusted operator selection can choose the backend, but the backend's advertised security capabilities should remain testable/provable. Configuration intent alone is not enforcement evidence.

## AP-015 — Ambient host credential becomes an implicit distributed-job capability

**Anti-pattern:** A spawned or distributed job silently inherits authority by reading a credential from the host user's environment/home directory when no explicit scoped grant was supplied.

**Source:** `huggingface/nanotron`

**Revision:** `2411b022a75fb7f7561a1bb4166706da5e1b76de`

**Evidence:** `src/nanotron/eval/one_job_runner.py:163-171` falls back to `~/.cache/huggingface/token` when `HUGGING_FACE_HUB_TOKEN` is absent and exports the token into the generated Slurm job environment.

**Failure mode:** Work receives host-user credential authority that is not represented in the job's explicit capability contract. Compromise, accidental subprocess behavior or later script changes can exercise that ambient authority.

**Security/reliability impact:** Secret provenance and revocation become tied to host identity rather than the Task/Attempt; least privilege and auditability weaken.

**Existing Runlith protection:** G6 Secrets Broker and provider environment isolation require explicit scoped secret delivery; G12 explicitly forbids ambient organization secrets for remote workers.

**Additional lesson:** Credential discovery convenience belongs at a trusted interactive/configuration boundary. Autonomous execution should receive only explicitly granted secret handles/material for the current operation.

## AP-016 — Failed background subprocess remains represented as in progress

**Anti-pattern:** A background state machine observes that its subprocess has already failed but leaves the durable/in-memory operation state as `UPLOADING`, `DOWNLOADING` or another active state, while waiters terminate only on success/idle.

**Source:** `huggingface/nanotron`

**Revision:** `2411b022a75fb7f7561a1bb4166706da5e1b76de`

**Evidence:** `src/nanotron/s3_checkpoints/s3_mover.py:194-228,373-405`: waiters poll until `IDLE`; `update()` sees a non-zero subprocess return code, logs it, and returns without transitioning to a terminal failure state.

**Failure mode:** A completed failure is encoded as continuing work. Polling callers can wait indefinitely and recovery cannot distinguish active progress from exhausted failure.

**Security/reliability impact:** liveness failure, stuck automation, ambiguous recovery ownership and operator dependence on logs rather than typed state.

**Existing Runlith protection:** bounded planning/retry/liveness requirements and recovery architecture require explicit failure/exhaustion outcomes, durable evidence and reconciliation rather than silent endless polling.

**Additional lesson:** Every autonomous/background operation needs a terminal failure representation and a bounded wait/retry/escalation path. Logging an error is not a lifecycle transition.

## AP-017 — Transport failure encoded in the model-content channel

**Anti-pattern:** After retry exhaustion, represent a provider/transport failure as an ordinary text value in the same channel used for model output.

**Source:** `Tencent-Hunyuan/Simple-Attention-Sparsification`

**Revision:** `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`

**Evidence:** `src/sas/eval/client.py:26-57` catches any request exception, performs bounded retries, then returns a string beginning `[REQUEST_FAILED...` plus zero completion tokens.

**Failure mode:** Callers/scorers must know a sentinel convention to distinguish execution failure from generated content; failure class, retry count, latency and recovery evidence disappear from the typed result.

**Security/reliability impact:** Benchmark or orchestration logic can under-observe provider instability and accidentally process an execution failure as content. In a stateful workflow this also makes safe retry/reconciliation decisions harder.

**Existing Runlith protection:** typed Provider/Attempt outcomes, durable events, `Task != Execution != Attempt != Session`, and `INV-091` bounded/observable retry policy. App-service orchestration tests explicitly reject hidden retry behavior.

**Additional lesson:** Content and execution state need separate types even when a research benchmark prefers a total-order result list.

## AP-018 — Model artifact selection implicitly grants code-execution authority

**Anti-pattern:** A model-serving launcher enables arbitrary repository/model custom code by default, making artifact selection double as permission to execute code under the server process authority.

**Source:** `Tencent-Hunyuan/Simple-Attention-Sparsification`

**Revision:** `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`

**Evidence:** `scripts/eval/_serve.sh:134-140` unconditionally adds SGLang's `--trust-remote-code` flag to the server arguments.

**Failure mode:** Supplying an untrusted remote model/repository can cross from data/model loading into Python-code execution without a separate authority decision.

**Security/reliability impact:** Supply-chain compromise or mistaken model selection executes with the ambient filesystem/network/credential rights of the inference process.

**Existing Runlith protection:** G6 typed Policy/capability enforcement, Runner isolation, provider environment isolation and Secrets Broker keep model/provider selection separate from process/network/filesystem/secret authority.

**Additional lesson:** Local inference adapters should declare whether they execute model-provided code and make that an explicit trust/capability condition. Model quality/routing evidence never grants runtime authority.

## AP-019 — Exact human approval translated into a coarse provider-wide bypass

**Anti-pattern:** A user approves one blocked action, but the integration resumes the provider under a permission mode that grants a wider class of actions because the upstream provider lacks an exact-action resume primitive.

**Source:** `alphaXiv/OpenResearch`

**Revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/local/harness/claude.rs:961-971` documents that approving a blocked Claude tool defaults to `PermissionMode::Bypass` because `acceptEdits` re-denies Bash/non-edit tools and `bypassPermissions` is the mode that clears the block.

**Failure mode:** Authority after approval is broader than the request the human actually approved; subsequent provider actions can execute under the widened mode.

**Security/reliability impact:** Approval evidence and effective runtime authority diverge, replay/audit semantics become provider-mode dependent, and provider permission vocabulary can silently weaken application policy.

**Existing Runlith protection:** ADR-0026, TASK-0062/TASK-0067 and `INV-148`/`INV-150` bind authority to the exact request/capabilities/policy fingerprint/provenance/validity and consume approval exactly once. Provider-native permission state is not canonical authority.

**Additional lesson:** When an upstream provider exposes only coarse permission modes, enforce exact approved authority outside the provider. Compatibility pressure never widens the canonical grant.

## AP-020 — Synced user environment becomes ambient autonomous-agent authority

**Anti-pattern:** A convenience environment file/shell import is propagated broadly into autonomous provider children, so adding a user variable can silently grant credentials or configuration authority to unrelated executions.

**Source:** `alphaXiv/OpenResearch`

**Revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/config.rs:129-132,328-353` states and implements fanning `~/.openresearch/env` into compute/agent environments; `src/local/chat/mod.rs:7689-7706` exports shell and synced environment into provider children.

**Failure mode:** A token or sensitive variable intended for one integration is automatically available to provider/tool subprocesses that were never explicitly granted it.

**Security/reliability impact:** least privilege, secret provenance, revocation and auditability weaken; provider compromise gains the ambient authority of the user's configured environment.

**Existing Runlith protection:** TASK-0065 clears inherited environment and builds an allowlist with isolated provider-writable runtime roots; Secrets Broker and Policy own explicit scoped secret delivery.

**Additional lesson:** Treat child environment as a capability manifest. Convenience discovery belongs at a trusted configuration boundary, not as an implicit grant to autonomous execution.

## AP-021 — Loopback network location treated as principal identity for a privileged local API

**Anti-pattern:** A broad local control plane treats successful connection to loopback plus HTTP Host/Origin checks as sufficient caller trust instead of authenticating the local OS principal/capability.

**Source:** `alphaXiv/OpenResearch`

**Revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/commands/up.rs:1-12,430-702` exposes the normal dashboard on loopback without the remote-session bearer layer; `src/commands/up_remote.rs:1147-1245` implements Host/Origin/Sec-Fetch checks; `src/commands/up.rs:3868-3975` explicitly documents loopback-only/no-auth arbitrary user-readable absolute-file access.

**Failure mode:** A local caller that can reach the port is inside an authority boundary spanning filesystem, project mutation, settings, shell/agent and compute actions even when it has no explicit application principal credential. A concrete privilege increase depends on the local sandbox/process threat model.

**Security/reliability impact:** transport locality and principal authorization are conflated; expanding the API expands the consequences of that trust assumption.

**Existing Runlith protection:** ADR-0020 authenticates same-user OS identity (UID/SID) before IPC handshake and uses endpoint permissions; sensitive operation authority is still separately controlled by Policy.

**Additional lesson:** Loopback binding and browser-origin defenses are useful transport/web protections, but they do not replace application/OS-principal authentication for a privileged control plane.

## AP-022 — Idempotent migration implemented by ignoring every DDL error

**Anti-pattern:** Make additive migrations rerunnable by discarding all `ALTER TABLE` execution errors instead of distinguishing the expected already-applied case from unexpected storage failures.

**Source:** `alphaXiv/OpenResearch`

**Revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/store.rs:591-630` executes a large additive migration list with `let _ = conn.execute(...)`; `637-674` in the same file demonstrates a versioned transactional migration for a more complex schema change.

**Failure mode:** disk/database/corruption/schema errors other than duplicate-column can be silently accepted, leaving an installation on a partially evolved schema.

**Security/reliability impact:** latent startup/schema drift, failures far from the migration cause, and weaker evidence about which schema contract a running process actually has.

**Existing Runlith protection:** versioned durable contracts, fail-visible persistence errors and verification discipline; no OpenResearch-specific Runlith gap was found.

**Additional lesson:** Idempotency means recognizing prior successful application. Unexpected migration failure remains a terminal/repairable error and must be observable.

## AP-023 — Browser navigation bypasses centralized SSRF/network policy

**Anti-pattern:** A browser-control layer validates only tool/session ownership and then forwards an arbitrary URL to the browser, leaving destination/scheme/private-network/redirect authority to whatever the browser can reach.

**Source:** `Tencent/BrowserSkill`

**Revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**Evidence:** `apps/extension/src/tools/navigation.ts:568-674` checks that `params.url` is a non-empty string, validates session/Agent Window scope, and then calls CDP `Page.navigate`; the fallback in `apps/extension/src/tools/browser-navigation.ts:16-40,47-109` uses browser navigation APIs. No centralized scheme/IP/private-range/redirect policy is invoked in this path.

**Failure mode:** Possession of the browser navigation capability implicitly grants the network reachability of the browser profile/host, including destinations outside the application-level intent unless another external layer blocks them.

**Security/reliability impact:** SSRF/internal-network reachability becomes coupled to browser placement and deployment topology. Moving the same browser worker to a cloud/VPN/internal environment can silently widen effective authority.

**Existing Runlith protection:** `INV-097`, `THR-007`, AGENT web rules and the Source of Truth `NetworkAccessPolicy` require centralized evaluation of every acquisition and redirect hop, including resolved network ranges.

**Additional lesson:** Session/tab ownership answers "which browser target may this Attempt control?" Network policy independently answers "which network destination may that target reach?" Neither boundary substitutes for the other.

## AP-024 — Remote device authentication doubles as browser-profile authorization

**Anti-pattern:** Treat successful pairing/authentication of a remote browser device as the trust decision for the whole browser profile, including existing signed-in website sessions, instead of separating device identity from site/account/target capabilities.

**Source:** `Tencent/BrowserSkill`

**Revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**Evidence:** `docs/remote-extension-connection.md:162-168` explicitly states that a paired server can create Agent Windows and navigate using the paired browser profile, including signed-in website sessions, and describes pairing as device authorization rather than a restricted website/account sandbox. Existing user tabs still require explicit borrow for content control, but the device-level trust boundary remains broad by design.

**Failure mode:** A compromised or over-privileged paired controller begins from the authority of the user's browser profile; target-level borrowing limits which existing tabs it can control but does not transform device authentication into least-privilege site/account authorization.

**Security/reliability impact:** The blast radius of one remote-device credential can include many unrelated authenticated website contexts and network locations available to that profile.

**Existing Runlith protection:** Policy Engine capability scoping, Secret Broker bindings, `INV-099`, `INV-106`, and `INV-146`-`INV-153` separate authenticated executor identity from exact operation/secret/network authority.

**Additional lesson:** Authenticate the worker/device strongly, then grant site credentials, targets, network scope and actions separately. Device identity is evidence about the executor, not a wildcard capability over everything that executor can technically access.
