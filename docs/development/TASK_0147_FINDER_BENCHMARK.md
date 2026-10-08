# TASK-0147 finder benchmark: protocol, question set, run log

Status: phase B benchmarked (R1–R5 complete). Gate note: G8 closed;
G9 de-facto active per owner directive (formal G9 activation still
open in queue).

## 1. Method definitions

- **rg baseline.** Fixed query list per question (section 4), executed
  by script, first run only (deterministic). Union of hits is the
  answer evidence; no judgment calls.
- **RTK-style baseline.** Agent answers using batched shell retrieval
  only (`rg`/`find`/`sed`/`head` — no semantic file tools), budget at
  most 5 tool calls per question, output condensed RTK-style (signals
  kept, noise dropped). 2 runs; runs share one operator, so observed
  variance is a lower bound. Timing/provenance commands are outside
  the budget and logged separately.
- **Finder (phase B).** The agentctl surface built in phase B, same
  question set, same scope and conditions.

## 2. Search scope and decontamination

- Root: repository checkout at the recorded revision.
- Excluded from every method: `.git/`, `target/`, `.claude/`
  (sibling worktrees would duplicate hits), and this file
  (`docs/development/TASK_0147_FINDER_BENCHMARK.md` — it contains the
  proofs; searching it is contamination).
- Violation of the exclusions invalidates the run.

## 3. Measures

- **Relevance 0–2** per question: 0 missed (proof fact absent), 1
  partial (fact present but incomplete or buried in noise that
  exceeds 3× the proof size), 2 full (proof fact present and
  identifiable).
- **Source support 0–2** per question: 0 no usable citation, 1 file
  right but location imprecise or unverifiable, 2 exact file path
  (and line or symbol where applicable) that contains the proof.
- **Tokens**: uniform `chars/4` estimator over method inputs (queries
  + commands) and outputs (raw bytes before condensing for RTK-style;
  scored transcript noted separately). No exact tokenizer exists in
  the repo (tiktoken is docs-only); the estimator is uniform across
  methods, so relative comparison is valid and absolute values are
  approximate.
- **Elapsed**: wall time per question (rg: `time` per block; RTK-style:
  `date +%s.%N` around the block, outside the tool budget).

## 4. Question set (proofs recorded before any run)

- **Q1 navigation.** Where is the advisory verification stage wired
  into the orchestration cycle? Proof: post-step in
  `continue_workspace_cycle`
  (`crates/app-services/src/orchestration_service.rs`) calling
  `run_advisory_verification`
  (`crates/app-services/src/advisory_verification.rs`). rg queries:
  `advisory_stage`, `run_advisory_verification`,
  `continue_workspace_cycle`.
- **Q2 contract.** What are the exit criteria for TASK-0165 step 2?
  Proof: `exit_criteria_summary` of the TASK-0165 entry in
  `planning/tasks/queue.yaml`. rg queries: `TASK-0165`,
  `exit_criteria_summary`.
- **Q3 boundary.** Which component owns Task lifecycle mutation, and
  what is forbidden to others? Proof: `TaskService`
  (`crates/app-services/src/task_service.rs`) owns mutation (INV-043,
  `docs/architecture/INVARIANTS.md`); `PlannerPort`
  (`crates/plan/src/model.rs`) proposes only. rg queries:
  `TaskService`, `INV-043`, `PlannerPort`.
- **Q4 failure semantics.** What happens to a task when independent
  evaluation produces no verdict? Proof: the Task stays VERIFYING
  (`crates/evaluation/src/error.rs` module docs). rg queries: `stays
  VERIFYING`, `EvaluationError`, `no verdict`.
- **Q5 config.** How does the daemon construct the verification
  runner, and with what isolation requirements? Proof: lazy factory
  in `bins/agentd/src/execution.rs`, strict
  `IsolationRequirements::new(true, true, true)`,
  `RunnerLaunchPurpose::Provider`. rg queries:
  `IsolationRequirements::new`, `RunnerLaunchPurpose::Provider`,
  `build_verification_runner`.
- **Q6 rationale.** Why does `schedule()` skip advisory verification?
  Proof: no workspace by construction — `finish()` constructor
  comments in `crates/app-services/src/orchestration_service.rs`
  ("shared by the workspace-less `schedule` path"). rg queries:
  `workspace-less schedule`, `no workspace to execute against`.
