# Audit remediation, 2026-11-03

Scope: continue fixes from `audit-reports/` and Claude session
`97faf86b-536a-4512-be47-c5e896ff5505`, at the owner's request. Baseline:
`75b176dba3b450c2c573ac8c7763c3e654ba896e`. This is an implementation ledger,
not a release approval or an independent closure audit.

The owner additionally requested the security findings from `main e468af3`.
Their item-by-item coverage is in
[SECURITY_REVIEW_e468af3_REMEDIATION.md](./SECURITY_REVIEW_e468af3_REMEDIATION.md).

## Recovered work

Selected fixes from `integration/hardening` (`41b4dc7`) were copied into this
working tree: dependency patches, test-only journal resealing, migration
backup/validation and startup diagnostics. Uncommitted IPC authentication
and routine changes were recovered from their respective Claude worktrees.
All original branches and worktrees are preserved. The Claude session ended
on a quota error; its verification claims do not verify this combined tree.

The containment harness, snapshots and draft tool broker remain on branches
`feat/task-0232-escape-harness`, `feat/task-0233-snapshot-rollback` and
`feat/task-0234-0235-broker-phase1`. ADR-0049 is Proposed; credential, egress,
resource-limit and audit-authority acceptance criteria remain open.

## Acceptance and evidence

Each confirmed defect needs a production-path fix, a meaningful regression
or deterministic configuration check, and affected callers updated. Preserve
the 1 MiB frame cap, no-replay mutation semantics, private endpoint admission,
policy checks and mandatory runner isolation.

| Finding | Implementation | Verification |
| --- | --- | --- |
| F01 Unix server authentication | Endpoint-node and kernel peer-UID checks before returning a stream | UID, owner and directory-mode checks passed; three socket fixtures blocked by host permissions |
| F02 CLI key input | Private files or one piped stdin key; raw flags rejected; input buffers zeroized | Unit test and fresh shipped-binary file/stdin signing regression passed |
| F03 / F13 dependencies | ctutils 0.4.4, Tokio 1.51.6 recovered from Claude | Compiled; both cached cargo-audit runs report zero vulnerability entries |
| F04 connection task retention | Live JoinSet reaping and retained-task bound assertion | Socket load/shutdown scenarios remain blocked by host permissions |
| F05 process/capture bounds | Owned process groups; bounded capture after exit and timeout | 4 verifier tests passed, including descendant fixtures |
| F06 / F11 list pagination/projection | Byte/count-bounded pages and a direct SQL projection; explicit legacy error | Two 600 kB tasks retrieved in bounded frames; count/cursor checks passed |
| F07 routine atomicity | Run, schedule and counter in one immediate transaction with stale-state guards | Fault-injection rollback and budget replay tests passed |
| F08 widget shutdown | Retained worker, cancel/join and draining admission | 10 widget tests passed, including shutdown |
| F09 desktop ID bounds | 256-byte IDs admitted | Native ID test passed |
| F10 blocking daemon dispatch | Blocking pool; drain in-flight dispatch before endpoint release | Real SQLite writer-lock + independent duplex ping regression passed |
| F12 metrics lock scope | Separate outbound serialization; state released for HTTP and persistence; bounded cache | Native Clippy passed; HTTP fixtures blocked by local socket restrictions |
| F14 native desktop CI | Host fmt/clippy/test steps, pinned actions, dependency/secret workflows | YAML parsed with duplicate-key checks; all 11 action references pinned; actual CI run pending |
| F15 memory supersession | One store transaction for domain transition and link | New regression reproduced partial state before fix, then passed including retry |
| F16 truthful routine status | Unsupported attempts are BLOCKED without charging budget | Background service/failure/restart tests passed |
| Additional confirmed Claude/Muse findings | Controlled auth-probe env, prompt terminator, no-follow file reads, vault/lock/log admission, codec allocation guards, stricter redaction and metric redirects | Auth sentinel, descriptor ingest, vault/lock/log admission, codec and redaction regressions passed; native redirect predicate passed |

No production version, accepted ADR, signing identity or owner-only GitHub
setting is inferred from this request. External audit findings have no
canonical task descriptor; the owner's request and these acceptance criteria
bound this work without inventing task completion or release authority.

## Compatibility decisions

Pagination uses the existing negotiated capability mechanism with the identifier
`task-list-pages-v1`; the public protocol number remains 13. Page requests and
continuations require that capability. Fields omitted by a legacy client retain
their old meaning. A legacy list too large for one frame receives a bounded,
typed error. Clients check cursor progress and collect every page. This does not
provide snapshot isolation across pages; concurrent inserts before the cursor
require a fresh list.

