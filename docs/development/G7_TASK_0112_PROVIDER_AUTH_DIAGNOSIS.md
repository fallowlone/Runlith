# TASK-0112 — Production Sandbox Provider Authentication Diagnosis

Status: diagnostic complete; no production sandbox change adopted.

## Question

Why does an authenticated Claude CLI work on the host but fail when the same
CLI is launched through Runlith's production macOS Seatbelt boundary?

This task is diagnostic only. It does not authorize provider login, credential
extraction, or widening the production sandbox.

## Reproduction

Measured on macOS with Claude Code `2.1.277` using a harmless fixed-marker
provider call. No credential contents were read or recorded.

| boundary | result |
| --- | --- |
| host | authenticated; provider marker succeeds |
| sanitized `PATH + HOME + USER` | authenticated; provider marker succeeds |
| permissive Seatbelt | provider marker succeeds |
| production-shaped Seatbelt | exit 1; marker absent; no useful provider diagnostic |

The repository's production G6 smoke reproduces the same confined failure when
run with its exact module-qualified test name. Host authentication therefore
does not imply that the confined worker can use the provider-owned session.

## Capability isolation

Differential Seatbelt probes kept the production-shaped filesystem/network
profile and changed one support capability at a time. The earlier hypothesis
that randomized Bun helper executables were the missing primitive was falsified:
allowing `/private/tmp`, `~/.bun`, or both did not restore the marker.

The current CLI succeeds only when all three of these capabilities are present
together:

```text
mach-lookup com.apple.SecurityServer
file-read-data ~/Library/Keychains/login.keychain-db
process-exec /usr/bin/security
```

Ablation on the same host and CLI:

| additional capability set | result |
| --- | --- |
| none | fail |
| SecurityServer only | fail |
| login Keychain read only | fail |
| `/usr/bin/security` exec only | fail |
| SecurityServer + login Keychain read | fail |
| SecurityServer + `/usr/bin/security` exec | fail |
| login Keychain read + `/usr/bin/security` exec | fail |
| all three | marker succeeds |

The executable requirement is specifically `process-exec`: granting only
read/map access to `/usr/bin/security` still fails. The filesystem requirement
was narrowed from all of `HOME`, to `~/Library`, to `~/Library/Keychains`, and
finally to the current login Keychain database. No Keychain contents were
opened by the diagnostic harness.

## Root cause

Claude Code `2.1.277` uses a macOS Keychain access path that, under Seatbelt,
requires the SecurityServer Mach service, read access to the login Keychain
database, and execution of `/usr/bin/security`. Runlith's production profile
intentionally grants none of that ambient credential authority, so the host
session is authenticated while the confined child cannot consume it.

TASK-0101 already established that `HOME` and `USER` propagation are required;
TASK-0112 shows that they are no longer the remaining blocker. TASK-0102's
SecurityServer probe identified one necessary primitive but did not isolate the
complete current capability set.

## Architecture decision for remediation

Do not add this capability set to the production worker sandbox.

The three primitives together expose the user's general login Keychain access
mechanism to the provider process. Seatbelt can scope the executable path and
Keychain file path, but it cannot bind `/usr/bin/security` argv to one Claude
credential item. That would broaden the worker's credential authority to make
one smoke pass, conflicting with the G6 least-authority boundary and the rule
that Runlith does not extract or broker a provider CLI's subscription
credential.

The G6-preserving successor is therefore an explicit pre-launch refusal for a
provider configuration whose authenticated host session cannot be used inside
the required production isolation envelope. Authentication remains a host-side
precondition; Runlith does not perform login and does not grant the confined
worker ambient Keychain authority.

## Successor acceptance envelope

The remediation task must preserve these properties:

- unauthenticated provider state refuses before provider work;
- authenticated-but-unusable confined auth refuses before provider work with a
  stable typed provider-authentication/unsupported condition;
- Runlith never invokes an interactive login flow;
- the production Seatbelt profile gains no Keychain, SecurityServer, or
  `/usr/bin/security` capability for the provider worker;
- no credential value enters durable state, logs, audit, model context, or task
  results;
- existing G6 isolation/adversarial tests remain green;
- a successful production provider smoke is required only when the selected
  provider authentication mechanism is actually compatible with the required
  confinement; an incompatible mechanism must fail closed rather than widen
  isolation.

## Evidence provenance

Primary repository boundaries inspected for the diagnosis:

- `adapters/provider-claude/src/authentication.rs`
- `adapters/provider-claude/src/environment.rs`
- `adapters/runner-macos-seatbelt/src/lib.rs`
- `bins/agentd/src/execution.rs`
- `docs/architecture/providers.md`
- `docs/architecture/runners.md`
- `docs/architecture/INVARIANTS.md`
- `external-research/studies/2026-09-19-provider-authentication-boundary.md`

Historical TASK-0102 commit `9a4a292` was inspected as evidence only and was
not adopted or cherry-picked.
