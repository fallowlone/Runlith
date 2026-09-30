# Security Review Log — Runlith (adversarial review)

- **Date:** 2026-09-14
- **Target:** Runlith workspace, HEAD `297f1bf` (branch `main`)
- **Method:** read-only security review in dedicated worktree
  `~/dev/runlith-sec-review` (detached HEAD). No files
  modified, nothing committed, nothing merged. Four parallel deep audits
  (IPC/transport, process execution/sandbox, policy/data/secrets,
  git/provider) + manual line-level verification of every HIGH/MEDIUM
  finding by the reviewing agent.
- **Evidence class:** static proof to exact file:line + existing
  adversarial test probes (9th-client, symlink socket, Seatbelt escape
  tests). No standalone dynamic exploitation harness was run.

## Verdict

No Critical findings and no proven cross-user privilege escalation.
The core (IPC auth, framing, socket admission, policy engine, git
adapter, secrets hygiene) is well hardened. Two real HIGH findings
exist in the isolation boundary (sandbox can be silently skipped;
children inherit the full parent environment), plus MEDIUM items
around data-at-rest permissions, audit-chain keying, single-factor
IPC authority over approval decisions, and one proven idle-disconnect
transport defect.

## HIGH

### H-1 · Sandbox can be silently skipped (fail-open by convention)

- **Evidence:** `crates/runner/src/lib.rs:345-357`
  (`prepare_process_launch` returns `Box<dyn PreparedLaunch>` as a
  plain trait object — the caller must remember to call
  `.apply(&mut command)`); `adapters/runner-macos-seatbelt/src/lib.rs:29-42`
  (`SeatbeltLaunch::apply` rewrites the `Command` into
  `/usr/bin/sandbox-exec -p <profile>` only when invoked);
  `adapters/runner-native/src/lib.rs:14-20` (`NativeLaunch::apply` is
  an empty function). Nothing in the workspace verifies the handoff
  was applied before spawn.
- **Scenario:** a provider adapter (current or future) obtains the
  launch handoff for audit evidence, then spawns the original
  `Command` directly. The child runs **unsandboxed on the host** while
  `EnforcementReport` claims filesystem+network+process axes enforced.
- **Remediation:** make the handoff the only spawn path (spawn method
  on the backend, or type-state `SandboxedCommand` that cannot be
  spawned without `PreparedLaunch`); optionally assert at first spawn
  that the child is actually sandboxed (probe child).

### H-2 · Spawned children inherit the full parent environment (secret exfiltration)

- **Evidence:** `adapters/runner-macos-seatbelt/src/lib.rs:30-41` — no
  `env_clear()`/`env()` anywhere in the runner adapters or
  `crates/runner` (grep: zero matches). Env is delivered via `execve`,
  so the Seatbelt profile (files/network/exec) cannot hide it.
  `NativeLaunch::apply` is a no-op. Contrast: `adapters/git-cli` and
  `adapters/provider-claude` already do `env_clear()` + allowlist
  correctly (`provider-claude/src/environment.rs:110-130`).
- **Scenario:** a malicious/compromised provider child runs `env` and
  exfiltrates API keys / credentials through the allowed `tcp:443`
  channel (see M-5). Inherited `PATH` also feeds the PATH-hijack items.
- **Remediation:** in `SeatbeltLaunch::apply` and a hardened native
  launch: `env_clear()` + explicit minimal allowlist (`PATH`, `HOME`,
  `TMPDIR`, locale). Env policy should live in the plan, not in
  inheritance.

## MEDIUM

### M-1 · Single-factor UID authority includes approving safety gates

- **Evidence:** `crates/ipc-transport/src/platform/peer.rs:263-267`
  (`same_local_principal` = UID equality only);
  `bins/agentd/src/lib.rs:289-299` (`authorize_peer`); dispatch of
  `PolicyCoreRequest::DecideApprovalRequest` and `ShutdownRequest`
  over the same authorized session.
- **Scenario:** any same-UID process (malicious postinstall, escaped
  helper) can connect to the socket and **approve its own escalation**
  — the approval gate is the system's safety boundary — and can shut
  the daemon down. Documented accepted model (local ≠ trusted,
  ADR-0020/TB2), but the blast radius of the approval surface exceeds
  "read my tasks".
- **Remediation:** for `DecideApprovalRequest` (and optionally
  `Shutdown`), require an out-of-band capability secret (daemon-made
  token in the runtime dir, 0600) — needs a design decision (ADR
  candidate; see REMEDIATION_PROMPT.md).

### M-2 · SQLite DB created with default 0644 permissions

- **Evidence:** `adapters/storage-sqlite/src/task_store.rs:38-58`
  (`Connection::open` + pragmas; no chmod anywhere — grep
  `set_permissions|umask|mode(0o` over storage/secrets/keychain: zero
  matches). DB contains the security-audit chain, approval decisions,
  secret binding targets, full task/runtime history.
