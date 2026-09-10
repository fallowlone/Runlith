# ADR Coverage Matrix

> Canonical coverage register for TASK-0010 unmaterialized ADR candidates.
> Not an ADR. Statuses follow
> [`ADR-0019-gate-scoped-adr-activation.md`](./ADR-0019-gate-scoped-adr-activation.md):
> ACTIVE DECISION, DEFERRED ARCHITECTURE TARGET, RESEARCH / HYPOTHESIS —
> plus "Covered by existing ADR" where a prior ADR already governs.
> Deferred timing never suspends binding invariants (INV-007, INV-008,
> INV-013, INV-040, and the TB1–TB7 set stay authoritative throughout).

| Architecture area | Classification | Earliest gate | ADR coverage | Required action |
|---|---|---|---|---|
| Local IPC peer authentication | ACTIVE DECISION | G2 | ADR-0020 | none — covered; implementation in TASK-0026 before mutable business IPC |
| G2 business IPC + local data composition | ACTIVE DECISION | G2 | ADR-0021 | none — covered; implementation in TASK-0028 before CLI commands |
| G3 runtime Event Journal scope (execution/attempt aggregate references, initial runtime vocabulary, outcome/version/provenance semantics) | ACTIVE DECISION | G3 | ADR-0022 | none — covered; implementation in TASK-0035 (migration 0004 + RuntimeService) before FakeAgent |
| Planning / Orchestration | ACTIVE DECISION | PRE-G3 | ADR-0017 | none — covered |
| Evaluation / Verification | ACTIVE DECISION | PRE-G3 | ADR-0018 | none — covered |
| Policy Engine | Covered by existing ADR (detail PRE-G6) | G6 | ADR-0013 (baseline: engine + isolation layers) | dedicated G6 design ADR before G6 implementation |
| Secrets Broker | DEFERRED ARCHITECTURE TARGET | G6 (PRE-G6) | direction in ADR-0013 / ADR-0014; no dedicated ADR | dedicated ADR before first G6 credential work |
| Security Audit | DEFERRED ARCHITECTURE TARGET | G6 (PRE-G6) | direction in ADR-0006 / ADR-0013 | finalization ADR before G6 audit work |
| Resource Engine | DEFERRED ARCHITECTURE TARGET | PRE-G9 | none yet | dedicated ADR before G9 retrieval work |
| Adaptive Model Router | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before first adaptive-routing implementation (no gate assigned yet) |
| DeviceResourceManager | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before first device-resource implementation |
| ModelResidencyManager | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before first residency implementation |
| Tools / Skills registry | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before registry implementation (gate unassigned) |
| Tool contracts JSON Schema decision | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before first tool-contract implementation |
| Frontend stack + design system | Covered by existing ADR (partial) | PRE_RELEVANT_GATE (UI phase) | ADR-0015 covers primitives-vs-system; Tauri/React/Vite stack has no dedicated ADR | future stack ADR before UI-phase implementation; ADR-0015 must not be cited for facts it does not cover |
| Security Runner | DEFERRED ARCHITECTURE TARGET | deferred (security gate) | none yet | dedicated ADR before first security-runner implementation |
| Web Acquisition | DEFERRED ARCHITECTURE TARGET | deferred | none yet | dedicated ADR before first acquisition implementation |
| Document Worker | DEFERRED ARCHITECTURE TARGET | deferred | none yet | dedicated ADR before first document-worker implementation |
| Procedure Engine | DEFERRED ARCHITECTURE TARGET | deferred | none yet | dedicated ADR before first procedure-engine implementation |
| Living Documentation / Documentation Engine | DEFERRED ARCHITECTURE TARGET | deferred | none yet | dedicated ADR before first doc-engine implementation |
| Extension-contract architecture | Covered by existing ADR (conditional) | PRE_RELEVANT_GATE if warranted | ADR-0001 + ADR-0009 + INV-069 govern ports/adapters today | none now; dedicated ADR only if the extension surface outgrows current ports |
| Artifact Store | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before first artifact-store implementation |
| External Documentation Source | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet | dedicated ADR before first implementation use |
| Local Inference port | DEFERRED ARCHITECTURE TARGET | PRE_RELEVANT_GATE | none yet (optional adapter by design) | dedicated ADR before first local-inference use |
| TOON/TRON renderer choice | RESEARCH / HYPOTHESIS | research only | none (optional renderers already constrained: never canonical) | benchmarking only; promotion needs review/ADR |

Note: Tournament Mode, Consensus Mode, and Shadow Agents are explicitly
post-v1 and correctly have no ADR; they are not tracked candidates here.
Consequence for G0/G1: no active decision required to safely enter G1
lacks ADR coverage — deferred targets do not block G1 for want of future
detailed ADRs (TASK-0016 audits this independently).
