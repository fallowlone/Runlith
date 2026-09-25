# ADR-0035 — G8 Immutable Git Candidate Input

Status: **Accepted** (TASK-0149; owner ratified after independent
architecture/security review PASS of exact pre-acceptance commit
`2c63a0946dc7a6aeea719fa3688c7c1e880d6539`)
Gate: G8

> This decision defines the initial candidate-input source for TASK-0144 and
> narrowly amends ADR-0032 and ADR-0034 as recorded below. It grants no
> completion authority.

## Context

Accepted ADR-0034 requires an atomic snapshot or equivalent race-safe capture
of the mutable worktree, including staged, unstaged, deleted, and untracked
inputs. The TASK-0144-v2 host harness does not consume candidate bytes, so its
output cannot establish that the candidate was evaluated. On macOS,
`fs_snapshot_create(2)` requires superuser privileges and an additional
entitlement. `clonefile(2)` is atomic for an individual clone operation, but
cloning paths one at a time does not establish one coherent point-in-time
view of a changing worktree. An ordinary unprivileged process therefore has
no demonstrated primitive for the ADR-0034 live-worktree capture contract.

Git commit and tree objects offer a possible immutable source, but only if the
product—not the caller or host harness—resolves and validates the complete
object graph and reads each blob itself. A commit identifier or metadata
alone does not prove which bytes were evaluated. A committed tree also does
not imply that generated, ignored, or external dependencies are present.

## Decision

For the first `candidate-input` method only, accept an explicitly selected,
complete Git commit and its root tree as the candidate subject. Support only a
linked worktree with a root `.git` pointer and a clean worktree whose `HEAD`
is that commit at selection time. Standalone clones with a root `.git`
directory are unsupported by this initial method. Staged changes, unstaged
changes, uncommitted tracked-file deletions, and untracked files make the
method unsupported. Committed deletions relative to the evaluated base remain
supported: the base/candidate diff records the deleted path, its base mode and
blob digest, and its absence from the candidate tree. Do not silently include
worktree changes or fall back to reading live paths.

The trusted product must:

1. Obtain the exact commit selection through a trusted operation bound to the
   Task, evaluated base, exact `EvidenceRequirement` revision, and authorized
   candidate-input selector. Provider output, a host-built harness, and a bare
   commit name or metadata cannot select or attest the candidate.
2. Resolve and validate the commit object, root tree, every nested tree, and
   each referenced blob for both candidate and evaluated-base commits.
   Independently validate each object using the repository's Git object
   framing and supported hash algorithm. Compute a versioned canonical
   SHA-256 manifest whose ordered entries contain raw repository-relative
   path bytes, Git mode/type, byte length, and SHA-256 of exact blob bytes;
   the manifest does not embed file contents. Reject ambiguous path encodings
   or collisions. Compare base and candidate manifests for the integrity
   assessment, recording committed deletions as described above. Bind both
   commit/tree OIDs, both manifest digests, encoding version, and candidate
   digest into the subject and invocation.
3. Use current G6 Policy authority for the exact Git executable and object
   store reads, and for product-owned snapshot creation/writes. Enforce those
   scoped operations at their process/filesystem boundaries; invoke Git
   plumbing through the governed G5/G6 Runner. Materialize the exact verified
   blob bytes, seal the product-owned snapshot against evaluator/candidate
   writes, and verify its complete path/mode/length/content-digest manifest
   after materialization and immediately before execution. The Runner invokes
   the product-owned evaluator with the snapshot read-only (or an equivalent
   bounded byte stream) so it consumes exactly the manifest-bound bytes.
4. Bind the durable assessment and structured result to the exact Task,
   attempt, base, selected commit/tree, manifest digest, requirement revision,
   policy revision/fingerprint, selector and criterion scope, evaluator
   identity/version, configuration, runtime/toolchain identity, and Runner
   enforcement outcome. The product validates the bounded structured result
   and persists it; child output and host harness summaries are untrusted.

