# Re-audit

confident-ai/deepeval — pytest-style LLM evaluation framework
(metrics + datasets + tracing) with LLM-as-a-judge verdicts.

- repo: https://github.com/confident-ai/deepeval
- reviewed_revision: `4598fe8eb7be713637fbc803b9f127b5b83de69a`
  (main, 2026-10-10T00:15:09Z — `Merge pull request #3417 …
  feat(dataset): send the pulled dataset version with test runs`)
- version at pin: 4.2.8 (`[tool.poetry] version` in `pyproject.toml`)
- license: Apache-2.0 (LICENSE.md, Confident AI Inc.)
- language/stack: Python >=3.9, pytest plugin (`pytest11` entry
  point), pydantic v2, OpenTelemetry (api+sdk), `typer` CLI,
  `deepeval login` sync to Confident AI cloud
- method: raw-file reads at pin + Atom commit feed. No clone.
  Scoped to verdict shapes, GEval rubric, test-case/dataset
  schemas, tracing spans, red-team location, judge-model limits.
  Metric bodies (faithfulness, answer relevancy, agentic, RAG),
  evaluator internals, integrations, CLI not line-read.

# What it is (source summary)

A unit-test harness for LLM apps: `LLMTestCase{input,
actual_output, expected_output, context, retrieval_context,
tools_called, expected_tools, …}` is fed to one or more metrics;
each metric yields `score ∈ [0,1] + reason`, and
`is_successful() = score >= threshold` decides pass/fail.
Two judge vocabularies recur: strict binary `yes|no` and a
three-bucket `yes|no|borderline` for ambiguous items, each metric
owning its borderline scoring. GEval is the flagship custom-
criteria judge: free-text `criteria` XOR stepwise
`evaluation_steps`, optional `Rubric[{score_range, expected_outcome}]`
bands, logprob-weighted averaging over the judge's score token
(OpenAI-shaped), normalized to 0–1. Datasets are `Golden` rows
(mirror of test-case fields) pulled/pushed with a dataset
*version*; tracing is a typed span tree (`agent|llm|retriever|
tool`) that metrics can evaluate directly (`requires_trace`).
Red-teaming no longer lives here (`red_teaming/README.md`:
moved to DeepTeam for v3+). Roadmap lists Guardrails unchecked.
Everything judge-flavored assumes a model key; cost/token
bookkeeping exists only for native-model runs.

# Architecture (as evidenced)

- Metric verdict shape (`metrics/base_metric.py`, full read):
  `BaseMetric{threshold, score, score_breakdown, reason, success,
  error, evaluation_cost, input_tokens, output_tokens,
  verbose_logs, strict_mode, async_mode, flaky, skipped}`.
  `is_successful()`: threshold None → success None; error set →
  False; else `score >= threshold` (TypeError → False). Three
  subclasses: `BaseMetric` (LLMTestCase), `BaseConversationalMetric`,
  `BaseArenaMetric` (returns preferred-side string, own
  `is_successful`). Error-forces-fail is the single fail-closed
  rule in the verdict path.
- QAG verdict vocabulary (same file, full read): `Verdict(str,
  Enum){YES="yes", NO="no", BORDERLINE="borderline"}`; members
  subclass str so they serialize as plain strings; `YesNo` /
  `YesNoBorderline` Literal aliases for judge schemas;
  `YES_NO` / `YES_NO_BORDERLINE` allowed-vocabulary tuples;
  `LEGACY_VERDICT_ALIASES{"idk": BORDERLINE}` for stale cached
  prompts. Header comment: every yes/no metric classifies a list
  of items (claims, statements, …) into one vocabulary; each
  metric decides borderline scoring (`score_qag_verdicts` in
  `metrics.utils`, cited not read).
- GEval (`metrics/g_eval/g_eval.py`, full read): `name` +
  `evaluation_params[List[SingleTurnParams]]` + (`criteria` XOR
  `evaluation_steps`) + optional `rubric` + `threshold=0.5`
  (`strict_mode` forces threshold 1 and integer scores).
  `requires_trace = not evaluation_params`: without params the
  metric evaluates `test_case._trace_dict` serialized to JSON —
  the trajectory *is* the test subject. Score normalized
  `(g_score − lo)/(hi − lo)` over rubric range (default 0–10);
  `_a_evaluate` uses `generate_rubric_score` with
  `calculate_weighted_summed_score` (logprob-weighted mean over
  the score token). `upload()`/`pull()` sync criteria/steps/
  rubric to Confident AI; trace-mode GEvals cannot upload
  (stated).
