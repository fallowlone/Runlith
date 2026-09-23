//! G8 prerequisite preflight service (TASK-0135).
//!
//! Read-only validation of declared [`task_domain::Prerequisite`]s before
//! task activation: Git refs must resolve and be ancestors of `HEAD`,
//! repo-relative files must exist inside the repository working tree, and
//! required commands must be on `PATH`.
//!
//! Authority boundary: preflight informs, the orchestrator decides. This
//! module performs no task transitions and persists nothing itself; a
//! caller maps `Failed` findings onto the existing journaled
//! `TransitionCommand::Block` path when it chooses to block, so blocker
//! reports remain durable through the normal task record. Findings are
//! three-valued on purpose: `Failed` means "checked and not satisfied",
//! `Indeterminate` means "could not be checked" (backend failure,
//! unusable repository root, missing `PATH`) — the two are never
//! conflated, and neither is ever treated as satisfied.
//!
//! Repo-escape protection is defense in depth: the vocabulary
//! ([`task_domain::Prerequisite`]) already rejects absolute paths and
//! `..` components; the checks here additionally prove containment of the
//! canonicalized target inside the canonicalized repository root so
//! symlink escapes fail closed as well.

use std::path::{Path, PathBuf};

use task_domain::{Prerequisite, PrerequisiteKind};
use workspace::{GitPort, TrustedCommonGit, WorkspaceFailure};

/// Outcome of checking one prerequisite.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrerequisiteStatus {
    /// Checked and satisfied.
    Satisfied,
    /// Checked and positively not satisfied (structured blocker input).
    Failed { reason: String },
    /// Could not be checked (backend or environment failure). Never
    /// treated as satisfied; callers decide whether it blocks.
    Indeterminate { reason: String },
}

/// One prerequisite paired with its check outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightFinding {
    pub kind: PrerequisiteKind,
    pub value: String,
    pub status: PrerequisiteStatus,
}

/// Bounded preflight result: one finding per declared prerequisite, in
/// declaration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightReport {
    findings: Vec<PreflightFinding>,
}

impl PreflightReport {
    #[must_use]
    pub fn findings(&self) -> &[PreflightFinding] {
        &self.findings
    }

    /// `true` only when every prerequisite is positively `Satisfied`.
    /// `Indeterminate` never counts as satisfied.
    #[must_use]
    pub fn all_satisfied(&self) -> bool {
        self.findings
            .iter()
            .all(|f| matches!(f.status, PrerequisiteStatus::Satisfied))
    }

    /// Findings that positively failed — the structured blocker payload a
    /// caller can record via the journaled task `Block` transition.
    pub fn blocking_failures(&self) -> Vec<&PreflightFinding> {
        self.findings
            .iter()
            .filter(|f| matches!(f.status, PrerequisiteStatus::Failed { .. }))
            .collect()
    }

    /// Human-readable blocker reason covering failures and indeterminate
    /// checks; `None` when everything is satisfied.
    #[must_use]
    pub fn blocker_reason(&self) -> Option<String> {
        if self.all_satisfied() {
            return None;
        }
        let mut parts = Vec::new();
        for finding in &self.findings {
            match &finding.status {
                PrerequisiteStatus::Satisfied => {}
                PrerequisiteStatus::Failed { reason } => parts.push(format!(
                    "prerequisite {} {:?} failed: {reason}",
                    finding.kind.as_str(),
                    finding.value
                )),
                PrerequisiteStatus::Indeterminate { reason } => parts.push(format!(
                    "prerequisite {} {:?} could not be checked: {reason}",
                    finding.kind.as_str(),
                    finding.value
                )),
            }
        }
        Some(parts.join("; "))
    }
}