The candidate-input selector may claim only properties it actually evaluates
from the supplied bytes. It does not compile or execute candidate code, run
candidate Rust tests, prove runtime behavior, or establish that the tree
contains generated or external inputs. Those requirements remain
blocked/unverified unless separately captured and authorized. Exact selector,
criterion scope, requirement revision, and Policy authorization are mandatory;
matching source text or a commit name alone is insufficient.

### Scoped G6 operation plans

For Git object reads, the product obtains the canonical absolute common-dir
path only from the authenticated `TrustedCommonGit` adapter anchor. It
revalidates that anchor against the repository before launch. The product then
requests a distinct, current `PolicyRequest` for
`ProcessExecute(<exact Git executable>)` and
`FilesystemRead(FileScope(workspace=<canonical common-dir path>, root='.'))`,
then narrows current `EffectiveCapabilities` to those exact capabilities for
one governed, read-only Git plumbing Runner plan. The anchor and scope are
never derived from provider data, a selector, repository config, or a
caller-supplied path. Anchor mismatch, denied/stale policy, unavailable
enforcement, or inability to narrow the authority blocks capture.

Snapshot materialization has a separate current G6 authorization and
filesystem scope. A product-owned in-process writer must enforce that scoped
authority at each filesystem boundary; a process-based writer gets its own
Runner plan and `EffectiveCapabilities`. Neither authority is combined with
the Git-read plan. The evaluator also gets a separate Runner plan, limited to
read access to the sealed snapshot and its required product runtime inputs;
it receives no common-Git, live-source, or snapshot-write authority. Plans do
not union or leak roots or capabilities across these operations. Existing
`PolicyRequest`, `FileScope`, `EffectiveCapabilities::narrow`, and Runner
contracts express this separation; no new capability model is proposed.
Denial, stale authority, scope mismatch, or missing enforcement at any plan
blocks that operation and the evidence capture.

### Git object source constraints

The product uses a pinned, trusted Git plumbing path that reads raw objects
without worktree checkout or clean/smudge filters. It must disable or reject
replacement-object and graft semantics; reject alternates and promisor/missing
objects; and fail closed for object formats or repository configurations it
cannot independently validate. Initial support excludes submodules/gitlinks,
LFS payloads, filters, sparse-worktree assumptions, symlink materialization,
special files, and any other source whose consumed bytes or semantics are not
fully represented and verified. A Git LFS pointer blob is only the bytes in
the tree; it is not evidence for the external LFS object. No trust is placed
in repository-local configuration to select executable plumbing, filters, or
external object sources. Inability to prove these conditions yields typed
unsupported/blocked capture and no successful result.

### Hostile tree validation and materialization

Treat tree entries and path bytes as hostile. Reject empty, `.` or `..`
components; absolute/rooted paths or platform prefixes; NULs or path
separators inside components; duplicate entries; file/tree prefix conflicts;
unsupported Git modes; and distinct paths that collide after the destination
filesystem's normalization or case rules. Treat any component equal to `.git`
under those same normalization/case rules as forbidden. Allow only
regular-file modes `100644` and `100755` plus tree entries; reject symlinks,
gitlinks, and every other mode. If the destination filesystem's path
comparison/normalization behavior cannot be established, fail closed.

Before materialization, enforce product-owned versioned limits for tree depth,
entry count, component and full-path length, and aggregate blob bytes. Reject
an over-limit or malformed tree without creating a candidate snapshot or
emitting an observed effect. Exclusively create a new product-owned snapshot
root for each capture; an existing root fails closed. Materialize with
directory handles and no-follow/beneath resolution
so a path cannot escape that root or redirect writes through a symlink.
Recompute and compare the complete path/mode/length/per-blob-digest manifest
after materialization. Any traversal, collision, bound, write, or verification
failure blocks capture and emits no `TEST_SUITE_RESULT` or successful effect.

Git objects are the race-safe capture source only after the product has
verified their object identities and complete tree traversal. The sealed
product snapshot is the evaluator’s input. Hashing the live worktree before
and after, `git status`, a commit OID without blob resolution, or per-path
cloning from the live worktree is not capture and cannot substitute for it.

