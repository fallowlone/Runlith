//! `finder-local` participation in the shared finder contract suite.
//!
//! The same [`repo_finder_contract::run_finder_search_contract`] every
//! retrieval engine must pass, driven here by [`LocalFinder`] against
//! the hermetic fixture (no working-repo reads, no network). Engine
//! recall and thoroughness (gitignore honoring, skip explicitness,
//! paging) stay in `local_engine.rs`; this file answers only: does
//! the local engine satisfy the common retrieval seam?

use std::sync::Arc;

use finder_local::LocalFinder;
use repo_finder::{FinderError, RepoProvenance, RepoRevisionPort, RepoRoot, Revision};
use repo_finder_contract::run_finder_search_contract;

/// Revision backend returning a constant binding for any root: the
/// fixture is not a Git repository, and revision behavior is pinned
/// separately by the `git-cli` revision run.
struct StubRevision;

impl RepoRevisionPort for StubRevision {
    fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
        Ok(RepoProvenance {
            root: root.clone(),
            revision: Revision::new("stubrev").unwrap(),
            dirty: false,
        })
    }
}

#[test]
fn local_finder_satisfies_the_shared_search_contract() {
    let engine = LocalFinder::new(Arc::new(StubRevision));
    run_finder_search_contract(&engine, &engine);
}
