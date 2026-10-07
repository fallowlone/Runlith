//! `git-cli` participation in the shared finder revision contract.
//!
//! The same [`repo_finder_contract::run_finder_revision_contract`]
//! every revision backend must pass, driven here by [`GitCli`]
//! against hermetic Git fixtures (real `git`, temp dirs, no network).
//! Binding specifics (HEAD OID, dirtiness) are pinned below; the
//! runner pins the seam: stable bounded bindings on real
//! repositories, [`FinderError::NotARepository`](repo_finder::FinderError)
//! everywhere else.

mod common;

use common::Fixture;
use git_cli::GitCli;
use repo_finder::{RepoRevisionPort, RepoRoot};
use repo_finder_contract::run_finder_revision_contract;

#[test]
fn git_cli_satisfies_the_shared_revision_contract() {
    let fixture = Fixture::new("finder-revision-contract");
    let root = RepoRoot::new(fixture.dir.clone()).unwrap();
    run_finder_revision_contract(&GitCli::new(), Some(&root));
}

#[test]
fn resolves_head_oid_of_fixture_repo() {
    let fixture = Fixture::new("finder-revision-head");
    let root = RepoRoot::new(fixture.dir.clone()).unwrap();
    let provenance = GitCli::new().resolve(&root).unwrap();
    assert_eq!(provenance.revision.as_str(), fixture.primary_head());
    assert!(!provenance.dirty, "fresh fixture must bind clean");
    assert_eq!(provenance.root, root);
}

#[test]
fn reports_dirty_worktree() {
    let fixture = Fixture::new("finder-revision-dirty");
    fixture.write_tracked("dirty.txt", b"uncommitted\n");
    let root = RepoRoot::new(fixture.dir.clone()).unwrap();
    let provenance = GitCli::new().resolve(&root).unwrap();
    assert!(provenance.dirty, "uncommitted file must bind dirty");
}