### Subject freshness, workspace drift, and recovery

The evidence subject is the immutable tuple `(Task, evaluated base commit,
selected candidate commit, resolved root tree, canonical manifest digest,
requirement revision, policy fingerprint, selector/scope, invocation)`. It
never means “whatever bytes happen to be in the worktree.” Establish freshness
with a paired metadata observation (A) and live-filesystem observation (B),
bound to the same trusted common-Git anchor:

**A — linked HEAD and index.** Use the read-only Git plumbing Runner plan
above, with CWD inside the canonical common Git directory and pinned
`--git-dir=<validated linked metadata dir>`. The linked metadata path must be
authenticated through the trusted `TrustedCommonGit` anchor and bidirectional
linked-worktree back-reference validation. The product may receive a path
candidate from B's authorized root-pointer read, but must prove it resolves to
the linked metadata under the trusted common dir, its `gitdir` back-reference
names that exact root `.git` file, and its `commondir` resolves to the trusted
anchor. The mutable pointer alone is never authority. For each pair, B reads
the pointer under its worktree scope, A authenticates the corresponding
metadata/back-reference under its common-dir scope and makes its observations,
then B rereads the pointer immediately before and after scanning and requires
the same bytes and authenticated target. Any mismatch fails the pair. Run only
bounded object reads and linked-HEAD/index observations. Compare linked
`HEAD` with the selected commit, then compare every stage-zero index entry,
path, mode, and object ID with the selected tree; reject unmerged stages or any
mismatch. Do not use `write-tree`. This plan must not run `status`, worktree
diff, checkout, filter, hook, or any command that reads live worktree files.

**B — live worktree.** A product-owned in-process scanner requests current G6
`FilesystemRead(FileScope(workspace=<canonical managed worktree>, root='.'))`
and re-presents the narrowed authority for each directory open, `read_dir`,
metadata lookup, file open, and file read. It uses descriptor-based
beneath/no-follow traversal and launches no Git process. Compare every
expected regular-file path, mode, length, and SHA-256 with the selected
manifest, and compare the complete expected directory set. Missing or extra
paths—including ignored files—symlinks, special files, or mismatches stale the
observation. The only exception is the root `.git` pointer file: it is not a
manifest entry and must be structurally revalidated against A's authenticated
linked metadata path and common anchor before each pair. Reject a `.git`
directory or symlink and any missing, malformed, or changed pointer.

Bind each A+B pair to the same anchor, Task, execution, attempt, base and
selected commit/tree, manifest version/digest, exact requirement revision,
policy revision/fingerprint, selector/scope, operation identity, observation
times, and both outcomes.
Recheck the pair and bindings at selection, before and after evaluation,
before result admission, at completion, and before post-restart reuse. A
detected mismatch makes evidence stale/blocked and cannot produce or reuse a
successful `TEST_SUITE_RESULT`. These sequential checks are freshness gates,
not capture: changes can occur between observations, so the contract claims
neither continuous worktree cleanliness nor a race-free snapshot of live
files. The evaluator receives only the sealed Git-object-derived snapshot.
Evidence describes only the selected commit subject; criteria requiring a
claim about uninterrupted or uncommitted worktree state are unsupported.

Before object resolution/evaluation, persist enough operation identity and
selection provenance to distinguish a retry from a new subject. Persist the
verified manifest and result bindings before making the effect usable. After
restart, revalidate object identities if re-materializing, the sealed
snapshot manifest if retained, result integrity, and the complete A+B pair
with current requirement/policy. Missing objects, lost or mutable snapshots,
ambiguous operation state, partial persistence, parser errors, duplicate
replay, or any mismatch remain blocked/stale/tampered; they never become
success. Re-execution is permitted only as a new or explicitly reconciled
attempt and must bind the same exact immutable subject. Retain durable bounded
provenance even if temporary snapshot bytes are later removed.

### Alternative: privileged APFS snapshot service