Fresh execution-cycle lock directories/files use 0700/0600. Existing owned
non-writable-by-others locks remain admissible, preserving recovery of existing
executions. Symlinks, non-regular files, foreign ownership and hardlinks refuse.

Only the existing supported macOS/Unix read path has descriptor-based file
admission. Unsupported platforms fail closed for these file reads. This is not
a claim of Windows product support or an independent escape-suite verdict.

## Combined-tree verification, 2026-11-04

Root and standalone native formatting checks passed. Root Clippy with all
targets/features and native Clippy passed with `-D warnings`; existing ts-rs
attribute diagnostics remain visible. Both command-line binaries were rebuilt.

The full workspace run measured **1881 passed, 126 failed, 5 ignored** across
18 failing targets (RTK recall `9e02508104facb91`). This is not a green run.
Compared with the audit baseline, it added five socket-authentication fixture
failures, two migration regressions, and one live-ownership failure. The two
migration issues were corrected without relaxing schema validation: the
corruption fixture now replays migration 0001, and refusal guidance preserves
the recreate instruction. The complete affected suites then passed **61/61**.
Two transport negative tests now validate their directory preconditions without
requiring a socket; the final auth module measured **3 passed / 3 socket-bind
failures**. These follow-ups do not retroactively change the full-run totals.

Fresh-binary key-input and early-startup-error tests passed **2/2**. The final
CLI library run measured **18 passed / 5 failed** in socket/startup fixtures.
The native suite measured **29 passed / 10 failed**; all ten fail at TCP bind
with EPERM, including the concurrent cached-read HTTP regression. Workspace
runner and positive execution paths also remain unverified. The additional
`two_shared_resumes_elect_a_single_live_owner` failure occurs after its provider
attempt terminates; its cause is not independently cleared by this run.

Both cached `cargo audit --no-fetch` checks returned zero vulnerability entries.
Core still has three unmaintained warnings; native still has one unmaintained
warning and the glib unsoundness advisory RUSTSEC-2024-0429. The cached database
contained 1403 advisories but no freshness metadata. Cargo-deny and actionlint
are unavailable locally, and the CI workflow has not run; its stricter advisory
policy may report these existing dependencies. Four YAML configurations were
parsed with duplicate-key checks, TOML parsed, and all 11 action references
checked for full commit SHA pins.

Gitleaks 8.30.1 scanned **498 commits**. Its two initial matches were PEM
delimiters and a truncated DER test literal in `redacts_pem_private_key_lines`
at commit `b7ca9341f65cda644ce1e864752be1b1ad5128da`: 24 decoded bytes with a
declared 1215-byte content length. `.gitleaksignore` excludes only those two
commit/file/rule/line fingerprints. With full commit IDs and that file, the
history scan passed; the same fixture on stdin still produced two findings.
The tracked diff and owned new files also passed a redacted stdin scan.

The machine-readable record is
[`AUDIT_REMEDIATION_2026-11-03.task-result.json`](./AUDIT_REMEDIATION_2026-11-03.task-result.json).
Its `testsPassed` is false and `taskId` is null: this request has no canonical
Task binding, so it does not claim Task acceptance or update product identity.

## Open Claude follow-ups

The owner selected OS confirmation for execution and Shutdown on 2026-11-04.
[ADR-0050](../architecture/adr/ADR-0050-os-user-presence-for-local-control.md)
records the decision. Daemon-owned LocalAuthentication runs before protected
mutation; absent, denied, busy or timed-out confirmation fails closed. Routine
ticks record `user_presence_required` without charging budget. Nine tests
passed, including Resume/routine refusal and ping during pending confirmation;
client denial/no-replay and persisted blocked-budget tests passed. Actual OS
success smoke returned `Unavailable`, code 4099 (NSXPCConnectionInvalid in the
SDK), so interactive success remains unverified here.

Positive CLI process matrices now use a separately built daemon fixture with
explicit test confirmation. A private installation pairs it with the shipped
CLI, retaining actual launch/restart, IPC, G6 and Runner behavior. CI builds
this Cargo example before the suites. The fixture supplies no evidence of a
successful native OS dialog; production `agentd` always selects the system gate.

Vault setting policy rejects predictable passwords without locking out legacy
vaults; 43 backend tests passed. Failed-check count/deadline persist in private,
locked, atomically replaced state; 24 daemon vault tests passed. Portable SID
validation (2), first-frame staging (3) and fatal revert (1) tests passed;
Windows native verification remains pending. Two org hex-parser tests passed
and owned org key copies now zeroize.

Provider summaries/details redact before durable writes and before independent
evaluation. The end-to-end regression reproduced journal leakage and the
separate evaluator leakage before their fixes. Four service suites passed 45
tests initially, then 46 including credential-shaped evidence-ref refusal;
an actual ClaudeExecutor fixture also passed the store/reopen boundary.
This does not prove universal secret recognition or scrub existing history.

