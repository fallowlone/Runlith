# D05 — Task Workspace A/B/C static prototypes

Date: 2026-09-21. Worktree: `design-d05-task-workspace-prototypes` (branch off `main` @ `3474a58`).

## Scope (per user instruction)

First implementation-style artifact of the design track: **comparable A/B/C Task
Workspace prototypes** with identical data and styling, per
`docs/ui/research/2026-09-17-runlith-ui-ux-research.md` §18 (D05) and the
layout/interaction contract of §9–§15.

## What this is not

- Not an approved design, not a frontend stack ADR (D08 remains deferred by
  roadmap), not a TASK-XXXX, no queue/roadmap changes.
- No production code, no backend calls, no dependencies; static HTML/CSS/vanilla JS.
- D02 (UI Product/IA contract) and D04 (Attention/HDR/Approval interaction
  contract) do **not** exist yet. Per the user's explicit decision, their
  requirements are taken here **as design-fixture assumptions** and are
  recorded below so a later D02/D04 can confirm or supersede them.

## Recorded envelope assumptions (D02/D04 stand-ins)

1. Task Workspace = stable header (objective, canonical lifecycle, one most
   important reason, next allowed command) + tabs Overview / Plan / Changes /
   Evidence (+ History in C) — the §9.1 candidate IA.
2. State presentation follows §12: lifecycle, reason, freshness are separate
   layers; succeeded Attempt ≠ COMPLETED; failed Attempt does not recolor the
   Task; no evidence ≠ failed; stale evidence is explained, old verdicts are
   never rewritten.
3. HDR choices come from the request only (RESUME/REPLAN/ABORT; no TAKEOVER);
   recommendation is shown separately from the choice; no timer applies it;
   recorded ≠ applied.
4. Expired/stale approval stays readable, mutation actions are disabled with an
   explanation, and a path to the current request remains.
5. No hidden unsupported actions: no Cancel-Task control, no universal Resume,
   terminal output is read-only and labeled, plan proposal never renders as an
   acting plan, diff names its base revision.

## Fixtures

Fictional scenario (§12.3): TASK-0342 VERIFYING, Attempt 1 TIMED_OUT, Attempt 2
SUCCEEDED, criteria C1/C2 VERIFIED against snapshot `5e11d0…`, C3 pending with
no evidence; earlier C1 verdict bound to older snapshot `4d22…` shown as
staleness note; child TASK-0344 blocked by pending HDR-0092 (NO_PROGRESS);
expired approval APR-0041 in Attention; accepted plan r4 with provenance and a
non-acting proposed change set CS-12.

Data lives in a single `fixture.js` and panels render through a single
`proto.js`; A/B/C differ only in their layout shells, guaranteeing identical
data and styling for comparison.

## Design-fixture capabilities

- Density: compact 28px / default 32px / comfortable 40px rows (§14.1).
- Text size: 100% / 150%, independent of density (§14.1).
- Screen fixtures: 1440×1000 default, 1280×800 narrow, larger optional
  (§11.2). Sizes are fixtures, not supported product resolutions.
- Variant C viability conditions (§11-C): explicit open/close with text label,
  pinning, no self-opening and no focus stealing, diagnostics explicit,
  Attention always visible in left nav.

## How to view

Open `index.html` in a browser (file:// works; no build, no network), or:

```sh
python3 -m http.server -d docs/ui/design/2026-09-21-d05-task-workspace-prototypes 8080
```

## Acceptance criteria (D05, from research §18)

- [x] Three comparable variants (A/B/C) with the same data and visual styling.
- [x] 1440×1000 and 1280×800 fixtures plus increased-text fixture.
- [x] No hidden unsupported actions (see assumptions §5 above).
- [ ] Formative usability testing (D06) — explicitly out of scope here.

## Reported missing contract (AGENT.md §2)

- No TASK-XXXX envelope exists for design-track work; D05 is a working label
  from research §18. This worktree uses branch
  `design-d05-task-workspace-prototypes` (no task ID available to embed, per
  the D-track naming; queue was intentionally not modified).
- `product/version.toml` was **not** bumped: it is updated per completed
  TASK-NNNN, and this artifact has no task identity; queue bookkeeping is
  orchestrator authority.
- D01 (primary workflows/release boundary) and D02–D04 contracts are
  unconfirmed; their assumptions above need confirmation or supersession by
  the canonical contract tasks.

## Verification

Run from the worktree root:

```sh
python3 docs/ui/design/2026-09-21-d05-task-workspace-prototypes/verify.py
```

Checks: all pages parse as HTML, all three variants load the same fixture.js,
no unsupported actions leak (TAKEOVER, Cancel Task, active approve on the
stale approval), density/text tokens present.