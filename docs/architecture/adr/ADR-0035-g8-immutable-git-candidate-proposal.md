# ADR-0035 — G8 Immutable Git Candidate Input (Proposal)

Status: **Proposed** (TASK-0149; owner ratification and independent
architecture/security review required)
Gate: G8

> This proposal defines a narrower initial candidate-input source for
> TASK-0144. It is not accepted and changes no existing decision. It grants
> no completion authority.

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

## Proposed decision

For the first `candidate-input` method only, accept an explicitly selected,
complete Git commit and its root tree as the candidate subject. Support only a
clean worktree whose `HEAD` is that commit at selection time. Staged changes,
unstaged changes, uncommitted tracked-file deletions, and untracked files make
this method unsupported. Committed deletions relative to the evaluated base
remain supported: the base/candidate diff records the deleted path, its base
mode and blob digest, and its absence from the candidate tree. Do not silently
include worktree changes or fall back to reading live paths.

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
never means “whatever bytes happen to be in the worktree.” The trusted product
requires `HEAD == selected commit`, the index tree to equal the selected root
tree, no tracked worktree differences, and no untracked paths when the
selection is made. It rechecks these conditions and the requirement/policy
before evaluation, after evaluation, before admitting the result, and at
completion. A detected change makes the attempt stale/blocked and cannot
produce or reuse successful `TEST_SUITE_RESULT`. Ignored files are not
candidate inputs and cannot be read by the evaluator, which receives only the
sealed snapshot. They are point-in-time freshness checks, not a lock on
external writers or a substitute for immutable capture.

These workspace checks are freshness gates, not capture evidence: a live
edit can be restored between checks, and there is no transaction spanning
worktree state and SQLite. Such an unobserved transient edit cannot alter the
Git-object-derived snapshot, so it does not change which commit bytes the
result describes. The product must describe and admit evidence only for that
selected commit subject; it must not claim continuous worktree cleanliness
or that the result describes an uncommitted/live candidate. If a criterion
requires those claims, this method is unsupported for it.

Before object resolution/evaluation, persist enough operation identity and
selection provenance to distinguish a retry from a new subject. Persist the
verified manifest and result bindings before making the effect usable. After
restart, revalidate object identities if re-materializing, the sealed
snapshot manifest if retained, result integrity, current requirement/policy,
and the selected subject. Before reuse after restart, repeat the point-in-time
HEAD/index/worktree cleanliness checks as well as requirement/policy checks.
Missing objects, lost or mutable snapshots, ambiguous operation state,
partial persistence, parser errors, duplicate replay, or any mismatch remain
blocked/stale/tampered; they never become success. Re-execution is permitted
only as a new or explicitly reconciled attempt and must bind the same exact
immutable subject. Retain durable bounded provenance even if temporary
snapshot bytes are later removed.

### Alternative: privileged APFS snapshot service

A product-managed privileged helper could take an APFS filesystem snapshot
and expose a read-only view. This may preserve the existing mutable-worktree
scope, but adds a privileged service, root-mediated IPC, entitlement and
signing/provisioning requirements, a larger attack surface, installation and
upgrade burden, and platform/filesystem availability constraints. It is not
selected by this proposal. It requires its own security design and owner
decision.

## Required changes if ratified

Ratification must explicitly supersede the following portions of accepted
contracts; this proposal does not edit or silently amend them:

- **ADR-0034 §1 and §5, Consequences and Acceptance/independent review:** replace
  the initial candidate-input requirement to atomically capture the mutable
  worktree with the selected-commit/tree method and its verified Git-object
  source, sealed product snapshot, clean-worktree gate, and stated freshness
  limits. Remove dirty/staged/untracked inputs from the initial supported
  method. Keep the stronger full-capture requirement for any later method
  that evaluates mutable worktree content or builds/runs candidate code.
- **ADR-0032 §1, §2 and §3:** replace the initial candidate’s dirty-worktree
  manifest assumption with the exact commit/tree/blob manifest and bind its
  digest into the integrity assessment and result. Retain the requirement to
  account for every input actually consumed by the selected check, and retain
  fail-closed treatment of inputs outside the supported tree/selector scope.
- **TASK-0144:** revise its implementation envelope, integrity assessment,
  freshness and acceptance criteria to the ratified candidate-input method.
  It remains blocked until ratification and must not claim compiled tests,
  generated/ignored inputs, external dependencies, or live-worktree contents
  without a separately accepted contract.

No accepted ADR status changes by this proposal. Any wider support for dirty
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
  a sealed product snapshot:** proposed as the smallest initial method with an
  immutable byte source and honest scope.
- **Install a privileged APFS snapshot helper:** viable future alternative
  when preserving mutable-worktree support justifies its authority and
  operational costs.

## Consequences and security impact

- Initial candidate-input evaluation becomes available only for clean,
  explicitly selected commits and supported tree contents. Ordinary dirty,
  staged, and untracked worktrees are unsupported by this method.
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