- **Scenario:** on multi-user hosts the DB is group/world-readable;
  pre-open symlink/hardlink race at the DB path (no O_EXCL/O_NOFOLLOW,
  no parent-dir permission check).
- **Remediation:** create parent dir 0700, set file 0600 immediately
  after open; refuse to open an existing group/world-readable DB
  (fail closed).

### M-3 · Audit-log tamper resistance is detection-only and unkeyed

- **Evidence:** `adapters/storage-sqlite/src/audit_store.rs:307-310`
  (`hash_record` = unkeyed SHA-256 chain), head row in the same file
  (`audit_store.rs:96-108`). Read-side verification is genuinely
  fail-closed (`audit_store.rs:73-86, 105-109`) — the chain detects
  accidental corruption, not an adversary.
- **Scenario:** anyone who can write the DB (see M-2) recomputes the
  whole chain — rewrites history, deletes approval records, forges
  `PrivilegeElevation` facts — leaving a fully self-consistent chain.
- **Remediation:** HMAC with a key stored outside the DB (OS keychain
  via the existing `SecretBackend` port); longer term anchor the head
  hash externally.

### M-4 · macOS runtime namespace can collapse into shared UID-less /tmp dir

- **Evidence:** `crates/ipc-transport/src/platform/unix.rs:421-428`
  (`macos_runtime_parent` uses `TMPDIR` or falls back to
  `std::env::temp_dir()`, appends `APP_DIR_NAME = "ai-engineering-os"`
  without a UID suffix; the Linux fallback at `unix.rs:410-418` uses
  `ai-engineering-os-{uid}`).
- **Scenario:** daemon started with empty `TMPDIR` (launchd/cron/
  scrubbed env) → endpoint `/tmp/ai-engineering-os/agentd.sock` shared
  by all users; any local user pre-creates it 0700 first → cross-user
  DoS of daemon startup (`validate_runtime_dir`, `unix.rs:137-160`).
- **Remediation:** append `-{uid}` on macOS like the Linux fallback.

### M-5 · Network scope `tcp:443` allows any destination, incl. localhost SSRF