A product-managed privileged helper could take an APFS filesystem snapshot
and expose a read-only view. This may preserve the existing mutable-worktree
scope, but adds a privileged service, root-mediated IPC, entitlement and
signing/provisioning requirements, a larger attack surface, installation and
upgrade burden, and platform/filesystem availability constraints. It is not
selected by this decision. It requires its own security design and owner
decision.

## Accepted amendments

Owner ratification followed independent architecture/security review PASS of
the exact pre-acceptance commit `2c63a0946dc7a6aeea719fa3688c7c1e880d6539`.
This decision explicitly amends the following portions of accepted contracts:

- **ADR-0034 §1, §2, §5, Consequences, Security impact, and Acceptance:** the
  initial candidate-input method is the explicitly selected immutable Git
  commit/tree method defined here, with validated object graphs and raw blobs,
  a canonical digest manifest, a sealed product-owned snapshot, and paired
  metadata/live-worktree freshness observations at selection, before and after
  evaluation, before result admission, at completion, and before post-restart
  reuse. It supports only a clean linked worktree. The mutable-worktree
  capture and compiled-test requirements remain applicable to those deferred
  methods; compiled candidate tests remain unsupported pending their separate
  structured-result and artifact contract.
- **ADR-0032 §1, §2 and §3:** for the initial candidate-input method, bind the
  exact selected commit/tree and validated raw-blob manifest digest into the
  integrity assessment and result. The broader dirty-worktree input rule
  applies only to a separately accepted mutable-worktree method. Every input
  actually consumed by the selected check must still be accounted for, and
  inputs outside the supported tree/selector scope fail closed.
- **TASK-0144:** proceed after TASK-0149 ratification and local integration,
  using only this clean linked-worktree commit method for candidate-input
  evidence. Do not claim compiled tests, generated/ignored inputs, external
  dependencies, or mutable live-worktree contents without a separately
  accepted method.

This acceptance changes no completion authority and authorizes no
implementation, migration, or dependency. Any wider support for mutable dirty
worktrees, LFS/submodules/filters, generated or external dependencies, or
candidate-compiled tests requires a separately specified and ratified method.

## Alternatives considered

- **Continue with mutable-worktree capture and pre/post hashes:** rejected for
  the initial unprivileged macOS method; hashes cannot close the capture race.
- **Clone the live tree or each file with `clonefile(2)`:** rejected as a
  coherent capture source; a sequence of individually atomic clones is not a
  single filesystem point-in-time snapshot.
- **Accept a commit OID or host-built harness result without resolving blobs:**
  rejected because it does not prove which candidate bytes were inspected.
- **Use selected commit/tree objects, verify all consumed blobs, then evaluate
  a sealed product snapshot:** selected as the smallest initial method with an
  immutable byte source and bounded scope.
- **Install a privileged APFS snapshot helper:** viable future alternative
  when preserving mutable-worktree support justifies its authority and
  operational costs.

## Consequences and security impact

- Initial candidate-input evaluation becomes available only for explicitly
  selected commits in supported linked worktrees, supported tree contents,
  and a matching live worktree. Standalone clones with a `.git` directory are
  unsupported. Dirty, staged, untracked, or ignored extra paths make freshness
  fail.
- The product owns object resolution, manifest creation, snapshot
  materialization, verification, invocation binding, result validation, and
  durable recovery. Candidate code, provider, caller, and host harness do not
  author these facts.
- A commit tree does not prove completeness for generated files, ignored
  inputs, toolchains, external dependencies, submodule contents, or LFS
  payloads. The exact requirement and selector must limit claims accordingly.
- Policy remains the authority source and G5/G6 Runner remains the enforcement
  boundary. No NativeRunner fallback is allowed when required isolation is
  unavailable. No result is emitted on unsupported Git semantics, failed
  verification, mismatch, or ambiguous recovery.
- APFS privileged snapshotting is unselected. No implementation, migration,
  dependency, or completion-authority change is authorized here.

## Amendment 2026-09-25: Seatbelt ancestor metadata for candidate launches