/// Checks every declared prerequisite against the live environment and
/// returns the full report. Order is preserved; checks are independent
/// (one failure never aborts the rest).
#[must_use]
pub fn check_prerequisites<G: GitPort>(
    git: &G,
    repository_path: &str,
    expected: &TrustedCommonGit,
    prerequisites: &[Prerequisite],
) -> PreflightReport {
    let findings = prerequisites
        .iter()
        .map(|prerequisite| {
            let status = match prerequisite.kind() {
                PrerequisiteKind::GitAncestorRef => {
                    check_git_ancestor(git, repository_path, expected, prerequisite.value())
                }
                PrerequisiteKind::RepoFileExists => {
                    check_repo_file(repository_path, prerequisite.value())
                }
                PrerequisiteKind::CommandOnPath => check_command_on_path(prerequisite.value()),
            };
            PreflightFinding {
                kind: prerequisite.kind(),
                value: prerequisite.value().to_owned(),
                status,
            }
        })
        .collect();
    PreflightReport { findings }
}

fn check_git_ancestor<G: GitPort>(
    git: &G,
    repository_path: &str,
    expected: &TrustedCommonGit,
    reference: &str,
) -> PrerequisiteStatus {
    let ancestor = match git.resolve_revision(repository_path, reference, expected) {
        Ok(revision) => revision,
        Err(WorkspaceFailure::InvalidRevision { .. }) => {
            return PrerequisiteStatus::Failed {
                reason: format!("git ref {reference:?} does not resolve to a commit"),
            };
        }
        Err(other) => {
            return PrerequisiteStatus::Indeterminate {
                reason: format!("git backend could not resolve ref: {}", other.kind()),
            };
        }
    };
    let head = match git.resolve_revision(repository_path, "HEAD", expected) {
        Ok(revision) => revision,
        Err(other) => {
            return PrerequisiteStatus::Indeterminate {
                reason: format!("git backend could not resolve HEAD: {}", other.kind()),
            };
        }
    };
    match git.is_ancestor(repository_path, &ancestor, &head, expected) {
        Ok(true) => PrerequisiteStatus::Satisfied,
        Ok(false) => PrerequisiteStatus::Failed {
            reason: format!("git ref {reference:?} is not an ancestor of HEAD"),
        },
        Err(other) => PrerequisiteStatus::Indeterminate {
            reason: format!("git backend could not prove ancestry: {}", other.kind()),
        },
    }
}

/// Existence plus canonical containment: the target must resolve inside
/// the canonical repository root, so a symlink pointing outside the
/// working tree fails closed even though the vocabulary already rejected
/// lexical escapes.
fn check_repo_file(repository_path: &str, relative: &str) -> PrerequisiteStatus {
    let root = match std::fs::canonicalize(repository_path) {
        Ok(root) => root,
        Err(error) => {
            return PrerequisiteStatus::Indeterminate {
                reason: format!("cannot canonicalize repository root: {error}"),
            };
        }
    };
    let candidate = root.join(relative);
    if !candidate.exists() {
        return PrerequisiteStatus::Failed {
            reason: format!("repo-relative path {relative:?} does not exist"),
        };
    }
    match std::fs::canonicalize(&candidate) {
        Ok(resolved) if resolved.starts_with(&root) => PrerequisiteStatus::Satisfied,
        Ok(_) => PrerequisiteStatus::Failed {
            reason: format!("repo-relative path {relative:?} escapes the repository root"),
        },
        Err(error) => PrerequisiteStatus::Indeterminate {
            reason: format!("cannot canonicalize repo-relative path: {error}"),
        },
    }
}

fn check_command_on_path(name: &str) -> PrerequisiteStatus {
    let Some(path_var) = std::env::var_os("PATH") else {
        return PrerequisiteStatus::Indeterminate {
            reason: "PATH is not set".to_owned(),
        };
    };
    let search_paths: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    if command_found_in(name, &search_paths) {
        PrerequisiteStatus::Satisfied
    } else {
        PrerequisiteStatus::Failed {
            reason: format!("command {name:?} was not found on PATH"),
        }
    }
}

