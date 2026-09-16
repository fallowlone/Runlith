# Product Versioning (TASK-0029)

> Canonical source, advancement rules, and independence guarantees for
> the product version. `product/version.toml` is authoritative; this
> document explains the policy.

## Canonical source

`product/version.toml` is the single source of product-version
identity:

```toml
semver = "0.1.0"
development = "0.1.0-dev.task0029"
task = "TASK-0029"
gate = "G2"
```

No release date, Git SHA, dirty flag, build machine, platform, or
provider versions live here — those are build/runtime metadata and may
be derived separately later. No second editable version string exists:
both binaries report this identity as their IPC `build_version`
through the zero-dependency `product-version` crate.

## Development version rule

Every successfully completed task advances the development identity
once:

```text
TASK-NNNN → development = "<semver>-dev.taskNNNN"
```

`task` becomes `TASK-NNNN` and `gate` becomes the current gate after
the task. Example: `TASK-0042` in gate `G4` (without a release bump)
produces `development = "0.1.0-dev.task0042"`, `task = "TASK-0042"`,
`gate = "G4"`.

## Public SemVer rule

`semver` is the public release line (`0.1.0` today). It does NOT move
for ordinary implementation tasks — no automatic `0.1.0 → 0.1.1`.
Public SemVer changes only through an explicit task/release decision.

## Task completion rule

A task version identifies a repository state, not a marketing release:
it means only "this repository state corresponds to this completed
task". It never implies gate complete, release ready, or production
stable. A correctly completed audit whose verdict is `Gx_INCOMPLETE`
may still advance the development version when it legitimately changed
repository audit/planning artifacts.

## Gate relationship

The `gate` field tracks `planning/roadmap.yaml` / queue progression
and moves together with the development version bump. The version
crate's test pins `gate` against the queue's `current_gate` so the two
can never drift independently.

## Protocol-version independence

```text
Product version != IPC protocol version.
```

Today (see `product/version.toml` for the current development task):
supported IPC `0, 1, 2, 3, 4, 5` (`2` adds the G5 `execution_core` envelope;
`3` adds the G6 security control surface; `4` adds bounded Human Decision
observation/resolution; `5` adds trusted G7 planning trigger and Human Decision
continuation application). Historical versions keep their original authority:
planning operations require a negotiated v5 session. See
`docs/architecture/ipc.md` and the corresponding accepted ADRs. A product-version bump
MUST NOT automatically change protocol version; protocol versions
change only for wire-contract compatibility reasons.

## Cargo-package-version independence

Cargo crate versions (`0.1.0` across the workspace) and the product
development identity are separate concerns. Do not mechanically bump
every package version per task; that only creates `Cargo.lock` churn.
`build_version` in handshake messages is the derived runtime
representation of the canonical product version — diagnostic metadata,
never compatibility.

## Derived runtime/build metadata rule

`build_version` is derived from `product/version.toml` at compile time
plus optional future build provenance. The canonical identity always
remains the version file; nothing else is edited to change the
product version.

## Mandatory rule for future task prompts

```text
# Product Version

On successful completion of this task:

1. Read product/version.toml.
2. Preserve public semver unless this task explicitly authorizes a release bump.
3. Set development to <semver>-dev.taskNNNN.
4. Set task to TASK-NNNN.
5. Set gate to the current gate after this task.
6. Validate the version file.
7. Include product/version.toml in changedFiles.
```