**Why needed.** Absolute-path resolution (`getcwd()`/`realpath()`), which a
candidate-input Runner launch performs when positioning its CWD via
`chdir` or resolving a linked Git worktree's `commondir` back-reference,
walks every ancestor directory of the path being resolved. A macOS
Seatbelt `subpath` grant covers only the given path and its descendants,
never its ancestors, so a workspace or root several directories under a
narrow allowed subtree failed closed with a misleading "Operation not
permitted" even though the launch never reads outside its granted scope.

**Exact grant.** `file-read-metadata` and `file-test-existence` only
(stat/existence, never directory listing or file data) on the exact
ancestor chain of the launch's workspace and filesystem roots, each
emitted as an individual `literal` rule — never `subpath` or a pattern —
after rejecting any source path containing a `..`/`.` component or a
doubled separator. The filesystem root `/` itself yields no literal.

**Candidate launches only.** The grant is owned by a typed,
construction-time attribute (`RunnerLaunchPurpose` /
`MacOsSeatbeltRunner::with_candidate_ancestor_metadata_grant`) that only
`bins/agentd/src/execution.rs`'s trusted composition sets, only for the
Git-read, snapshot-write, and evaluator-read-only candidate-input Runner
operations. It defaults to absent (fail closed) and is never derived
from caller, IPC, or per-launch input. Provider/tool launches never
receive it and keep exactly the pre-amendment Seatbelt profile and
behavior.

Owner ratified 2026-09-25. This amendment narrows and documents a
detail of the read-only Git plumbing plan's enforcement already
authorized by this ADR's "Scoped G6 operation plans" and by ADR-0026's
G6 Seatbelt authority surface; it grants no new filesystem data or
listing access beyond what those sections already authorize, and no
completion authority.

## Amendment 2026-09-25: pinned Git executable for candidate-input reads

**Why needed.** Candidate git-read launches resolved the Git executable
from `PATH`, which may select a package-manager build such as Homebrew
Git. Such a build loads dynamic libraries from its own prefix; under the
candidate Seatbelt profile dyld cannot read them and the process aborts
(observed: `Library not loaded: /opt/homebrew/opt/pcre2/lib/libpcre2-8.0.dylib`,
SIGABRT). A `PATH`-resolved Git is also not the "pinned, trusted Git
plumbing path" required by this ADR's "Git object source constraints".

**Exact selection.** On macOS, candidate-input Git resolves only from
this ordered list of Apple toolchain locations:
`/Library/Developer/CommandLineTools/usr/bin/git`, then
`/Applications/Xcode.app/Contents/Developer/usr/bin/git`. It never uses
`PATH` or the `/usr/bin/git` xcrun shim, which depends on xcode-select
state and caches outside the Seatbelt scope. A candidate is accepted only
if it exists, canonicalizes to a regular file, and its canonical path
equals its parent directory's canonical path joined with its file name, so
a symlink redirecting the executable elsewhere is rejected. If no candidate
qualifies, resolution fails closed with a path-free error; there is no
fallback to `PATH` and no sandbox widening.

On non-macOS platforms, candidate-input Git reads are unsupported, and
the resolver falls back to the existing `resolve_git_executable()` PATH
behavior since the candidate Seatbelt restriction does not apply there.

**Scope.** The pinned Git executable is used only for candidate-input
Git-read operations under `resolve_candidate_git_executable()`. Provider
and workspace Git operations continue using the prior
`resolve_git_executable()` PATH-based resolution, unchanged.

Owner ratified 2026-09-25. Grants no new filesystem, process or
completion authority.

## Amendment 2026-09-25: evidence-only production evaluation and permanent invalidation

