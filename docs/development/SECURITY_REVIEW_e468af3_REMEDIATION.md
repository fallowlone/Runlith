# Security review e468af3: remediation coverage

The owner's additional review concerns `main e468af3`. These changes are
applied to `fix/planned-child-objective-and-attempt-verdict`, based on
`75b176d`. This is implementation evidence, not an independent verdict on
main. The full combined-tree test run is not green.

## Findings

| Review item | Current change / status | Evidence and limits |
| --- | --- | --- |
| Auth probe inherits daemon environment | Shared cleared probe construction; explicit identity anchors for auth only | Provider auth sentinel passed; ambient cloud/API credentials do not inherit |
| Timeout leaves descendants | Own process groups; signal before reaping, retain leader PID with NOWAIT; bounded capture | Four verifier tests, kernel waitability/descendant cleanup test and 94 provider library tests passed. Candidate uses the same cleanup; no Windows process-tree claim |
| JWT and assignment redaction | Compact JWT-shaped credentials and key/token/secret/PAT/auth assignment values redact | Redaction tests passed; unmarked arbitrary secrets remain outside a heuristic recognizer |
| Provider output persistence/context | Redact attempt detail before storage, lifecycle reasons before journal append and submitted summary before evaluation; reject credential-shaped opaque evidence refs | Journal, evaluator summary and evidence-ref leakage reproduced before fixes; 46 affected service tests and actual ClaudeExecutor fixture-to-journal check passed. Plaintext line endings are preserved |
| Same-UID control authority | Owner chose OS confirmation: ADR-0050; daemon checks Shutdown, Run/Resume and routine authorize/run | Nine confirmation tests passed. Positive CLI matrices use a separate test-composed daemon. Actual OS success smoke returned Unavailable, code 4099; native success remains unverified |
| OAuth in child environment | Phase 1 exposure documented in providers.md; no guessed credential-file format written | ADR-0039 accepts the residual. A private file is still readable by provider tools; proxy remains separate work |
| Predictable vault passwords | Setting-time blocklist; legacy passwords remain unlockable | 43 backend tests passed, including create/change/recovery rejection and legacy compatibility |
| Vault backoff resets on restart | Private bounded state, stable OS lock, atomic replacement and fsync | 24 daemon vault tests passed, including reopen, success reset, cap and corrupt/link/mode refusal. No same-user tamper-resistance claim |
| Prompt option injection | Explicit `--` before the prompt | Argv regression and installed 2.1.317 help/parser boundary probes passed. Local-only synthetic-credential probe after `--` timed out at 5 seconds instead of taking the version fast path; no live inference claim |
| Provider outbound 443 | No implicit network-policy change | ADR-0039 explicitly retains tcp:443; dropping all network also prevents inference. Host allowlisting needs its separate gateway decision |
| Finder/effect-capture TOCTOU | Descriptor-relative no-follow traversal, consume held descriptors; capture also checks size/mtime/ctime and revalidates traversed directories | Adapter tests and deterministic replacement fixture passed; same-inode rewrite regression reproduced acceptance before the version check and now refuses. All 10 capture tests passed. These checks do not provide an atomic tree snapshot; Windows file-read support is not claimed |
| Hidden candidate helper entrypoints | One-shot 30-second nonce, argv/input binding and private inherited channel; macOS kernel peer/audit-token/same-image check before candidate file access | Direct binary write reproduced before fix. Three authorization tests and three fresh-binary adversarial tests passed; replay, expiry, nonce/argv/input substitution and another-image issuer refuse. Governed Seatbelt positive test remains blocked here |
| Cycle lock links | Owned/private admission; no-follow regular single-link file | Admission regressions passed; compatible owned legacy modes retained |
| Data/runtime directory creation window | Mode 0700 at creation | Directory admission tests passed |
| Git revision option injection | rev-parse --verify --end-of-options | Revision regression and 25 hostile-repository safety tests passed |
| Vault path | No-follow owned regular 0600 single-link file; bounded read | Vault path regressions passed |
| Codec prefix allocations | Removed prefix-based capacity allocation; validate against actual input | Malformed-prefix regressions passed |
| Metrics redirect/cache | Exact HTTPS GitHub API origin/port, redirect bound, bounded cache | Predicate/cache tests and native Clippy passed; HTTP fixtures cannot bind locally |
| Windows SID bounds | Aligned initialized storage; validate returned address/header/revision/count/end before copy | Two portable tests passed; Windows CI now runs ipc-transport tests. WinAPI compile/runtime pending |
| Supply chain | SHA-pinned actions, scheduled audit/deny/secret scans, Dependabot, desktop CI, toolchain pin | Configuration checks passed. Live RustSec comparison verifies the cached base and three upstream additions; none of the added packages occurs in either lockfile. Strict warning-denying audits fail on the existing three core/two native warnings; actual cargo-deny and CI execution remain pending |

