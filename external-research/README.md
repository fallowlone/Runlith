# External Architecture Intelligence

This directory contains source-level research on external repositories. It is architecture intelligence, not an implementation backlog and not an authority source for Runlith.

## Method

Every repository report is pinned to an exact upstream commit and treats previous reports only as hypotheses. Findings must flow through:

```text
external source evidence
  -> architectural finding
  -> Runlith overlap search
  -> novelty classification
  -> differentiation test
  -> adoption decision
```

Novelty classifications are `NOVEL_GAP`, `PARTIALLY_EXISTS`, `ALREADY_DESIGNED`, `ALREADY_IMPLEMENTED`, `SUPERSEDED`, `CONFLICTS_WITH_ARCHITECTURE`, and `NEEDS_ADR`.

Adoption decisions are `ADOPT`, `ADAPT`, `INSPIRED_ONLY`, `REJECT`, and `EXTERNAL_VALIDATION`.

No external repository is an implementation dependency. External code is not copied into Runlith. A candidate implementation may enter the implementation knowledge queue only when a real Runlith delta is evidenced; architecture-only or already implemented ideas remain research evidence.

## Layout

- `repos/` — repository-specific re-audits with commit-pinned source evidence.
- `PATTERN_LIBRARY.md` — reusable architecture concepts, independent of backlog.
- `ANTI_PATTERN_LIBRARY.md` — reusable failure/security lessons.
- `IMPLEMENTATION_KNOWLEDGE_QUEUE.md` — only evidenced gaps that are eligible for later architecture/task consideration.

## Current re-audit set

| Repository | Reviewed revision | Report |
|---|---|---|
| cosmicstack-labs/mercury-agent | `f465e6fb9e8c90712c46f8003801ef822bfe5320` | `repos/mercury-agent.md` |
| Nitjsefnie/Overflow | `eb11283d8535028702ff17f7ed1d928c8d069166` | `repos/overflow.md` |
