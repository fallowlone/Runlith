# Planner Evidence Resolution Policy

> Classification and redaction rules for ADR-0029 option B (TASK-0175).
> Authorizes exactly one new flow: trusted planning-service resolution of
> `EvidenceRef`s to bounded classified content passed on the planner request.
> Everything not stated here stays identity-only.
>
> - Decision: [`../architecture/adr/ADR-0029-planner-evidence-resolution.md`](../architecture/adr/ADR-0029-planner-evidence-resolution.md)
> - Guarantees: [`TECHNICAL_PRIVACY_CONTRACT.md`](./TECHNICAL_PRIVACY_CONTRACT.md) (PRIV-003, PRIV-004, PRIV-010)
> - Flow registry: [`DATA_FLOW_MAP.md`](./DATA_FLOW_MAP.md) (FLOW-013)
> - Sensitivity levels: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Invariants: INV-008 (secrets never enter model context), INV-043,
>   INV-093

## 1. What may resolve

Only refs minted by the trusted runtime itself resolve. Two kinds:

| Ref shape | Source | Resolves to |
|---|---|---|
| `attempt:<attempt-id>` | durable attempt record (SQLite, canonical owner: execution/runtime store) | execution identity, provider ref, outcome (`open` while unconcluded), conclusion detail |
| `human-decision:<request-id>` | durable human decision record (SQLite, canonical owner: human decision store) | reason code, decision status, selected continuation, decided-at timestamp |

Every other ref (unknown kind, unknown id, fixture/test minted strings,
refs to records that no longer exist) stays **identity-only**: it remains in
`evidence_refs` exactly as before and never gains content. Absence of an
attached item is the complete signal; no "unresolved" or "denied" marker is
sent to the planner, because even revealing that a record was withheld
leaks one bit the planner is not owed.

## 2. Classification

Sensitivity follows `DATA_CLASSIFICATION.md`; the level travels with the
resolved item (`ResolvedEvidence.classification`):

- `attempt:` content is **PROJECT** — ordinary execution evidence.
- `human-decision:` content is **RESTRICTED** — approval-chain material.
  Only the decision outcome crosses (reason, status, selected continuation,
  decided-at). The `decided_by` human identity never crosses: it is personal
  data the planner cannot use (`PRIV-003` minimization).

**SECRET never resolves.** Credential material is never read into a
resolved item at all; the resolution sources above hold no secret values
by construction (INV-151). As defense in depth, every resolved text is
screened through the canonical INV-008 redaction screen
(`secret_redaction::redact_line`: canonical assignment keys, vendor
credential prefixes, `Bearer` tokens, PEM blocks, long base64 bodies)
before it is attached to a request. Screened items keep the surviving
text with the classification unchanged; text that is nothing but secret
material resolves to nothing (identity-only) rather than to a bare
redaction marker.

Derived content keeps its source's level (conservative inheritance); no
summary or truncation lowers it.

## 3. Bounds

Per planner call, enforced by the trusted service **before** any provider
spend is reserved:

- per item: at most 2,048 bytes of content;
- per call: at most 4 items and at most 8,192 bytes of total content;
- token estimate: ceil(bytes / 4) per item plus 16 tokens of framing per
  item, charged against `PlanningBudget.input_tokens_remaining`.

Exceeding the per-call bound refuses the planning call before invocation
(zero provider spend), the same semantics as a pre-call guardrail refusal.
Truncation never silently drops the tail of an item mid-content: over-long
item content is cut at a byte cap with an explicit `…[truncated]` suffix so
the planner can see the fact of truncation (INV-127 spirit: degraded
content stays explicit).

Growth is measured, not estimated once: resolved bytes are inside the
planner prompt, so the adapter-reported `PlanningCostReceipt.input_tokens`
already includes them and keeps settling through the existing durable
budget machinery.

## 4. Disclosure (PRIV-010)

Resolved content leaves the device with the provider call exactly like
FLOW-003 selected context. The transfer is observable through the existing
planning evidence: the request records which refs resolved, the durable
planner-call settlement records the measured input tokens, and the
provider/model identity comes from the adapter's provenance. No resolved
content is written to operational logs or journal payloads — the journal
keeps refs and hashes only (INV-008).

## 5. Non-goals

- No new read path for the planner: adapters never resolve refs, never
  query stores, never hold handles. They render attached content only.
- No vector database, no retrieval ranking: resolution is exact-id lookup
  of records the trigger already named.
- No change to what planner output may do. INV-043 still holds: proposals
  are validated trusted-side before any durable mutation.