- **Q7 negation control.** Does the runner execute V0 plans? Proof:
  no — refused by construction in `validate_plan_commands`
  (`crates/app-services/src/verification_execution.rs`). rg queries:
  `validate_plan_commands`, `refuses V0`.
- **Q8 multi-hop.** Who writes and who reads observed effects for the
  repair loop? Proof: the verification runner records `ProcessExit`
  via `append_observed_effect`
  (`crates/app-services/src/verification_execution.rs`,
  `adapters/storage-sqlite/src/observed_effect_store.rs`);
  `mint_candidate_verdicts`
  (`crates/app-services/src/candidate_verdict.rs`) binds them per
  attempt. rg queries: `list_observed_effects_for_attempt`,
  `mint_candidate_verdicts`, `append_observed_effect`.

## 5. Run protocol

- Same checkout state for all runs of one round (record revision).
- Same question set and order (Q1–Q8), same scope (section 2).
- Repeats: rg 1× (deterministic); RTK-style 2×; finder per phase B
  plan (1× + 1 repeat on failure).
- Each run records: repo revision, build/config (n/a for retrieval),
  hardware (`uname -m`, CPU model), method, date, per-question
  queries/calls (verbatim), raw outputs (or hash + excerpt when
  large), condensed transcript (RTK-style), token counts, elapsed,
  scores with one-line justification each.
- Scoring is done against section 4 proofs only; anything not in the
  proofs is a hypothesis, not a result.

## 6. Provenance (this round)

