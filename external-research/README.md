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
- `studies/` — cross-product architecture studies. These answer one question
  across several external systems rather than auditing one repository. Their
  evidence level is stated in each file and is weaker than `repos/` when the
  sources are vendor documentation rather than source.
- `PATTERN_LIBRARY.md` — reusable architecture concepts, independent of backlog.
- `ANTI_PATTERN_LIBRARY.md` — reusable failure/security lessons.
- `IMPLEMENTATION_KNOWLEDGE_QUEUE.md` — only evidenced gaps that are eligible for later architecture/task consideration.

## Current re-audit set

| Repository | Reviewed revision | Report |
|---|---|---|
| cosmicstack-labs/mercury-agent | `f465e6fb9e8c90712c46f8003801ef822bfe5320` | `repos/mercury-agent.md` |
| Nitjsefnie/Overflow | `eb11283d8535028702ff17f7ed1d928c8d069166` | `repos/overflow.md` |
| NVIDIA/OpenShell | `eeba0e7954c0fb4d8e9e2e29d1bfa68e290eb8b3` | `repos/openshell.md` |
| 0xK3vin/MegaMemory | `e0bb3c270d7fb4f6f280ae4685e0c538eb225d93` | `repos/megamemory.md` |
| jonaslsaa/pi-optchat | `0aa45406325f610f3106510218d8cc69deb0cccc` | `repos/pi-optchat.md` |
| deeplethe/utopia | `1a75be63140ff1a94147524352a8b74647830a5e` | `repos/utopia.md` |
| opensandbox-group/OpenSandbox | `fdb06ac7d392a1d8570e563857d796bacd7250c4` | `repos/opensandbox.md` |
| Bitdefender AI Guardian (docs, no SHA) | last-updated 2026-10-09, reviewed 2026-10-10 | `repos/bitdefender-ai-guardian.md` |
| langchain-ai/langgraph | `b4991f1ba36a2449ab59c38ab71828ee865bff84` | `repos/langgraph.md` |
| pydantic/pydantic-ai | `69ea1e57201ed59e7d30fbe4a7590d193acecce0` | `repos/pydantic-ai.md` |
| deepseek-ai/deepseek-harness | `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720` + addendum `d743267388641bc76f17c45ce8b4c231aed1d32c` | `repos/deepseek-harness.md` |
| getzep/graphiti | `964bb11d1e7020e3cc67fab244268b0736b6c219` | `repos/graphiti.md` |
| confident-ai/deepeval | `4598fe8eb7be713637fbc803b9f127b5b83de69a` | `repos/deepeval.md` |
| mastra-ai/mastra | `2d177b025ddea32aa511c3a9d5a9ac78f7290bbc` | `repos/mastra.md` |
| langfuse/langfuse | `a416d22f9534609b8850d17ea2e8f2531fbcbfde` | `repos/langfuse.md` |
| topoteretes/cognee | `0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1` | `repos/cognee.md` |
| agno-agi/agno | `24bc39f137f7f5fb5b1df89e3065914e01c0e901` | `repos/agno.md` |
| alphaXiv/OpenResearch | `603558cbf18207fdf758cc50b317a06e01ba2354` | `repos/openresearch.md` |

## Studies

| Study | Date | Report |
|---|---|---|
| Karpathy on LLM output comprehension (X post, no SHA) | 2026-10-10 | `studies/2026-10-10-karpathy-llm-output-comprehension.md` |