## Confidence gaps

| Review gap | Current evidence / remaining work |
| --- | --- |
| Windows pipe staging/prefetch | Reproduced pipelined-frame loss; exact reads now preserve later bytes and bound silent/partial staging. Three portable tests passed. Kernel/Tokio impersonation behavior needs Windows execution |
| Failed impersonation revert | Found continuation after failed RevertToSelf; explicit revert and Drop now fail-stop. Abort child-process regression passed; WinAPI bridge pending CI |
| Provider output to SQLite/journal | Actual ClaudeExecutor fixture, store/reopen and evaluator-boundary regressions added; see above. Existing history is not rewritten |
| Per-operation authorization | Selected OS gate implemented; G6, approvals and Runner still follow. Genuine binary attestation, injection resistance and arbitrary same-user filesystem protection are not established |
| Process memory protection from Claude follow-ups | Unix daemon/helper guard zeroes soft/hard core limits; macOS denies debugger attach. Three real-process tests passed for early entrypoints, inheritance, hard-limit refusal and debugger exception preserving zero limits. PT_DENY_ATTACH returns zero; PT_TRACE_ME control is denied by the host, so independent attach resistance and hardened-runtime/signing acceptance remain unverified |
| Org AEAD key zeroization | Owned wire/decoded key use Zeroizing. Org now explicitly enables the cipher's zeroize feature; the standalone graph previously lacked it (the full daemon inherited it through vault). Standalone org tests passed 7/7 and its feature graph confirms zeroize. Erasure of all IPC/allocator copies and caller-owned borrowed keys is not proven |
| Tauri spawn/localStorage | Source: launcher delegates to agentctl; no arbitrary process command API; localStorage holds validated display settings/demo layout. Runtime penetration test not performed |
| NoMandatoryConfinement paths | Production backend selection and Runner gates inspected; direct helper fence implemented and adversarially tested. Only helper profiles receive read-only signature-query authority. Full governed execution is unverified here: even sandbox-exec launching true fails sandbox_apply with EPERM |
| INV-008 versus display-only header | Header corrected; durable provider text and evaluator summary use shared redaction; credential-shaped evidence refs refuse before interpretation/evaluation. Heuristics do not recognize every possible secret |
| TOCTOU timing | Deterministic descriptor replacement and same-inode content-change rejection tested. Timing stress PoC not run; timestamp checks do not prove atomic snapshots or prevent later writes |
| Runtime-root leaf sharing | Reproduced same-leaf sharing and a legacy leaf aliasing a new hashed name. Full-path SHA-256 namespaces plus a fresh provider-environments-v2 daemon base now isolate state. Regression, 94 provider unit tests, 32 adapter/contract tests and 11 service-binding tests passed. Old directories remain untouched; old provider-native sessions are not automatically migrated |
| Other malformed hex paths | Worker IPC and CLI org-plaintext rendering both reproduced UTF-8 boundary panics. Worker returns typed invalid_input without mutation, then accepts a valid enrollment. CLI rejects Unicode, odd length and invalid digits without partial success. Both regressions passed |
| Hostile Git repository | All 25 safety_regression tests passed |
| Live CVEs / parser / redirects | RustSec upstream HEAD 965bf9ad was checked live against the exact cached base: 1403 advisory files match, and all three upstream additions concern absent packages. Normal audits still report zero vulnerability entries; strict warning-denying audits fail on existing warnings. See the pinned provenance in the main ledger. CLI option-boundary probes ran; live inference and HTTP redirect execution remain open or locally blocked |

The combined evidence is in
[AUDIT_REMEDIATION_2026-11-03.md](./AUDIT_REMEDIATION_2026-11-03.md).
The selected OS policy is
[ADR-0050](../architecture/adr/ADR-0050-os-user-presence-for-local-control.md).
No task ID, product version, release approval, signing identity or owner-only
repository setting is inferred.
