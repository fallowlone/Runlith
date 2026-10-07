# TASK-0165 — Auto-invocation product decision

Owner decision, 2026-10-07 (step 1 of TASK-0165). Mode **B — advisory
auto-run**:

- Trigger: every `SubmitResult`, all tasks. (Narrowing the task scope
  later is a follow-up, not this task.)
- Levels: V1/V2/V3 auto-execute; V0 skips by construction (the
  TASK-0162 runner refuses V0 plans — there is nothing to execute).
- Results are evidence only: they feed the TASK-0125 repair loop and
  never block `VERIFYING → COMPLETED` admission.
- Timeout, quota, sandbox-unavailable, or any infrastructure failure is
  recorded as advisory failure evidence; completion proceeds per the
  unchanged 0125 admission.
- Blocking auto-verify is explicitly deferred until advisory
  reliability is proven in practice.

Wiring (step 2) must implement exactly this: no new selection logic,
no TASK-0125 contract change, runner semantics unchanged.