- Rubric helpers (`metrics/g_eval/utils.py`, full read):
  `Rubric{score_range: Tuple[int,int] (each end 0–10, start ≤
  end), expected_outcome: str}`; `validate_and_sort_rubrics`
  sorts by band start and rejects *any* overlap; `format_rubrics`
  renders `start-end: outcome` lines into the judge prompt;
  `get_score_range` falls back to (0,10). Param-name maps exist
  in triples: display (`G_EVAL_PARAMS`), API (`G_EVAL_API_PARAMS`),
  conversational variants — prompt wording and wire format kept
  as separate dictionaries.
- Test-case schema (`test_case/llm_test_case.py`, full read):
  `LLMTestCase{input: str, actual_output?, expected_output?,
  context?, retrieval_context?, tools_called?, expected_tools?,
  metadata?, expectations?, expected_labels?, token counts?,
  completion_time?, mcp_*?, multimodal auto-detect}`.
  `SingleTurnParams` enum (INPUT, ACTUAL_OUTPUT,
  EXPECTED_OUTPUT, CONTEXT, RETRIEVAL_CONTEXT, TOOLS_CALLED,
  EXPECTED_TOOLS, …) is the explicit per-metric input selector:
  a metric declares which fields it reads and the harness
  validates presence (`check_llm_test_case_params`).
  `ToolCall{name, type: FUNCTION|MCP, description?, reasoning?,
  input_parameters?, output?}` with value-equality + hash.
  `expected_labels: Dict[classifier-name, label]` gates
  classifier pass/fail per test case.