/// Testable core of the PATH search: an executable regular file named
/// `name` in any search directory. Unix requires an execute bit; other
/// platforms accept a regular file (executable-extension policy is a
/// runner concern, not a preflight one).
fn command_found_in(name: &str, search_paths: &[PathBuf]) -> bool {
    search_paths
        .iter()
        .any(|dir| is_executable_file(&dir.join(name)))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use workspace::{
        BranchRef, DiffEvidence, ExecutionId, RepositoryRef, Revision, WorkspaceLease,
        WorktreeStatus,
    };

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// Test-owned temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("preflight-{}-{id}-{tag}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Fake backend: canned resolve/ancestry answers, everything else
    /// fail-closed stubs. The preflight service must never call the
    /// mutating operations.
    struct FakeGit {
        resolve_ref: Result<Revision, WorkspaceFailure>,
        resolve_head: Result<Revision, WorkspaceFailure>,
        ancestry: Result<bool, WorkspaceFailure>,
    }

    impl FakeGit {
        fn happy(ancestry: bool) -> Self {
            Self {
                resolve_ref: Ok(Revision::new(OID_A).unwrap()),
                resolve_head: Ok(Revision::new(OID_B).unwrap()),
                ancestry: Ok(ancestry),
            }
        }
    }

    fn backend_error(detail: &str) -> WorkspaceFailure {
        WorkspaceFailure::GitBackendFailure {
            detail: detail.to_owned(),
        }
    }

    impl GitPort for FakeGit {
        fn backend_ref(&self) -> &'static str {
            "fake-git"
        }

        fn establish_trusted_anchor(
            &self,
            _path: &str,
        ) -> Result<TrustedCommonGit, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn is_repository(
            &self,
            _path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn default_branch(
            &self,
            _path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<String, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn resolve_revision(
            &self,
            _path: &str,
            rev: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<Revision, WorkspaceFailure> {
            if rev == "HEAD" {
                self.resolve_head.clone()
            } else {
                self.resolve_ref.clone()
            }
        }

        fn is_ancestor(
            &self,
            _path: &str,
            _ancestor: &Revision,
            _descendant: &Revision,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            self.ancestry.clone()
        }

        fn create_worktree(
            &self,
            _lease: &WorkspaceLease,
            _repository_path: &str,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<(), WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn worktree_status(
            &self,
            _path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<WorktreeStatus, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn compute_diff(
            &self,
            _lease: &WorkspaceLease,
            _expected: &TrustedCommonGit,
        ) -> Result<DiffEvidence, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn remove_worktree(
            &self,
            _lease: &WorkspaceLease,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<(), WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn remove_branch(
            &self,
            _repository_path: &str,
            _branch: &BranchRef,
            _expected: &TrustedCommonGit,
        ) -> Result<(), WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn branch_exists(
            &self,
            _repository_path: &str,
            _branch: &BranchRef,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn is_registered_worktree(
            &self,
            _repository_path: &str,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            Err(backend_error("stub"))
        }

        fn owner_is_live(
            &self,
            _repository: &RepositoryRef,
            _owner: &ExecutionId,
        ) -> Result<bool, WorkspaceFailure> {
            Err(backend_error("stub"))
        }
    }

    fn anchor(dir: &TempDir) -> TrustedCommonGit {
        let canonical = dir.path().canonicalize().expect("canonical temp dir");
        TrustedCommonGit::from_canonical_commondir(canonical).expect("anchor")
    }

    fn prerequisite(kind: PrerequisiteKind, value: &str) -> Prerequisite {
        Prerequisite::new(kind, value).expect("valid prerequisite")
    }

    #[test]
    fn git_ancestor_satisfied() {
        let dir = TempDir::new("git-ok");
        let report = check_prerequisites(
            &FakeGit::happy(true),
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[prerequisite(
                PrerequisiteKind::GitAncestorRef,
                "refs/heads/base",
            )],
        );
        assert!(report.all_satisfied());
        assert!(report.blocker_reason().is_none());
    }

    #[test]
    fn git_ref_not_ancestor_fails() {
        let dir = TempDir::new("git-not-ancestor");
        let report = check_prerequisites(
            &FakeGit::happy(false),
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[prerequisite(
                PrerequisiteKind::GitAncestorRef,
                "refs/heads/feature",
            )],
        );
        assert!(!report.all_satisfied());
        assert_eq!(report.blocking_failures().len(), 1);
        let reason = report.blocker_reason().expect("blocker reason");
        assert!(reason.contains("not an ancestor of HEAD"), "{reason}");
    }

    #[test]
    fn git_ref_that_does_not_resolve_fails() {
        let dir = TempDir::new("git-unresolved");
        let fake = FakeGit {
            resolve_ref: Err(WorkspaceFailure::InvalidRevision {
                detail: "no such ref".to_owned(),
            }),
            ..FakeGit::happy(true)
        };
        let report = check_prerequisites(
            &fake,
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[prerequisite(
                PrerequisiteKind::GitAncestorRef,
                "refs/heads/missing",
            )],
        );
        assert_eq!(report.blocking_failures().len(), 1);
        let reason = report.blocker_reason().expect("blocker reason");
        assert!(reason.contains("does not resolve"), "{reason}");
    }

    #[test]
    fn backend_failure_is_indeterminate_never_satisfied() {
        let dir = TempDir::new("git-backend-down");
        let fake = FakeGit {
            ancestry: Err(backend_error("merge-base crashed")),
            ..FakeGit::happy(true)
        };
        let report = check_prerequisites(
            &fake,
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[prerequisite(
                PrerequisiteKind::GitAncestorRef,
                "refs/heads/base",
            )],
        );
        assert!(!report.all_satisfied());
        assert!(report.blocking_failures().is_empty());
        assert!(matches!(
            report.findings()[0].status,
            PrerequisiteStatus::Indeterminate { .. }
        ));
    }

    #[test]
    fn repo_file_exists_and_missing() {
        let dir = TempDir::new("repo-file");
        std::fs::create_dir_all(dir.path().join("crates")).unwrap();
        std::fs::write(dir.path().join("crates/Cargo.toml"), b"[workspace]").unwrap();
        let report = check_prerequisites(
            &FakeGit::happy(true),
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[
                prerequisite(PrerequisiteKind::RepoFileExists, "crates/Cargo.toml"),
                prerequisite(PrerequisiteKind::RepoFileExists, "crates/missing.rs"),
            ],
        );
        assert!(matches!(
            report.findings()[0].status,
            PrerequisiteStatus::Satisfied
        ));
        assert!(matches!(
            report.findings()[1].status,
            PrerequisiteStatus::Failed { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn repo_file_symlink_escape_fails_closed() {
        let dir = TempDir::new("repo-escape");
        let outside = TempDir::new("repo-escape-target");
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let report = check_prerequisites(
            &FakeGit::happy(true),
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[prerequisite(
                PrerequisiteKind::RepoFileExists,
                "link/secret.txt",
            )],
        );
        assert_eq!(report.blocking_failures().len(), 1);
        let reason = report.blocker_reason().expect("blocker reason");
        assert!(reason.contains("escapes the repository root"), "{reason}");
    }

    #[cfg(unix)]
    #[test]
    fn command_search_requires_execute_bit() {
        let dir = TempDir::new("path-search");
        let executable = dir.path().join("tool-ok");
        std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = executable.metadata().unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).unwrap();
        }
        std::fs::write(dir.path().join("tool-no-exec"), b"#!/bin/sh\n").unwrap();
        let paths = [dir.path().to_path_buf()];
        assert!(command_found_in("tool-ok", &paths));
        assert!(!command_found_in("tool-no-exec", &paths));
        assert!(!command_found_in("tool-missing", &paths));
    }

    #[test]
    fn report_maps_failures_to_a_single_blocker_reason() {
        let dir = TempDir::new("multi");
        let report = check_prerequisites(
            &FakeGit::happy(false),
            dir.path().to_str().unwrap(),
            &anchor(&dir),
            &[
                prerequisite(PrerequisiteKind::GitAncestorRef, "refs/heads/base"),
                prerequisite(PrerequisiteKind::RepoFileExists, "missing.txt"),
            ],
        );
        let reason = report.blocker_reason().expect("blocker reason");
        assert!(reason.contains("git-ancestor-ref"), "{reason}");
        assert!(reason.contains("repo-file-exists"), "{reason}");
    }
}
