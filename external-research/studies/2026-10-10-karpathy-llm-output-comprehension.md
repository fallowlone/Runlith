# Study

Karpathy on LLM output comprehension (X post + ASD-STE100 sheet)

- url: https://x.com/karpathy/status/2105819303471976479
- resource_type: social post with attached infographic (no repo, no SHA)
- review_date: `2026-10-10`
- access_note: fetched via user-pasted text 2026-10-10; direct X fetch
  failed (no creds, Jina domain-blocked, oembed empty). Text is
  user-supplied evidence; image inspected inline (ASD-STE100 one-page
  sheet, source asd-ste100.org, "1 of 1").

# Thesis (source summary)

As LLMs do more legwork, human work rises into oversight and
understanding — and LLMs themselves supply the comprehension ladder:

1. Constrained writing: explain in ASD-STE100 (aerospace controlled
   language; heavy readability constraints), optionally softened
   ("80% of the way to ASD-STE100").
2. Diagrams/images over prose: easier to parse than writing.
3. HTML pages over static diagrams: beautiful interactive artifacts;
   LLMs are strong at frontend.
4. Bespoke explainer videos over pages: most bullish format — custom
   3b1b-style explainer on any topic, narrated via ElevenLabs key or
   free local-compute alternatives. "Starting to work."

Closing claim: intelligence and code are increasingly abundant, so
large custom *discardable* software artifacts (web apps, video
explainers) are now economical where they never made sense before.
Push the boundaries; expect surprise.

# ASD-STE100 constraints (from attached sheet)

- Procedural sentence ≤ 20 words; descriptive ≤ 25; paragraph ≤ 6
  sentences, one topic each; noun cluster ≤ 3 words; one instruction
  per sentence (simultaneous actions excepted).
- Approved dictionary (~900 words, one word = one part of speech =
  one meaning; approved shown UPPERCASE); unapproved mapped
  (commence→START, ensure→MAKE SURE, prior to→BEFORE, utilize→USE).
- Command (imperative) and simple present approved; progressive,
  perfect, passive-in-procedures not approved (passive only inside
  technical names, e.g. "landing gear", when necessary).
- Active voice in procedures; same word for the same thing every time;
  no omitted words ("the", "a", "this"); vertical lists for complex
  text; WARNING = risk of injury, CAUTION = risk of damage.
- History: 1979 AECMA start → 1986 guide → 2005 ASD-STE100 (ASD from
  2004) → now free, maintained by STEMG; use spread from aircraft
  maintenance to defence/rail/industry.

# Findings for Runlith

- F-01 — Constrained-language output: STE's caps (20/25/6/3-word,
  approved-dictionary, active voice) are a ready-made spec for
  machine-generated summaries humans must oversee quickly.
- F-02 — Format ladder (text → diagram → interactive HTML → video):
  comprehension scales with artifact richness, not prompt length.
- F-03 — "80% STE" softening: full spec is stringent; partial
  compliance keeps most readability gain — a tunable strictness knob.
- F-04 — Discardable bespoke artifacts: one-off review/diagnosis UIs
  are worth generating per-incident when generation is cheap.
- F-05 — Oversight framing: the product surface that matters most is
  what the human reviews (verdicts, audits, diffs), not what the
  agent generated.

# Comparison with our project

## What is better in Runlith

- The operator's own harness already cites ASD-STE100 as a prose
  floor (20 words max, active voice, one idea per sentence) — the
  discipline exists as practice, though not as product surface.
- Hash-chained audit + journal give oversight *trustworthy* content;
  Karpathy's ladder addresses *legibility*, which presupposes content
  worth reading — Runlith owns the harder half.
- Evaluation Port separates verdict from execution; a verdict summary
  is exactly the artifact class STE-constrained output fits.

## What is worse in Runlith (gaps this exposes)

- No format ladder for human review: agentctl/audit outputs are
  terminal text; no diagram/HTML export for run review (P3, R-02).
- No constrained-language option for generated summaries (verdicts,
  audit explanations, scan findings) — P3, R-01.
- Debug/diagnosis tooling is built to keep, not to discard; the
  discardable-artifact principle is uncited (P3, R-03).

## What is better in the source

- Names the real bottleneck (oversight, not generation) and gives an
  ordered, experimentally-grounded ladder, cheapest first.
- "80% STE" concedes spec stringency without abandoning it — tunable
  strictness beats all-or-nothing adoption.
- Discardable-software economics stated plainly: abundance changes
  what artifacts are worth creating.

## What is worse in the source

- No trust story: a beautiful bespoke explainer can mislead faster
  than plain text; no provenance, no verification, no bounds.
- Vendor-dependent top rung (ElevenLabs key) with a hand-waved local
  alternative; no offline posture.
- Marketing-adjacent ("you'll be surprised") with no failure modes,
  no cost/quality measurement, no adversarial note.

# Opportunities and recommendations

- R-01 — Offer STE-constrained summaries for generated review text
  (verdict explanations, audit findings, scan reports), default off,
  strictness tunable (full vs "80%"). P3, `NOVEL_GAP` → `ADAPT`
  (requirements-level; needs an ADR before product claims).
- R-02 — Diagram/HTML export for run review (audit day-stream,
  task lifecycle) as review surfaces, not storage. P3,
  `NOVEL_GAP` → `INSPIRED_ONLY` (view layer; cite alongside
  AI-Guardian R-04 calendar notes).
- R-03 — Cite discardable-artifact economics for one-off
  diagnosis/debug scaffolding (generate per incident, keep only
  findings). P3, `NOVEL_GAP` → `EXTERNAL_VALIDATION`.
- REJECT: video generation in product; ElevenLabs/cloud-narration
  dependency; replacing chain-of-custody text (audit/journal) with
  generated artifacts — legibility never substitutes integrity.

# Evidence and references

- Post text (user-supplied 2026-10-10) + inline image (STE sheet,
  "Simplified Technical English: overview", asd-ste100.org).
- Method limits: single opinionated source, no repo, no verification;
  treated as hypothesis input per README Method, not authority.
- Runlith references: operator harness ASD-STE100 floor (global
  AGENTS.md caveman block); `AGENT.md` §§4/7; G6 audit review notes
  (AI-Guardian R-04); G8 verification scope.

# Final assessment

Useful as UX philosophy, not architecture: the oversight ladder
(text→diagram→HTML→video) and STE constraints are directly citable
for review-surface work (R-01…R-03, all P3), while the trust gap
(beautiful ≠ verified) confirms Runlith's ordering — integrity
(chain, bounds, verifier) first, legibility second. Keep as study
evidence; no IKQ entry (no evidenced product delta).