- Dataset schema (`dataset/golden.py`, full read): `Golden`
  mirrors test-case fields (input/outputs/contexts/tools/costs/
  expectations/expected_labels) plus `_dataset_rank/_dataset_alias/
  _dataset_id/_dataset_version` PrivateAttrs; `ConversationalGolden
  {scenario, expected_outcome, persona, turns}` separates *who*
  (`Persona{characteristics, metadata, voice…}`) from *what*
  (`scenario` + `expected_outcome`). Pin commit (#3417) sends the
  pulled dataset version with test runs — version linkage is
  current work, not finished design.
- Tracing span model (`tracing/types.py`, full read):
  `BaseSpan{uuid, status, children, trace_uuid, parent_uuid,
  start/end_time, name?, metadata?, input?, output?, error?,
  llm_test_case?, metrics?, expected_*?}`; typed spans
  `AgentSpan{available_tools, agent_handoffs}`,
  `LlmSpan{model, provider, prompt{+alias/version/label/commit},
  token counts/costs}`, `RetrieverSpan{embedder, top_k,
  chunk_size}`, `ToolSpan{name, description}`;
  `TraceSpanStatus{SUCCESS, ERRORED, IN_PROGRESS}`;
  `Trace{uuid, status, root_spans, thread/user ids, test_case/
  run/turn ids}`. `EvalSession{mode: OFF|EVALUATE|
  ITERATOR_SYNC|ITERATOR_ASYNC, pending_traces (insertion-ordered
  dict, start order preserved), traces_to_evaluate (deduped
  queue), trace_uuid_to_golden}` — per-run eval state reset by
  single assignment.
- Judge-model posture + stated limits: README claims ANY LLM
  incl. local NLP models; source shows fidelity tiers —
  logprob weighting is OpenAI-shaped (`OPENAI_MODELS_DATA`,
  `supports_log_probs`, `no_log_prob_support` fallback to raw
  score); `evaluation_cost`/token counters accrue only when
  `using_native_model`, else None (BYO-key runs are cost-blind
  in-framework). System-One (Jev) fields (`eval_mode`,
  `confidence`, `system_one_fallback_reason`) thread every
  metric: calibrated-probability answers with a recorded
  hybrid-fallback reason.
- Red-teaming: in-repo module is a stub pointer
  (`red_teaming/README.md`, full read — 3 lines): moved to
  `confident-ai/deepteam` for v3+. Attack taxonomy not
  inspectable at this pin; roadmap checkbox only.
- Commit feed at pin: dataset-version propagation (#3417),
  expectations reasoning + `verdicts` attribute (#3423),
  LiteLLM model-name/region resolution (#3424), transcription-
  accuracy metric (#3414) — eval-platform churn centers on
  dataset linkage and metric breadth, not verdict semantics.

# Findings for Runlith

- F-01 — Universal metric envelope: score + reason + threshold
  + tri-state success (None when unjudged) with error forcing
  fail. Smallest complete verdict record seen; maps 1:1 onto
  an EvaluationPort verdict struct.
- F-02 — Borderline third bucket with per-metric scoring
  policy + legacy-alias handling (`idk`). Precedent for an
  explicit "indeterminate" verdict instead of overloading
  fail — plus a migration story for renamed verdicts.
- F-03 — Rubric as validated data: sorted non-overlapping
  score bands each carrying an expected outcome, rendered
  verbatim into the judge prompt. Direct template for G8
  verification checklists with severity bands.
- F-04 — Explicit input selection per check
  (`SingleTurnParams`): each metric declares the fields it
  reads; harness validates presence. Precedent for scoping
  EvaluationPort checks to named evidence fields.
- F-05 — Trace-as-subject (`requires_trace`): when a check
  declares no direct params it evaluates the recorded
  trajectory JSON. Validates G8 verifying journal/replay
  evidence rather than fresh model output.
- F-06 — Golden/dataset version linkage (`_dataset_version`,
  pin commit #3417): fixtures carry their source version into
  the run. Precedent for pinning verification fixtures in
  Runlith's deterministic policy world.
- F-07 — Typed span taxonomy with terminal statuses and
  per-run eval session (modes, deduped queue, trace→golden
  map). Useful contrast object for Runlith's journal — richer
  span types, weaker durability claims.
- F-08 — Judge-fidelity tiers honestly scoped: logprob
  weighting only where supported with raw-score fallback;
  cost accounting only for native runs. A checklist for what
  Runlith must *refuse* (model-judged enforcement, silent
  cost-blindness).

# Comparison with our project

## What is better in Runlith

- Verdicts are deterministic and policy-bound (G7 closed,
  EvaluationPort for independent verification), not
  model-sampled: no judge calibration, no logprob shaping, no
  provider-shaped fidelity tiers.
- Durable substrate is local SQLite + hash-chained journal;
  deepeval's run evidence (traces, goldens, rubrics) round-
  trips through Confident AI cloud (`upload`/`pull`,
  `deepeval login` recommended path).
- Version linkage is structural in Runlith (policy hash,
  content-addressing); deepeval's dataset-version propagation
  is week-old feed work on a cloud-backed store.
- No cost-blindness ambiguity: Runlith has no model spend at
  all; deepeval accrues cost only for native runs and leaves
  BYO-key evaluation unpriced in-framework.

## What is worse in Runlith (gaps this exposes)

- No indeterminate verdict: pass/fail has no `borderline`
  equivalent, no per-check ambiguity scoring, no verdict-
  rename alias story (P3, R-02).
- No rubric-shaped checklist: G8 checks lack sorted non-
  overlapping severity bands with expected outcomes per band
  (P2, R-03).
- Checks don't declare their evidence inputs; no
  SingleTurnParams-style selector + presence validation
  (P3, R-04).
- Fixture versioning for verification inputs is unowned;
  `_dataset_version`-style source linkage doesn't exist
  (P3, R-06).
- Verdict record has no standard reason/cost/token/error
  envelope fields to converge on (P2, R-01).

## What is better in the source

- `Verdict` enum discipline: str-backed members, Literal
  schema aliases, allowed-vocabulary tuples, legacy alias map —
  typo becomes AttributeError/import-time, never silent score.
- Rubric validation (`validate_and_sort_rubrics`) rejects
  overlap instead of documenting around it; score-range
  fallback (0,10) keeps un-rubricked judges total.
- Trace-evaluating metrics: the trajectory-as-subject mode
  with an explicit error when the trace is absent
  (`MissingTestCaseParamsError`), not a None-score mystery.
- `EvalSession` documents every field's producer/consumer and
  resets by single assignment — per-run state discipline
  Runlith's services could mirror.

## What is worse in the source

- Enforcement is model opinion: weighted token probabilities
  over a judge's score token, provider-gated fidelity, no
  determinism claim anywhere in the verdict path.
- Cloud coupling is the default: dataset pull, metric pull/
  upload, trace streaming, MCP-server persistence layer all
  assume Confident AI; local-first is a README claim against
  a `login`-recommended reality.
- Red-teaming evacuated to a second repo (DeepTeam); the
  in-tree taxonomy is a 3-line pointer — nothing to adapt.
- Guardrails sit unchecked on the roadmap; adversarial
  coverage is marketing surface, not source.
- Cost/token/error accounting is partial by construction
  (native-model-only costs, TypeError-swallowed success).

# Opportunities and recommendations

- R-01 — Adopt the metric envelope for EvaluationPort
  verdicts: `{score ∈ [0,1], reason, threshold, success ∈
  {true,false,null}, error}` with error-forces-fail and
  null-when-unjudged (F-01). P2, `ADAPT` (record shape only;
  deterministic producers).
- R-02 — Add an explicit indeterminate verdict with per-check
  scoring policy + alias map for renames (F-02, cf. pydantic
  R-02 verdict algebra). P3, `ADAPT` (needs ADR; indeterminate
  must never gate-admit).
- R-03 — Shape G8 verification checklists as validated
  rubrics: sorted non-overlapping bands, each with an expected
  outcome, rendered verbatim into review surfaces (F-03).
  P2, `ADAPT` (docs + types; no judge import).
- R-04 — Give each EvaluationPort check a declared evidence
  selector (named journal/replay fields) with presence
  validation at admission, mirroring `SingleTurnParams`
  (F-04). P3, `ADAPT`.
- R-05 — Cite `requires_trace` trajectory-as-subject (F-05)
  as `EXTERNAL_VALIDATION` for G8 verifying journal/replay
  evidence. P3.
- R-06 — Cite dataset-version linkage (F-06, pin commit
  #3417) as `EXTERNAL_VALIDATION` for pinning verification
  fixtures to source versions. P3.
- R-07 — Cite span taxonomy terminal statuses
  (SUCCESS/ERRORED/IN_PROGRESS) + per-run eval-session reset
  (F-07) as `INSPIRED_ONLY` contrast for journal recovery
  docs; Runlith's durability model stays. P3.
- REJECT: Python framework/pytest-plugin dependency; LLM-
  as-judge dependence incl. logprob-weighted scoring and Jev/
  System-One fields; Confident AI cloud (login, dataset/metric
  upload/pull, trace streaming, MCP persistence); DeepTeam
  red-teaming import (not in-tree at pin); provider-SDK cost/
  token accounting; guardrails (absent at pin).

# Evidence and references

- Pin: `main@4598fe8e…` via Atom feed `commits/main.atom`
  (2026-10-10T00:15:09Z, merge #3417; prior entries #3423
  verdicts-attribute, #3424 litellm region, #3414
  transcription-accuracy).
- Full pinned raw URLs (verified at pin):
  - README (framework/metrics/tracing overview):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/README.md
  - version + deps (4.2.8, pytest plugin, OTel):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/pyproject.toml
  - license (Apache-2.0):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/LICENSE.md
  - metric envelope + Verdict enum (full read):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/metrics/base_metric.py
  - GEval judge (full read):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/metrics/g_eval/g_eval.py
  - rubric validation + param maps (full read):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/metrics/g_eval/utils.py
  - test-case schema + SingleTurnParams + ToolCall (full read):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/test_case/llm_test_case.py
  - dataset Goldens + version attrs (full read):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/dataset/golden.py
  - span/trace/eval-session types (full read):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/tracing/types.py
  - red-team stub pointer (full read, 3 lines):
    https://raw.githubusercontent.com/confident-ai/deepeval/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/red_teaming/README.md
  - red_teaming dir listing (1 tree fetch; only README.md
    in-tree at pin):
    https://github.com/confident-ai/deepeval/tree/4598fe8eb7be713637fbc803b9f127b5b83de69a/deepeval/red_teaming
- Not inspected (honest scope): metric bodies (faithfulness,
  relevancy, agentic/RAG/multimodal), `score_qag_verdicts`
  borderline scoring, judge-model classes, integrations,
  CLI, DeepTeam repo (out of scope per method).
- Failed raw guesses (documented, no content claimed):
  `deepeval/red_teaming/types.py`, `deepeval/red_teaming/
  __init__.py` (both 404 — consistent with v3 stub).
- Runlith references: G7 planning closed; G8 verification
  in_progress; EvaluationPort verdict vocabulary; pydantic-ai
  R-02/R-03 (verdict algebra, advisory gate).

# Final assessment

Highest-yield ideas-to-steal source for G8 verdict design:
the metric envelope (R-01) and validated rubrics (R-03) are
the two P2 ADAPT items — small record shapes with the
source's own validation logic doing half the ADR work — while
borderline verdicts (R-02), evidence selectors (R-04), and
trajectory-as-subject (R-05) round out the P3 set. Everything
that makes deepeval *run* (judge LLMs, logprob weighting,
cloud sync, DeepTeam, provider accounting) stays REJECT, and
the red-team taxonomy is absent at pin rather than thin — a
finding in itself. No IKQ entry: requirements-level deltas
only, deterministic producers throughout.
