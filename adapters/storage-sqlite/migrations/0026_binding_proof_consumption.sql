-- 0026: ADR-0036 binding proof consumption (TASK-0160 IMPL-C).
--
-- Operator bindings consume their presence proof nonce exactly once
-- per (task, criterion): the durable AlreadyBound. The proof must
-- bind this exact operation, and only `candidate-binding:` operations
-- are consumable here — a B1 review proof (or any other protocol's
-- proof) can never authorize a binding (cross-protocol replay).
-- History is append-only.

CREATE TABLE binding_proof_consumption (
    nonce TEXT PRIMARY KEY REFERENCES owner_presence_proofs(nonce),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    criterion_id TEXT NOT NULL CHECK (length(trim(criterion_id)) > 0),
    operation TEXT NOT NULL CHECK (operation GLOB 'candidate-binding:*'),
    bound_at_ms INTEGER NOT NULL CHECK (bound_at_ms >= 0),
    UNIQUE (task_id, criterion_id)
) STRICT;

CREATE TRIGGER binding_proof_consumption_no_update
BEFORE UPDATE ON binding_proof_consumption
BEGIN
    SELECT RAISE(ABORT, 'binding proof consumption is immutable');
END;

CREATE TRIGGER binding_proof_consumption_no_delete
BEFORE DELETE ON binding_proof_consumption
BEGIN
    SELECT RAISE(ABORT, 'binding proof consumption is immutable');
END;

-- The consumed proof must bind this exact operation.
CREATE TRIGGER binding_proof_consumption_proof_operation
BEFORE INSERT ON binding_proof_consumption
WHEN (SELECT operation FROM owner_presence_proofs WHERE nonce = NEW.nonce)
    IS NOT NEW.operation
BEGIN
    SELECT RAISE(ABORT, 'binding proof binds a different operation');
END;
