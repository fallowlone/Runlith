# Study

Planner call shape against a provider CLI: resolving TASK-0106 D3 and D5.

- study_type: vendor-documentation research plus direct measurement of our own provider path
- review_date: `2026-09-19`
- sources: Claude Code CLI reference and structured-output documentation, read live; plus 20 controlled calls against the authenticated CLI
- evidence_level: **measured**, for the experimental half; documentation-level for flag semantics

# Method limitation, stated up front

TASK-0103 reached its conclusion by convergence across five independent
products. This study does not have that. GitHub code search returned nothing
usable for headless `claude -p` invocation patterns, so there is **no
cross-product corroboration** that the recommendation below is how others
solve this.

What it has instead is stronger for this particular question: the behaviour
of the exact provider we ship against, measured directly, with a control.
Every rate below is a count of real calls, not an inference.

# Research target

Two questions escalated by TASK-0107, recorded in providers.md §16:

1. **D5** — may a planner propose against evidence references it cannot
   resolve, and is `permission_mode: "plan"` the right proposal-only control?
2. **D3** — can a per-call input-token bound mean anything when the provider
   CLI's own system prompt dominates the measurement?

# Finding: both questions have one root cause, and it is ours

The planner call is shaped wrong, and the shape produces both symptoms.

We send the planner role **in the user message body**, on top of Claude
Code's full coding-agent system prompt, and constrain it with
`--permission-mode plan`. From that single choice:

- **D5 follows.** The model sees two competing instruction sets. Its refusal
  says so in its own words: the planner block "reads as an attempt to
  override the operating context via the message body rather than genuine
  task content." That is a correct reading of what we sent.
- **D3 follows.** We measured the default system prompt we never replaced.
  Our own prompt is ~1,815 bytes; the rest of the 24,000-35,000 tokens is
  Claude Code being Claude Code.

## F-1 — The CLI has primitives we were not using

From the CLI reference, read live on the review date:

| flag | documented behaviour |
|---|---|
| `--system-prompt-file` | Replaces the **entire** system prompt with file contents |
| `--tools ""` | Restricts built-in tools; `""` disables all |
| `--disallowedTools "mcp__*"` | `--tools` does not affect MCP tools; this denies them |
| `--json-schema` | Validated JSON output matching a JSON Schema (print mode) |
| `--bare` | Skips discovery of CLAUDE.md, plugins, MCP, skills, subagents |

## F-2 — Measured: the call shape decides everything

Same prompt content, same empty `git init` workdir, `--safe-mode` throughout.

| configuration | valid JSON | input tokens |
|---|---|---|
| current shipped shape (`--permission-mode plan`, role in body) | **0 / 3** | ~34,800 |
| `--system-prompt-file` + `--tools ""` | **7 / 7** | **~1,890** |
| the same plus `--json-schema` | **2 / 2** validated | ~3,300 |

Zero V5 violations in all 9 successful runs, against the TASK-0107 prompt
that now states the baseline risk.

## F-3 — `--tools ""` is a stronger control than plan mode, not a weaker one

This matters, because "turn off plan mode" reads like weakening a control to
make a test pass, which AGENT.md §23 forbids outright.

It is the opposite. `bound_planner_config` describes plan mode as "defense
in depth on top of the mandatory Runner sandbox". Plan mode is a *behavioural
mode*: the model is asked to plan rather than act. `--tools ""` is
*structural*: there are no tools to call. Denying `mcp__*` alongside closes
the documented gap that `--tools` does not cover MCP.

Replacing a behavioural constraint with a structural one strengthens the
proposal-only guarantee. The Runner sandbox remains the load-bearing
boundary either way (providers.md §15), and planner output remains untrusted
and validated either way (§11, INV-093).

## F-4 — `--json-schema` makes three defect classes structurally impossible