**Why needed.** An independent adversarial review of TASK-0144 found
(a) the production composition turned a candidate-input check into a Task
completion (or failure) verdict, although TASK-0144's scope records typed
integrity and capture statuses, not completion verdicts, and adds no
completion authority; completion belongs to TASK-0125 claim verification
(ADR-0032); and (b) a tamper or freshness mismatch detected after result
admission was only journaled, so the evidence could still be read, reused,
checkpointed and completed, contrary to this ADR ("a detected mismatch
makes evidence stale/blocked and cannot produce or reuse a successful
TEST_SUITE_RESULT").

**Evidence-only production evaluation.** The production candidate-input
evaluation durably admits bound evidence (operation status RESULT_ADMITTED
with its Selection, BeforeEvaluation, AfterEvaluation and ResultAdmission
checkpoints) and then reports no verdict, so the Task remains VERIFYING.
Passing or failing the selector check is recorded in the evidence, never
applied to the Task. The Completion freshness checkpoint and the
RESULT_ADMITTED → COMPLETE transition remain available only to a future
completion-stage caller (TASK-0125), which must invoke them before any
completion.

**Permanent invalidation.** Operation status stays monotonic: it never
regresses from RESULT_ADMITTED or COMPLETE. Integrity signals set a
permanent invalidation (time and a fixed status:code) on the operation.
A database trigger forbids changing or clearing it. Integrity signals are
decided by the call site that detects them, never by matching error text.
They are: the linked-worktree pointer, the linked-worktree gitdir
back-reference, the selection binding, the Git
object sources and object graph, the reuse evidence binding, the Completion
immutable subject, the evaluator result disagreeing with the trusted
recomputation, and every freshness checkpoint on the evaluation,
result-admission, reuse and Completion paths. A selection mismatch counts
only when the durable selection is bound to a different subject; an
interrupted operation for the same subject is journaled only. Within
those checks, a failure attributable to repository or worktree
content or state is an integrity signal: object, pointer, gitdir or
configuration content; a missing, special or changed file; and a check
process that exits unsuccessfully, produces oversized output, times out
or exits before reading its input. An unclassified failure inside a check
defaults to an integrity signal. Every evidence read,
integrity-assessment read,
checkpoint append and COMPLETE transition refuses an invalidated
operation. If an integrity signal's invalidation cannot be recorded, the
failing call reports that alongside its error. Failures independent of
content are journaled only and never invalidate.
These are the product failing to launch or wait for a process, a G6
authorization or storage error, a policy or Runner refusal, any other
storage error, and a Task or attempt not in the evaluatable state. The
same applies to the snapshot-write, evaluator and snapshot-removal
launches, which are not integrity checks; the evaluator's reported result
is still checked as an integrity signal. Admitted evidence survives a
transient failure and an idempotent retry stays safe. Owner decision
2026-09-25, following an adversarial re-review.

**Special files.** Descriptor-relative reads open entries non-blocking, so a
FIFO or other special file in the worktree, in linked metadata, or in place
of `.git` makes the check stale promptly instead of blocking it. Such a
failure is an integrity signal and permanently invalidates the operation.

Owner ratified 2026-09-25. Narrows authority: removes completion verdicts
from TASK-0144's production path and grants no new filesystem, process or
completion authority.

## References

- ADR-0018 — independent evaluation and completion authority.
- ADR-0026 — G6 security authority and capability contracts.
- ADR-0030 — CV-1..CV-9 and subject freshness.
- ADR-0032 — test integrity and trusted evidence authoring.
- ADR-0034 — accepted candidate-bound execution contract.
- `adapters/git-cli/src/observe.rs` — current Git observation covers HEAD
  and index only.
- `adapters/git-cli/src/runner.rs` and `crates/workspace/src/trusted.rs` —
  trusted common-Git anchor derivation, canonical-path validation, and
  per-operation anchor revalidation.
- `crates/evaluation/src/capture.rs` — current capture request/effect seam.
- `crates/runner/src/lib.rs` — G6 Runner enforcement contract.
- `crates/policy/src/lib.rs` — `PolicyRequest`, `FileScope`, and narrowed
  `EffectiveCapabilities` contracts.
- [Apple `fs_snapshot_create(2)`](https://github.com/apple/darwin-xnu/blob/main/bsd/man/man2/fs_snapshot_create.2)
  documents the superuser and additional-entitlement requirements.
- [Apple `clonefile(2)`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/clonefile.2)
  documents individual clone behavior and discourages directory-hierarchy
  cloning; no multi-file point-in-time guarantee is claimed here.
