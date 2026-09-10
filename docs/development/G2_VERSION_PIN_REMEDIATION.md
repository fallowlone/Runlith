# G2 Version Pin Remediation

> TASK-0031 — test-only fix for the two blockers reported by the TASK-0030
> closure audit (`docs/development/G2_CLOSURE_AUDIT.md`, left untouched as
> historical evidence). No product behavior changed.

## Original Failure

After the task-mandated version bump (`task0029` → `task0030`), exactly
two tests failed, both asserting hardcoded historical literals:

```text
crates/product-version/src/lib.rs:184
  canonical_file_parses_to_task0029 — expected "0.1.0-dev.task0029"/"TASK-0029"
crates/ipc-control/src/negotiation.rs:297-298
  build_versions_report_canonical_development_identity — expected
  "0.1.0-dev.task0029" twice
```

Final TASK-0030 state: 241 passed, 2 failed. Every G2 substance check
passed; only these pins blocked closure.

## Root Cause

TASK-0029’s versioning mechanism was sound (build versions derive from
`product/version.toml` via the zero-dependency `product-version`
crate), but its tests encoded the *current task number* as the
definition of correctness. Any mandated version advancement therefore
broke the suite. The defect was test scaffolding, not product logic.

## Fix

Test-only edits in the two explicitly authorized `#[cfg(test)]`
modules; production functions untouched (verified by diff: only test
fns changed, no new dependencies):

- `crates/product-version/src/lib.rs`:
  `canonical_file_parses_to_task0029` → renamed to the timeless
  `canonical_file_is_self_consistent`. It parses the real canonical
  file and asserts the *relationships*: `development ==
  <semver>-dev.task<NNNN>` with a 4-digit numeric suffix,
  `task == TASK-<NNNN>` with the same suffix, and
  `development_version() == version.development`.
- `crates/ipc-control/src/negotiation.rs`:
  `build_versions_report_canonical_development_identity` now compares
  `default_daemon_build_version()` and
  `default_client_build_version()` against
  `product_version::development_version()` — no literals, no duplicated
  file parsing in `ipc-control`.

`product/version.toml` advanced to `0.1.0-dev.task0031` / `TASK-0031` /
gate `G2`; public SemVer stays `0.1.0`.

## Dynamic Version Assertions

Current-identity checks that survive future bumps without edits:

```text
development == semver + "-dev.task" + 4-digit suffix   (product-version)
task == "TASK-" + same suffix                         (product-version)
gate == planning queue current_gate                   (existing
                                                        gate_matches_planning_queue,
                                                        untouched)
daemon/client build_version == development_version()   (ipc-control)
```

## Regression Search

Searched `task0029`, `TASK-0029`, `0.1.0-dev.task0029`, `task0030`,
`TASK-0030`, `0.1.0-dev.task0030` across the repo. Remaining hits
classified:

- Deliberate parser-rejection fixtures (`malformed_inputs_fail_closed`)
  — legitimate, keep.
- Historical records: queue entries, `VERSIONING.md` examples,
  `G2_CLOSURE_AUDIT.md` evidence, doc-comment `e.g.`s, provenance
  comments — legitimate, untouched.
- No remaining test describes the current canonical identity with a
  task-specific literal.

## Validation

```text
cargo fmt --all -- --check                                   PASS
cargo check --workspace                                      PASS
cargo test -p product-version                                4/4 PASS
cargo test -p ipc-control                                    35/35 PASS
cargo test --workspace --no-fail-fast                        all PASS (see below)
cargo clippy --workspace --all-targets --all-features \
  -- -D warnings                                             PASS
```

Rollover simulation: `product/version.toml` temporarily posed as
`task0032`/`TASK-0032` (test files untouched) → product-version 4/4
and ipc-control 35/35 green; file restored to `task0031` and
re-verified green. A `task0031 → task0032` bump requires zero edits in
the repaired modules.

## Remaining G2 State

```text
current_gate: G2,  G2: in_progress,  G3: planned
completed_through: TASK-0031 (queue updated with TASK-0031 + TASK-0032)
```

No G2 product code changed, so no re-verification of Task Core
behavior was needed or performed. Follow-up: **TASK-0032 G2 Closure
Re-Audit** (do not transition to G3 before it).