- Repo revision: `40754ae` (origin/main, post PR #28).
- Hardware: recorded per run.
- Operator: single (Muse session).

## 7. Run log

### Run R1 — rg baseline (2026-10-07, arm64, Apple M2 Pro)

Script `/tmp/task0147-r1.sh` (session artifact; queries verbatim from
section 4, exclusions from section 2, no output cap). Raw output
72,508 bytes; per-question excerpts below are the scored evidence.

| q | rel | sup | out-tok | elapsed | note |
|---|-----|-----|---------|---------|------|
| Q1 | 2 | 2 | ~776 | 0.18s | calls + def + module visible, 23 lines |
| Q2 | 1 | 1 | ~3645 | 0.08s | proof buried in 171 lines of cross-refs |
| Q3 | 1 | 1 | ~10309 | 0.07s | proofs present in 332 lines of noise |
| Q4 | 2 | 2 | ~1240 | 0.08s | proof line verbatim (`error.rs:3`) |
| Q5 | 1 | 1 | ~962 | 0.07s | 5 similar `new(true,true,true)` sites, ambiguous |
| Q6 | 2 | 2 | ~62 | 0.05s | single exact proof line |
| Q7 | 1 | 1 | ~390 | 0.05s | symbol found, refusal body needs one more read |
| Q8 | 2 | 2 | ~725 | 0.11s | writer + reader both visible, 26 lines |

Totals: relevance 13/16, support 13/16, ~18,127 output tokens
(~150 input), 0.69s, 21 fixed queries.

### Run R2 — RTK-style run 1 (2026-10-07, same hardware)

Strategy A (symbol-first rg, follow-up `sed` context). 12 tool
calls (budget ≤5/question respected: 2,1,2,1,2,1,1,2).

- Q1: `rg advisory_stage` + `run_advisory_verification\(` →
  follow-up `sed` confirms placement inside
  `continue_workspace_cycle`. 2/2.
- Q2: `sed` TASK-0165 entry directly → exit criteria verbatim. 2/2.
- Q3: `struct TaskService` + `INV-043` (usages) → follow-up reads
  `INVARIANTS.md:19` statement + `trait PlannerPort`. 2/2.
- Q4: `stays VERIFYING` → `error.rs:3` + `port.rs:14`. 2/2.
- Q5: factory symbol + purpose lines → follow-up `sed` shows
  strict requirements verbatim. 2/2.
- Q6: exact phrases hit `orchestration_service.rs:955-956`. 2/2.
- Q7: `V0` in runner file → line 27 refusal doc verbatim. 2/2.
- Q8: fn defs → follow-up confirms runner
  `record_effect` → `append_observed_effect`. 2/2.

Totals: relevance 16/16, support 16/16, ~2,654 output tokens
(~700 input), ~2.8s elapsed.

### Run R3 — RTK-style run 2 (2026-10-07, same hardware)

Strategy B (file-first vocabulary, deliberately different queries).
14 tool calls (1,2,2,2,2,2,1,2). One vocabulary miss recovered in
budget: Q4 `no verdict` scoped to `evaluation/src` hit an unrelated
line; retry on `VERIFYING` in the two proof files converged. All
other questions converged as in R2.

Totals: relevance 16/16, support 16/16, ~3,142 output tokens
(~825 input), ~1.0s elapsed.

### Phase A reading (hypothesis, not verdict)

Fixed-query rg loses exactly where a competent operator adapts:
cross-reference burial (Q2/Q3), ambiguous near-duplicate sites
(Q5), and body-behind-symbol (Q7). RTK-style converges fully in
both runs at ~6× fewer output tokens for ~2–4× elapsed, with zero
score variance across runs (same-operator caveat from section 1
stands). The phase B finder must beat R2/R3 on elapsed and
output tokens at equal scores — not merely beat rg.

### Run R4 — finder run 1 (2026-10-07, arm64, Apple M2 Pro)

Phase-B surface (`agentctl find`, protocol v7 over UDS IPC) against
`/tmp/task0147-corpus` (detached clean `40754ae`). Fresh
`target/debug/agentd` + `agentctl` from branch
`task-0147-repo-finder`. 19 calls (budget ≤5/question respected:
2,2,4,2,2,2,1,4). Raw outputs: `/tmp/task0147-r4.log` (session
artifact, timed verbatim, 33,866 bytes incl. timing lines;
sha256 `118a16d85d8e4e27…`). Replay script `/tmp/task0147-r4.sh`.
Contamination: zero hits in this file; `.git/`/`target/`/`.claude/`
never surfaced (hidden-skip + ignore files). Every response bound
to rev `40754ae…b2f51`, clean. Evidence ids and continuation
tokens below abbreviated as `<id>`/`<tok>`; full values in the log.

- Q1: `query … "run_advisory_verification
  continue_workspace_cycle" --scope crates/app-services --limit 10`
  → 10/13 (callee def `advisory_verification.rs:91`, import, refs);
  same + `--continuation <tok>` → 3/13 with def
  `orchestration_service.rs:468` + call `:669`. Placement by
  same-file ordering — the same inference R2 scored 2/2 on. 2/2.
- Q2: `query … "TASK-0165" --scope planning/tasks --limit 5` →
  header `queue.yaml:5516` (1/1); `show … <id> --context 40` →
  window 5476-5556 with the full `exit_criteria_summary` value
  verbatim (key 5533, value 5534-5539 by window arithmetic). 2/2.
- Q3: `query … "struct TaskService" --limit 5` → def
  `task_service.rs:52` (2nd; 1st is a filename hit by design);
  `query … "INV-043 TaskService" --limit 5` → rule statement in a
  non-proof file; `query … "INV-043" --scope docs/architecture
  --limit 5` → `INVARIANTS.md:19` verbatim; `query … "trait
  PlannerPort" --limit 3` → `plan/model.rs:638`. Same evidence
  standard as R2. 2/2.
- Q4: `query … "stays VERIFYING" --limit 3` → 3 non-proof lines
  (643 total, path-major order); same `--scope crates/evaluation`
  → `error.rs:3` + `port.rs:14`, exactly R2's citation. 2/2.
- Q5: `query … "build_verification_runner
  IsolationRequirements::new" --scope bins/agentd --limit 5` →
  strict sites 851/860/3647 + factory binding 3633 (R1's
  ambiguity reproduced); `show … <id-3633> --context 16` →
  factory window 3617-3649 with the lazy closure,
  `RunnerLaunchPurpose::Provider` (3643) and strict
  `new(true, true, true)` (3647) inside it — ambiguity resolved
  structurally. 2/2.
- Q6: `query … "workspace-less schedule" --limit 3` → `:955`
  first, rationale cut mid-comment; `show … <id> --context 1` →
  954-956 full ("`finish` is shared by the workspace-less
  `schedule` path, which has no workspace to execute against"). 2/2.
- Q7: `query … "validate_plan_commands refuses V0" --limit 5` →
  `verification_execution.rs:27` refusal doc verbatim first. 2/2.
- Q8: `query … "append_observed_effect" --limit 5` → store def
  `observed_effect_store.rs:12` + runner call `:346`; `show …
  <id-346> --context 4` → call args (no `ProcessExit` in range);
  `query … "ProcessExit" --scope crates/app-services/src --limit
  6` → payload built at `verification_execution.rs:317`;
  `query … "mint_candidate_verdicts" --limit 3` → def
  `candidate_verdict.rs:37` + orchestration call `:931`. 2/2.

Totals: relevance 16/16, support 16/16, 30,710 output bytes
(~7,678 tokens), 2,165 input bytes (~541 tokens), 10.8s elapsed.

### Run R5 — finder repeat (2026-10-07, same hardware/corpus)

Verbatim replay of the 19 R4 calls (`/tmp/task0147-r5.sh`,
generated from the R4 log; multi-word query argv re-split and
rejoined by the parser, so argv-equivalent). Log
`/tmp/task0147-r5.log` (33,866 bytes, sha256 `519bc27c7ecc8122…`).

All 19 outputs byte-identical to R4 (30,710/30,710 bytes; only the
timing lines differ): 9.7s elapsed. Scores identical, 16/16. R5
proves cross-process determinism and confirms the cost-axis gap is
structural, not operator noise.

### Phase B reading

Scores equal R2/R3 at 16/16 with matched-or-better evidence
(Q5 disambiguated structurally; every citation rev-bound and
hash-verified). The phase-A bar — beat R2/R3 on elapsed AND
output tokens at equal scores — is NOT met: elapsed 10.8/9.7s vs
2.8/1.0s (~4–10×), output tokens ~7,678 vs ~2,654/3,142 (~2.6×).
The one cost axis the finder wins is inputs (~541 vs ~700–825:
fewer, denser requests).

Structural causes, all measured consequences of the design, not
accidents: (1) the evidence-id tax (~200 chars/hit) — the
verifiability mechanism IS the overhead; (2) one unindexed
full-corpus scan per query (~0.5–0.9s each); (3) 19 vs 12–14
calls — (terms, path, line) ranking cuts proofs off tight pages
(Q1 continuation, Q4 rescope); (4) single-line excerpts force
`show` for multi-line proofs (5 of 19 calls).

What the bar does not measure is what phase B bought:
byte-identical reproducibility (R5), rev-bound staleness
detection, explicit skips with no silent truncation, the UI-rule
separation (the CLI never reads the repo), and matched-term
explanations on every hit. The RTK-style baselines condense
provenance away; the finder keeps it — and that is the token
cost. The phase-A bar conflated retrieval cost with evidence
quality.

Consequence: accept phase B as the evidence-quality milestone it
is. The cost axes need follow-ups that do not change the seam:
evidence-id shortening (hash16, ~5–8%), an index (phase C) for
elapsed, symbol-aware ranking to cut calls. Security filtering
status for the TASK-0147 exit criteria: `PassthroughFilter`
shipped, the `ContentFilter` seam reviewed (whole-window
filtering pinned by tests); INV-008 redaction stays the named
follow-up.

## 8. Phase C round (TASK-0173): R6–R7

Same question set (section 4), same corpus (`/tmp/task0147-corpus`,
detached clean `40754ae` — still byte-clean, so R6 runs new
binaries against the identical checkout state for a direct A/B),
same exclusions (section 2). New binaries: fresh
`target/debug/agentd` + `agentctl` from branch
`task-0173-finder-cost` (hash16 `rf2` evidence ids, symbol-aware
ranking: definition + whole-token score boosts, external-usage
file tiebreak; no seam change). Methodology deviation, recorded:
the daemon ran with `AI_ENGINEERING_OS_RUNTIME_DIR` and
`AI_ENGINEERING_OS_DATA_DIR` overrides (fresh socket + fresh
database) because the canonical local database (a v27
dev-artifact missing `owner_presence_proofs`) fails the 0030
migration — reproduced identically with the unmodified TASK-0172
binary, so pre-existing and out of scope. Retrieval never touches
the database; the overrides only isolate daemon startup.

### Run R6 — finder run 2 (2026-10-08, arm64, Apple M2 Pro)

18 calls (per-question: 1,2,4,2,2,2,1,4). Rig
`/tmp/task0173-r6.sh`, raw log `/tmp/task0173-r6.log` (session
artifacts, timed verbatim, 29,369 bytes, sha256
`cc8aed1a…f63e7`). Contamination: zero hits in this
file; `.git/`/`target/`/`.claude/` never surfaced. Every response
bound to rev `40754ae…b2f51`, clean. All evidence ids `rf2.`
(zero `rf1.`); every `show` reopened under the stored hash16.

- Q1: `query … "run_advisory_verification
  continue_workspace_cycle" --scope crates/app-services --limit 10`
  → both definitions top-two (:91, :468) AND the post-step call
  :669 fourth (external-usage tiebreak pages it past the
  callee-file mentions). No continuation: def+call same-file
  ordering on one page — the same inference R4 scored 2/2 on,
  one call fewer. 2/2.
- Q2: `query … "TASK-0165" --scope planning/tasks --limit 5` →
  header `queue.yaml:5516` (1/1); `show … <id> --context 40` →
  window 5476-5556 with the full `exit_criteria_summary` value
  verbatim. 2/2.
- Q3: `query … "struct TaskService" --limit 5` → def
  `task_service.rs:52`; `query … "INV-043 TaskService" --limit 5`
  → non-proof file (unchanged: hyphenated `INV-043` never forms
  a symbol token, so no ranking signal); `query … "INV-043"
  --scope docs/architecture --limit 5` → `INVARIANTS.md:19`
  verbatim; `query … "trait PlannerPort" --limit 3` →
  `plan/model.rs:638` first (`.md` lookalike `trait` lines lost the
  Rust-only definition boost). Same evidence standard as R4. 2/2.
- Q4: `query … "stays VERIFYING" --limit 3` → 3 non-proof lines
  (643 total; the proof ties with other both-terms-as-tokens
  lines and path order still buries it — measured, not
  assumed); same `--scope crates/evaluation` → `error.rs:3` +
  `port.rs:14`, exactly R4's citation. 2/2.
- Q5: `query … "build_verification_runner
  IsolationRequirements::new" --scope bins/agentd --limit 5` →
  factory binding 3633 on page; `show … <id-3633> --context 16`
  → factory window 3617-3649 with the lazy closure,
  `RunnerLaunchPurpose::Provider` and strict
  `new(true, true, true)` inside it. 2/2.
- Q6: `query … "workspace-less schedule" --limit 3` → def
  `:181` first, proof `:955` second (two-term line still
  outranks every single-term competitor — the file preference
  is a tiebreak, never a promotion); `show … <id> --context 1`
  → 954-956 full rationale. 2/2.
- Q7: `query … "validate_plan_commands refuses V0" --limit 5` →
  def `:28` first, refusal doc `:27` verbatim second (adjacent
  proof block; def-first is the intended new order). 2/2.
- Q8: `query … "append_observed_effect" --limit 5` → store def
  `:12` + runner call `:346`; `show … <id-346> --context 4` →
  call args (no `ProcessExit` in range, as in R4); `query …
  "ProcessExit" --scope crates/app-services/src --limit 6` →
  payload built at `:317`; `query … "mint_candidate_verdicts"
  --limit 3` → def `:37` + orchestration call `:931`. 2/2.

Totals: relevance 16/16, support 16/16, 26,704 output bytes
(~6,676 tokens), 1,976 input bytes (~494 tokens), 11.1s elapsed.

### Run R7 — finder replay (2026-10-08, same hardware/corpus)

Verbatim replay of the 18 R6 calls (`/tmp/task0173-r7.sh`,
generated from the R6 log; same argv-equivalence caveat as R5).
Log `/tmp/task0173-r7.log` (29,368 bytes, sha256
`41b70414…f7f3bc2`).

All 18 outputs byte-identical to R6 (26,722/26,722 body bytes;
only the timing lines differ), all exit 0. Scores identical,
16/16. R7 proves the ranking change keeps cross-process
determinism: the new order is a pure function of query and
corpus. On verifiability: every `show` re-verified under
hash16, so nothing is lost against accidental drift — but the
16-hex handle hash is a 64-bit prefix, not a
collision-resistance boundary (birthday collisions need ~2^32
crafted lines; the revision binding still covers committed
tampering, and the window is in-revision drift only).

### Phase C reading (TASK-0173)

R6 matches R4/R5 at 16/16 with one fewer call (18 vs 19: Q1's
continuation cut by the external-usage tiebreak) and ~1,000
fewer output tokens (~6,676 vs ~7,678: ~745 from hash16 over 62
hit lines plus the dropped continuation page). The token math
reconciles exactly with the mechanism — no unexplained delta.
Elapsed is unchanged (11.1s vs 10.8s): ranking and ids were
never going to fix the unindexed scan; that stays TASK-0174's
job, as scoped. Q4's rescope survives by measurement (ties no
ranking signal breaks without file-identity knowledge), and Q3
stays 4 calls for a vocabulary reason (hyphenated terms are not
symbol tokens) — both documented above so TASK-0174's index
work does not re-litigate them. No seam changed: `EvidenceId`
stays opaque, `rf1` still parses, scores stay adapter-local,
the contract suite is untouched.
