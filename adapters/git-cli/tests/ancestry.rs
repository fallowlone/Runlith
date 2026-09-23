//! TASK-0135: `GitPort::is_ancestor` proves commit ancestry through real
//! `git merge-base --is-ancestor` — exit status only, hermetic fixtures,
//! trusted anchor, fail-closed on backend failure.

mod common;

use common::{anchor_for, git, Fixture};
use git_cli::GitCli;
use workspace::{GitPort, Revision, WorkspaceFailure};

#[test]
fn ancestor_relationships_are_proven_exactly() {
    let fixture = Fixture::new("ancestry");
    let adapter = GitCli::new();
    let anchor = anchor_for(&adapter, &fixture.path_str());

    let base = Revision::new(fixture.base_oid.clone()).unwrap();
    git(
        &fixture.dir,
        &["commit", "-q", "--allow-empty", "-m", "second"],
    );
    let head_oid = fixture.primary_head();
    let head = Revision::new(head_oid.clone()).unwrap();

    // Base is an ancestor of the newer HEAD; HEAD is not an ancestor of
    // the older base; a revision is an ancestor of itself.
    assert!(adapter
        .is_ancestor(&fixture.path_str(), &base, &head, &anchor)
        .unwrap());
    assert!(!adapter
        .is_ancestor(&fixture.path_str(), &head, &base, &anchor)
        .unwrap());
    assert!(adapter
        .is_ancestor(&fixture.path_str(), &base, &base, &anchor)
        .unwrap());
}

#[test]
fn unrelated_histories_are_not_ancestors() {
    let fixture = Fixture::new("ancestry-unrelated");
    let adapter = GitCli::new();
    let anchor = anchor_for(&adapter, &fixture.path_str());

    git(
        &fixture.dir,
        &["checkout", "-q", "--orphan", "unrelated-history"],
    );
    git(
        &fixture.dir,
        &["commit", "-q", "--allow-empty", "-m", "orphan root"],
    );
    let orphan = Revision::new(fixture.primary_head()).unwrap();
    let base = Revision::new(fixture.base_oid.clone()).unwrap();

    assert!(!adapter
        .is_ancestor(&fixture.path_str(), &orphan, &base, &anchor)
        .unwrap());
    assert!(!adapter
        .is_ancestor(&fixture.path_str(), &base, &orphan, &anchor)
        .unwrap());
}

#[test]
fn backend_failure_fails_closed_instead_of_guessing() {
    let fixture = Fixture::new("ancestry-backend");
    let adapter = GitCli::with_executable("/definitely/missing/git-binary");
    let anchor = anchor_for(&GitCli::new(), &fixture.path_str());
    let base = Revision::new(fixture.base_oid.clone()).unwrap();

    let result = adapter.is_ancestor(&fixture.path_str(), &base, &base, &anchor);
    assert!(
        matches!(result, Err(WorkspaceFailure::GitBackendFailure { .. })),
        "expected fail-closed backend failure, got {result:?}"
    );
}