D1 (fenced output), D4 (`scope_delta` shape drift) and D6 below are all the
same failure: we asked for a shape in prose and got prose-quality
compliance. A schema is enforced by the SDK, which validates and re-prompts
on mismatch, and the validated document arrives in a `structured_output`
field rather than inside a text blob that must be parsed.

**Cost, which must be accounted rather than ignored.** Structured output
adds a turn (`num_turns` 2 rather than 1) and ~1,400 input tokens, and the
documentation states it **re-prompts on mismatch** up to a retry limit,
failing the run if it never validates. Those retries consume tokens that
ADR-0027 §6 budgets and TASK-0078 reserves. A receipt that reports only the
final turn would under-report real spend. This needs explicit handling
before adoption, not after.

## F-5 — `--bare` breaks provider authentication

Measured, 1 of 1: `claude -p --bare` returns `is_error: true` with
`Not logged in · Please run /login` and zero usage, on a CLI that
`claude auth status` reports as logged in.

This is the TASK-0101 shape again — a deliberately minimal environment
severing provider-owned authentication. `--bare` would have been attractive
for D3; it is unusable for a subscription-authenticated provider. Recorded
so nobody re-derives it.

## F-6 — New defect: the prompt's placeholder notation is taken literally

1 of 7 runs returned `scope_delta: ["workspace:repo:src"]` — the model
copying the prompt's `[workspace:path]` pattern as a literal prefix. This
fails `ScopeRoot::parse`. It is a prompt-notation defect, cheap to fix, and
a schema `pattern` catches it independently.

# Consequence for D3: no constant needs to change

At ~1,890 tokens (or ~3,300 with a schema) against `MAX_PLANNER_INPUT_TOKENS`
of 32,000, the budget has roughly tenfold headroom. The bound was never too
small; the call was too fat.

The principle TASK-0107 raised remains true — a bound on
provider-reported *total* input still counts provider overhead, and will
still drift with CLI versions — and should stay recorded in providers.md
§16. But it is no longer a practical problem, and raising the constant would
have masked the real defect while spending more quota. **Recommendation: fix
the call shape, change no constant, keep the principle on the open list.**

# Consequence for D5: the seam question survives, narrowed

The mechanical half is answered: the refusal was caused by instruction
placement plus plan mode, and the fix strengthens rather than weakens the
proposal-only control.

The substantive half is **not** answered and must not be quietly dropped.
The model's objection — that it was asked to decompose against
`criterion-root` and `evidence-planner-smoke` it could not resolve — is
still true in the passing configuration. It complies now because the role is
legitimately its own, not because the evidence became resolvable. We are
getting plausible decompositions over unresolvable references.

That is the real open question, and it is unchanged by this study: whether
`DecompositionRequest` should carry resolvable evidence, or whether
proposing against opaque refs is the accepted v1 semantic with validation as
the safety net. It belongs in an ADR, not in an adapter.

# Adoption decision (proposed, not taken)

Adopt, as one bounded task:

1. Replace `--permission-mode plan` with `--tools ""` plus
   `--disallowedTools "mcp__*"` in `bound_planner_config`; update the test
   that pins plan mode to pin the stronger control instead.
2. Move the planner role from the message body to `--system-prompt-file`,
   leaving only the Request object as the user message.
3. Fix the `[workspace:path]` notation (F-6).
4. Keep `json_payload` as belt-and-braces even if a schema lands.

Evaluate separately, because it changes the cost model:

5. `--json-schema` for the proposal document, **with** the retry accounting
   of F-4 settled first.

Do not adopt:

6. `--bare` (F-5).

Unchanged and still open: the evidence-resolution semantics (D5 substantive
half) and the measurement principle behind D3.

# Version sensitivity

Several of these flags carry documented version minimums, and the local CLI
is 2.1.277. providers.md §15 already states that provider CLI versions change
independently of our application and that detection must eventually support
incompatible states. A call shape built on these flags makes that concrete:
adapter-local version policy (ADR-0024) should cover them before this ships
to anyone whose CLI we do not control.