Direct `__candidate-snapshot-write` was reproduced against the built binary.
Helpers now require a private one-shot permit bound to argv/input, issued
inside the verified Runner invocation. Kernel socket identity and an audit-token
code-directory-hash check reject another executable posing as the parent.
Three exchange tests and three fresh-binary adversarial tests passed; only
helper profiles receive read-only signature-query authority. The new governed
positive test cannot pass here: even `/usr/bin/true` under `sandbox-exec` reports
`sandbox_apply: Operation not permitted`. There is no confinement downgrade.

Shared process cleanup now observes exits without reaping, signals the owned
group while the leader PID remains reserved, then reaps. This removes the
post-reap PID-reuse window in candidate, verifier and provider cleanup. The
kernel waitability/pipe-closure test, four verifier tests and all 93 provider
library tests passed. The 25 hostile-Git safety tests also passed. All 12
redaction tests passed, including preserved CRLF/trailing blank lines.

The installed Claude Code 2.1.317 accepted the governed help argv. In an isolated,
credential-free environment, `--version` without the separator printed the
version; after `--` it did not take that fast path and the local-only synthetic
credential probe was killed after five seconds. This establishes option-boundary
evidence, not a completed provider request.

The root executor constructs `ClaudeConfig::default()` without calling
`proposal_only()`. Consequently the draft queue's assumption that tools are
off is not established. Do not enable the draft broker or treat the credential,
egress and resource escape gates as closed on the strength of this patch.
Same-UID installation trust and keyed audit authority remain separate open
decisions; OS user presence follows the owner's newer decision above.

The risk gate is a deterministic validation over the proposal and explicit
context, including derived addition/update risk and grants. Ten repeated live
planner calls with captured identical policy inputs were not performed; the
reported TASK-0244 escalation inconsistency is not closed. Process hardening
is described below; unrestricted attach verification, READY idle limits and
Windows stderr capture remain follow-ups.
Desktop authoring, signing/distribution and owner-only repository settings in
Claude's future queue are separate undelivered tasks. No release closure or
independent containment verdict is claimed.

## Follow-up verification, 2026-11-05

The runtime-root confidence gap exposed a concrete collision: different full
worktree paths with the same leaf shared private HOME/config/XDG/temp. The
regression failed on the shared path (RTK recall `2632b98ae935cb6f`). A full-path
SHA-256 namespace now separates them while reusing the same recorded path on
restart. A second regression proved that a legacy leaf named like a hashed key
could otherwise contaminate the new root (`66ab0c2ec996441b`); the daemon now
selects a separate `provider-environments-v2` base. Old runtime directories are
retained without automatic import. Provider-native sessions in them are not
migrated; canonical Task, Execution, lease and vault state is unchanged.
The migration regression passed; provider unit tests passed 94/94, four adapter
and contract targets 32/32, and the service-binding suite 11/11.

Standalone `org` compilation enabled only the AEAD alloc feature; key erasure
in chacha20poly1305 0.11.0 Drop is conditional on zeroize. The full daemon
already got that feature through vault, but the domain crate must request it
itself. Its manifest now explicitly enables zeroize. The standalone feature
graph confirms it, all 7 org tests passed, and no dependency version changed.
This is not proof of erasure of every allocator/compiler or IPC copy.

Worker IPC hex decoding reproduced a UTF-8 boundary panic (`da59badc161a0f5f`).
It now refuses non-ASCII input before slicing. The regression checks typed
refusal, no malformed enrollment writes, a subsequent valid enrollment and the
64-byte signature path. CLI org plaintext formatting independently reproduced
the same panic (`3efdbeb98a0ecebb`), and also silently discarded invalid hex
pairs or an odd tail. It now rejects malformed encoding without partial output.
Both regressions passed. Five worker/org service targets passed 22/22 and two
SQLite contract targets passed 2/2.

Capture's inode-only check also accepted an in-place rewrite. A deterministic
regression reproduced this with unchanged inode and length, after a successful
unchanged-file read (RTK recall `e8edb2ecbbfb0372`). Capture now compares length,
mtime and ctime in addition to device/inode, and revalidates each traversed
directory. All 10 adapter tests passed, including the shared contract. These
checks detect observed version changes; they do not provide atomic tree
snapshots or prevent writes after the final check. No timing stress PoC was run.

The cached root advisory scan remains at zero vulnerability entries and three
unmaintained warnings (1403 cached advisories; no freshness metadata). The
current primary [glib advisory](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)
still identifies the native lockfile's glib 0.18.5 as affected and lists 0.20.0
as the first patched line. No incompatible GTK dependency override or advisory
suppression was introduced.

