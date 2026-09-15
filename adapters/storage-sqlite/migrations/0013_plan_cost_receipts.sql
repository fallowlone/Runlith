-- TASK-0078: durable planning-call cost evidence referenced by PlanRevision.
--
-- This is deliberately plan-scoped provenance, not the future general Usage
-- Ledger. It stores no prompts, transcripts, credentials, or provider output.

CREATE TABLE plan_cost_receipts (
    receipt_ref    TEXT PRIMARY KEY CHECK (length(trim(receipt_ref)) > 0),
    input_tokens   INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens  INTEGER NOT NULL CHECK (output_tokens >= 0),
    wall_time_ms   INTEGER NOT NULL CHECK (wall_time_ms >= 0),
    price_class    TEXT NOT NULL CHECK (length(trim(price_class)) > 0)
);
