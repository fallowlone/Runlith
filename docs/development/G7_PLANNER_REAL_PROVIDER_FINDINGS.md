# G7 Planner — Real-Provider Findings (TASK-0106 / TASK-0107)

> Evidence record for the first execution of the G7 planning path against the
> real `claude` CLI. Measurements, not inference. Two findings are resolved
> here; two are escalated because they are contract questions rather than
> defects, and one is a measurement whose conclusion is that the bound may be
> measuring the wrong thing.

## Why this document exists

Until TASK-0106 the G7 planning path had never run against a real provider.
Both the adapter contract suite and the `claude_fixture` harness in
`planning_service.rs` substitute a `#!/bin/sh` script for the CLI, so the
path *real CLI → provider parsing → `PlanChangeSet` → durable revision* had
no test. TASK-0106 added one
(`crates/app-services/tests/planner_real_provider_smoke.rs`, `#[ignore]`d
behind `RUNLITH_CLAUDE_SMOKE=1`). It failed on first contact, and every
finding below comes from that failure or from reproducing it directly
against the CLI.

Measurement discipline: the smoke's harness was verified against a
hand-built known-good proposal before its verdict was trusted, reaching
`Accepted` with durable head 1, so a real-CLI failure is a statement about
the provider path rather than about the test.

## D1 — the CLI fences its JSON (RESOLVED, TASK-0107)

**Observed.** Three consecutive smoke runs (41.6s, 38.9s, 75.4s):

```text
malformed planner output: invalid JSON: unexpected character at byte 0
```

Reproduced directly: `result` begins ` ```json\n{ ` and ends ` }\n``` `. The
JSON inside is well-formed. The prompt already says "Return one JSON object
only; no markdown"; the model does not comply.

**Resolved.** `json_payload` in the Claude adapter unwraps exactly one
cleanly fenced document — the fence must open the response and close it,
with nothing outside. Anything else is passed through untouched so the JSON
parser refuses it. This locates a payload; it does not trust one. The narrow
rule is deliberate: scanning for a brace inside prose would let a model hide
a second document behind surrounding text, and providers.md §11 keeps
provider output untrusted either way.

## D2 — the prompt withheld the baseline risk (RESOLVED, TASK-0107)

**Observed.** With the fence unwrapped, the live CLI returned a node with
`"risk_class": "medium"` and `"required_gate": null` against a `Low`
baseline. V5 refuses exactly that shape. The prompt listed the legal risk
classes but never stated the baseline, so the planner could not have known.

**Resolved.** `baseline_risk` now travels on `DecompositionRequest` and
`ReplanRequest`, set by the trusted service from
`PlanningPolicyContext.baseline_risk` exactly as `scope_bound` already is,
and the adapter states both the baseline and the gate rule in provider
input. See the TASK-0107 amendment to ADR-0027, including why an input bound
is not an authority.

## D4 — `scope_delta` had no stated shape (RESOLVED, TASK-0107)

**Observed.** Smoke run after D1: `invalid input: scope must declare at
least one root`. Three direct runs returned three different shapes for the
same key:

| run | `scope_delta` |
|---|---|
| 1 | `["repo:src"]` |
| 2 | `{"added": [], "removed": []}` |
| 3 | `[]` |

Only the first parses. `scope_delta` was the single required response key
whose type the prompt never specified — node `scope` is documented as
`[workspace:path]`, `budget_delta` and `coverage_map` have stated shapes,
`scope_delta` appeared only in the key list.

**Resolved.** The prompt now states the shape and what to send when the
proposal adds no scope.

## D5 — `plan` permission mode makes the CLI refuse the planner prompt (ESCALATED)

**Observed.** Same prompt, same directory, six runs, the only variable being
`--permission-mode plan`:

| configuration | valid JSON |
|---|---|
| with `--permission-mode plan` | **0 / 3** |
| without | **3 / 3** (3 nodes each, no empty node scopes) |

The refusals are prose, not malformed JSON. The model explains itself: it
reports two conflicting instruction sets, notes that it can see no
`repo:src`, no `criterion-root` and no `evidence-planner-smoke` it can
verify, states that producing the JSON would mean inventing planning data
with no basis, and cites "the plan-mode rule that overrides any other
instructions I've received this session."

**Why this is escalated rather than fixed.** `bound_planner_config` forces
`permission_mode: "plan"` deliberately, as proposal-only defense in depth,
and `planner_forces_proposal_only_provider_permission_mode` pins it.
Removing it to make the smoke pass would be weakening a deliberate control
to suit an implementation — the thing AGENT.md §23 forbids outright. It is
also not a trivial call in either direction:

- providers.md §15 already says provider-native permission features are
  defense in depth only; the load-bearing boundary is the Runner sandbox
  plus the fact that planner output is untrusted and validated. On that
  reading, dropping `plan` mode costs a redundant layer, not a guarantee.
- Against that: it is a documented, tested, intentional choice, and the
  reason it was chosen does not stop being true because the provider finds
  it inconvenient.

**The deeper half.** The model's substantive objection is sound engineering
and survives the plan-mode question: `DecompositionRequest` hands the planner
opaque evidence *references* with no means to resolve them, then asks it to
decompose against them. In `plan` mode the model refuses to invent; without
it, the model complies — which means in the passing configuration it is
producing a decomposition for criteria and evidence it cannot see. That is
worth stating plainly: D5's "fix" makes the smoke green by removing the
objection, not by answering it.

**Question for the owner.** Whether a planner may propose against evidence
references it cannot resolve, and if not, what resolution boundary the seam
needs. This changes what `DecompositionRequest` means and touches ADR-0027 §1
and providers.md. It is not decided here.

## D3 — the per-call input bound may be measuring the wrong thing (ESCALATED)

`MAX_PLANNER_INPUT_TOKENS` is 32,000 (ADR-0027 §6), and the TASK-0078
accounting amendment reserves that full amount per call before invoking the
port.

**Measured**, `total_input_tokens` = `input + cache_creation + cache_read`:

| environment | request | total input |
|---|---|---|
| adapter-isolated, cold cache | trivial | 24,282 / 24,338 / 24,343 |
| host `~/.claude`, warm cache | trivial | 34,844 / 34,844 / 34,846 / 36,121 |

**Reading.** A *trivial* request consumes 76% of the per-call bound under the
adapter's own isolation, and exceeds it outright in a warmer-cache
environment. But the dominant term is Claude Code's own system prompt and
cache behaviour, not our planning prompt, which is roughly 1.5k characters.

So the number is not straightforwardly "too small". The finding is that a
fixed per-call bound applied to a provider CLI's *total reported* input
tokens is mostly measuring provider overhead, and will drift with CLI
versions independently of anything Runlith does. Raising the constant would
restore headroom and preserve the same defect.

**Question for the owner.** Whether §6's per-call input bound should measure
provider-reported total input at all, or whether the budget needs to
distinguish planning input from provider overhead. Deliberately not answered
by picking a new constant: the honest options are a different measurement, an
explicit provider-overhead allowance, or an accepted decision that the bound
tracks provider cost rather than planning size.

## State of the smoke

Red, on D5, in the configuration the architecture currently requires. D1, D2
and D4 are fixed and the path now reaches the validator. The smoke is
`#[ignore]`d and gated, so this costs no CI signal; it records honestly that
G7's own capability does not yet work end to end against a real provider.