- **Evidence:** `crates/runner/src/lib.rs:15-18`
  (`NETWORK_SCOPE_OUTBOUND_TCP_443` = "any remote TCP destination on
  port 443"); `adapters/runner-macos-seatbelt/src/lib.rs:167-169`
  (`(remote tcp "*:443")`).
- **Scenario:** sandboxed child reaches attacker-chosen C2 and local
  HTTPS admin panels / cloud metadata endpoints on 443.
- **Remediation:** rename the scope so coarseness is explicit
  (`tcp:443:any-host`); add host-literal scope variants where policy
  needs precision.

### M-6 · Proven idle-disconnect defect (breaks ping-liveness model)

- **Evidence:** `crates/ipc-transport/src/framing.rs:117-130`
  — `read_frame` wraps the read in `time::timeout(FRAME_IO_TIMEOUT)`;
  `crates/ipc-transport/src/lib.rs:38` — `FRAME_IO_TIMEOUT = 5 s`. A
  connection that waits longer than 5 s for its (single in-flight)
  request gets `TransportError::timeout()` → session torn down with no
  distinguishable client-side error (surfaced as
  `MalformedFrame("connection closed before a frame completed")`).
- **Scenario:** any slow/legitimate client holding an open session (UI,
  long human decision) is silently disconnected; a keepalive ping
  cadence must be < 5 s to survive.
- **Remediation:** separate an idle-keepalive deadline (server side)
  from the per-frame I/O deadline, or fix the liveness model
  (ping cadence < 5 s documented, or configurable deadline per session
  class).

### M-7 · `RequireApproval` on a non-`can_grant` layer degrades toward Allow

- **Evidence:** `crates/policy/src/lib.rs:510-534` (`apply_approval`);
  engine compiles `RequireApproval` rules on layers without grant
  rights such that the effective result can be `None` (accidental
  Allow semantics).
- **Scenario:** user-managed config places RequireApproval on a layer
  that cannot grant → accidental privilege creep without a hard error.
- **Remediation:** reject at `compile()` any `RequireApproval` rule on
  a non-`can_grant` layer (fail closed at configuration time).

## LOW / INFO

- **L-1** Git output bound enforced after full in-memory capture —
  `adapters/git-cli/src/runner.rs:637-651`; `Command::output()` reads
  all stdout+stderr before the `MAX_GIT_OUTPUT_BYTES` check → hostile
  repo (huge `git status --porcelain`) can OOM the orchestrator. Copy
  the drain-thread `take(bound+1)` pattern from
  `provider-claude/src/process.rs:287-299`; bound stderr too.
- **L-2** `git` resolved via ambient PATH —
  `adapters/git-cli/src/lib.rs:74-79` + `runner.rs:623` re-injects the
  host PATH. PATH-hijack with a same-UID foothold. Resolve once at the
  composition root with canonicalize (pattern already exists for
  `claude`: `bins/agentd/src/execution.rs:642-644`).
- **L-3** `--resume` accepts leading-dash text —
  `adapters/provider-claude/src/config.rs:150-158`; latent option
  injection if a provider-controlled session ref ever flows there.
- **L-4** Seatbelt profile passed as argv — visible in `ps`
  (`adapters/runner-macos-seatbelt/src/lib.rs:34`); consider
  `-f <0600 file>`.
- **L-5** `canonical_support_root` silently degrades on canonicalize
  failure (`adapters/runner-macos-seatbelt/src/lib.rs:196-200`) →
  symlink race for not-yet-existing paths; fail closed instead.
- **L-6** `checked_id` accepts shell/path-adjacent characters —
  `crates/policy/src/lib.rs:641-646`; ids flow into keychain account
  names (`adapters/keychain-native/src/lib.rs:93`) where a crafted
  `SecretRef` can alias another credential within service `runlith`.
  Restrict to `[A-Za-z0-9._:-]`.
- **L-7** `agentctl` PATH-fallback for daemon spawn —
  `bins/agentctl/src/lib.rs:937-950`
  (`daemon_executable_next_to` falls back to bare `agentd` through
  PATH); a same-UID attacker can plant a fake daemon and harvest user
  input. Fail with an error instead of PATH fallback.
- **L-8** In-memory `apply_approval` is replayable (no consumption
  marker on the contract object); the durable replay barrier is
  correct (`approval_store.rs:265-283`) — keep memory-level single-use
  in sync when the store evolves.

## What is done right (verified)

1. IPC client auth: kernel peer credentials (SO_PEERCRED/getpeereid,
   `peer.rs:60-116`), UID mismatch closes the connection before the
   first frame, unsupported platforms fail closed, no auth↔dispatch
   TOCTOU (credentials fixed at connect time), daemon principal never
   from argv/env/config.
2. Socket admission: runtime dir non-symlink/0700/owner-checked
   (`unix.rs:114-160`), socket node forced 0600 post-bind, existing
   nodes never unlinked/chmodded/replaced, stale recovery uses
   (device, inode, uid, is_socket) snapshot→probe→re-verify→remove
   (`unix.rs:264-306`), symlinks rejected without following.
3. Framing: 1 MiB cap pre-allocation, empty frames rejected both ways,
   no resync, 5 s deadlines, 8-slot semaphore → slowloris
   self-healing (`framing.rs`, `lib.rs:29-38`).
4. No TCP fallback; the Windows surface mirrors the discipline
   (per-SID pipe, `reject_remote_clients(true)`, owner-only DACL,
   impersonation with revert).
5. Policy engine: default deny (`engine.rs:5-6, 337-344`), Allow only
   with `can_grant`, one Deny poisons the whole multi-capability
   request (`most_restrictive`), a child cannot widen parent authority
   (type-checked `narrow`, `lib.rs:203-210, 616-632`), hard DENY not
   approvable (`lib.rs:515-517`), approval fingerprints cover
   request+policy version+expiry+effective set (`engine.rs:474-496`).
6. SQL: no injection anywhere — all 12 storage files use static SQL
   with positional parameters; scope paths reject `..`/absolute/Win
   prefixes (`lib.rs:648-661`); corrupt DB/migrations fail closed
   (`migration.rs:41-68, 139-147`).
7. Secrets: keychain-only (no file fallback), zeroized after delivery,
   redacted Debug, binding/authority checked before touching the
   backend (`secrets/src/lib.rs:272-291`).
8. git adapter: single `run_git` choke point with `env_clear` + fixed
   hardening flags; hooks/fsmonitor/sshCommand/credential.helper/
   global/system config neutralized (`runner.rs:600-636`); managed
   branch dash-refusal and safe alphabet; `.git` metadata identity
   re-proved against the trusted anchor on every operation
   (`runner.rs:144-154, 566-573`); cleanup non-forced only with
   ownership re-proof; no clone/fetch/push surface; provider output
   never reaches command construction.
9. Honest error hygiene: typed errors never on the wire, client-facing
   messages length-capped and sanitized (`agentd/dispatch.rs:9-31`).

## Priority

1. H-2 (env_clear + allowlist in runner) — cheapest, biggest effect.
2. H-1 (mandatory PreparedLaunch spawn path, type-state).
3. M-2 + M-3 (0600 DB, HMAC-keyed audit chain).
4. M-1 (second factor for approval/shutdown; needs ADR decision).
5. M-4, M-5, M-6, M-7, then LOW items.