After these changes, workspace formatting, all-target/all-feature Clippy and
fresh builds of agentd, agentctl and the separate confirmed-daemon fixture
passed. Existing ts-rs attribute diagnostics remain visible. The ownership
check accounts for 132 changed paths (97 tracked, 35 new), preserving 26
pre-existing untracked paths. The updated diff and owned new files passed
Gitleaks with zero findings. The full suite is still not green: the recorded
OS-confirmation, socket, Seatbelt and Windows limits remain unresolved.

## Process memory protection, 2026-11-05

The daemon and hidden helper previously retained an inherited nonzero
RLIMIT_CORE. The helper regression reproduced a 4096-byte soft/hard limit
after its entrypoint returned (RTK recall `2e50db661c118e01`; only the helper
failure is valid evidence in that run). After correcting the daemon fixture's
0700 directory precondition, disabling just its new guard independently
reproduced the same limit retention (`851c31cb6ad9a1dc`). The guard was restored.

The shared Unix guard sets both limits to zero. `main` calls it before runtime
creation and argument handling; library startup repeats it before endpoint
admission and service opening; the hidden helper calls it before permit
authentication or candidate input access. Unprivileged children inherit a hard
limit they cannot raise. The macOS guard also calls PT_DENY_ATTACH and refuses
startup on error. `RUNLITH_ALLOW_DEBUGGER=1` bypasses only that ptrace call in
builds with debug assertions; it never enables core dumps, and the default
release configuration compiles out the exception.

Already-zero inherited limits are not rewritten; child tests also exercise
this repeated guard path. The final workspace formatting/Clippy checks,
release-profile daemon check, fresh daemon/CLI/fixture builds, three binary
authorization tests and two secret-input/startup-log tests passed. The shipped
daemon's help path also returns successfully through the pre-runtime guard.

Three isolated-process tests passed, including daemon/helper entrypoints,
child inheritance, refusal to raise the limit and the debugger exception's
continued core-dump protection. Their single ignored entry is an explicitly
invoked subprocess fixture. All three existing authorization tests passed.
No secret-bearing crash fixture was created. The real PT_DENY_ATTACH syscall
returned zero here; the PT_TRACE_ME control returned EPERM, so attachment
resistance cannot be independently distinguished from host restrictions.
This does not establish hardened-runtime entitlements, signing, mlock,
Mach-memory isolation or Windows dump protection.

## Live RustSec delta check, 2026-11-05

The GitHub connector returned upstream HEAD
[`965bf9ad083b58e6340d65829d85217b18be9290`](https://github.com/rustsec/advisory-db/commit/965bf9ad083b58e6340d65829d85217b18be9290),
dated 2026-11-03 20:34:28 UTC. The local cache is at
`6c5fedf8ff03c485ed674d8f941e0180f3f72d70`. All **1403** local advisory files
(1383 crates, 20 Rust) match that commit's Git blob hashes, with no extra or
missing advisory files. The live
[comparison](https://github.com/rustsec/advisory-db/compare/6c5fedf8ff03c485ed674d8f941e0180f3f72d70...965bf9ad083b58e6340d65829d85217b18be9290)
contains three commits and three added advisories: arrow-data,
astral-tokio-tar and pin-utils. Each complete advisory was read at the pinned
upstream revision; none of these package names occurs in either lockfile.

This establishes that those upstream additions add no finding for these
lockfiles. It does not claim that cargo-audit fetched the newer database:
the cache remains unchanged, and cargo-audit's own freshness fields are null.
Both normal cached audits were rerun and returned zero vulnerability entries.
Strict `--deny warnings` runs both exit **1**: core reports unmaintained
zbus_names, zvariant and zvariant_derive (RUSTSEC-2026-0371/0370/0372);
native reports unmaintained proc-macro-error (RUSTSEC-2024-0370) and unsound
glib (RUSTSEC-2024-0429). No advisory ignore was added. Actual cargo-deny and
CI execution remain unverified; the strict-audit failures are not passes.

## Upstream pin provenance

The GitHub API was inaccessible from the local shell. Commit identities were
verified through the official release pages and their commit links:
[checkout v4.4.0](https://github.com/actions/checkout/commit/11d5960a326750d5838078e36cf38b85af677262),
[setup-node v4.4.0](https://github.com/actions/setup-node/commit/49933ea5288caeca8642d1e84afbd3f7d6820020),
[CodeQL v4.39.1](https://github.com/github/codeql-action/commit/9b3254085494b55a0f7c4c2b3d2d52cc712ce370).
The scanner container digest comes from the
[official gitleaks package](https://github.com/gitleaks/gitleaks/pkgs/container/gitleaks/).
