//! `IndexedFinder` behavior: byte-identical outcomes with the scan
//! path, plus the snapshot-cache contract (rev-keyed reuse, dirty
//! bypass, size-cap fallback).
//!
//! Equivalence with [`LocalFinder`] is the core promise: the same
//! queries against the same fixture must produce identical hits
//! (ids, excerpts, scores), skips, totals, and continuations.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use finder_local::{IndexedFinder, LocalFinder};
use repo_finder::{
    FinderError, FinderQuery, RepoProvenance, RepoRevisionPort, RepoRoot, RepoSearchPort, Revision,
    SearchOutcome,
};
use repo_finder_contract::{ContractRepo, NEEDLE_ALPHA, NEEDLE_BETA};

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

struct MutableRevision {
    rev: Mutex<String>,
    dirty: Mutex<bool>,
}

impl RepoRevisionPort for MutableRevision {
    fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
        Ok(RepoProvenance {
            root: root.clone(),
            revision: Revision::new(self.rev.lock().unwrap().clone()).unwrap(),
            dirty: *self.dirty.lock().unwrap(),
        })
    }
}

fn scan_engine() -> LocalFinder {
    LocalFinder::new(Arc::new(StubRevision))
}

struct ScratchRepo {
    dir: PathBuf,
    root: RepoRoot,
}

impl ScratchRepo {
    fn with_files(files: &[(&str, &[u8])]) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rf-indexed-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        for (name, bytes) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
        }
        let root = RepoRoot::new(dir.clone()).unwrap();
        Self { dir, root }
    }

    fn root(&self) -> &RepoRoot {
        &self.root
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn indexed_engine() -> IndexedFinder {
    IndexedFinder::new(Arc::new(StubRevision))
}

/// Outcomes must match field-for-field (provenance rev aside: both
/// bind `stubrev` here anyway).
fn assert_outcomes_equal(scan: &SearchOutcome, indexed: &SearchOutcome) {
    assert_eq!(scan.hits.total_hits, indexed.hits.total_hits);
    assert_eq!(scan.hits.hits.len(), indexed.hits.hits.len());
    for (left, right) in scan.hits.hits.iter().zip(indexed.hits.hits.iter()) {
        assert_eq!(left.evidence_id.as_str(), right.evidence_id.as_str());
        assert_eq!(left.path, right.path);
        assert_eq!(left.span, right.span);
        assert_eq!(left.excerpt, right.excerpt);
        assert_eq!(left.reason, right.reason);
    }
    assert_eq!(scan.hits.skipped, indexed.hits.skipped);
    assert_eq!(
        scan.hits.continuation.as_ref().map(|token| token.as_str()),
        indexed
            .hits
            .continuation
            .as_ref()
            .map(|token| token.as_str())
    );
}

#[test]
fn indexed_matches_scan_on_fixture_queries() {
    let repo = ContractRepo::build();
    let scan = scan_engine();
    let indexed = indexed_engine();
    for (text, limit) in [
        (NEEDLE_ALPHA, 50),
        (NEEDLE_ALPHA, 1),
        (NEEDLE_BETA, 10),
        ("fn main", 10),
        ("V0", 5),
        ("st", 5),
        ("e", 5),
    ] {
        let query = FinderQuery::new(text, limit).unwrap();
        let scan_outcome = scan.search(repo.root(), &query).unwrap();
        let indexed_outcome = indexed.search(repo.root(), &query).unwrap();
        assert_outcomes_equal(&scan_outcome, &indexed_outcome);
    }
}

#[test]
fn indexed_matches_scan_with_scope_and_continuation() {
    let repo = ContractRepo::build();
    let scan = scan_engine();
    let indexed = indexed_engine();
    let query = FinderQuery::new(NEEDLE_ALPHA, 1)
        .unwrap()
        .with_scope(PathBuf::from("src"))
        .unwrap();
    let scan_page = scan.search(repo.root(), &query).unwrap();
    let indexed_page = indexed.search(repo.root(), &query).unwrap();
    assert_outcomes_equal(&scan_page, &indexed_page);
    if let Some(token) = scan_page.hits.continuation.clone() {
        let next = FinderQuery::new(NEEDLE_ALPHA, 1)
            .unwrap()
            .with_scope(PathBuf::from("src"))
            .unwrap()
            .replace_continuation(Some(token));
        let scan_next = scan.search(repo.root(), &next).unwrap();
        let indexed_next = indexed.search(repo.root(), &next).unwrap();
        assert_outcomes_equal(&scan_next, &indexed_next);
    }
}

#[test]
fn indexed_keeps_filename_only_matches() {
    // The prefilter must admit files whose NAME matches even when no
    // content line does; empty files stay non-hits in both engines.
    let scratch = ScratchRepo::with_files(&[
        ("needle_config.txt", b"nothing relevant here\n" as &[u8]),
        ("empty_needle.txt", b"" as &[u8]),
        ("plain.rs", b"fn main() {}\n" as &[u8]),
    ]);
    let scan = scan_engine();
    let indexed = indexed_engine();
    let query = FinderQuery::new("needle_config", 10).unwrap();
    let scan_outcome = scan.search(scratch.root(), &query).unwrap();
    let indexed_outcome = indexed.search(scratch.root(), &query).unwrap();
    assert_eq!(scan_outcome.hits.total_hits, 1);
    assert_outcomes_equal(&scan_outcome, &indexed_outcome);
    let query = FinderQuery::new("empty_needle", 10).unwrap();
    let scan_outcome = scan.search(scratch.root(), &query).unwrap();
    let indexed_outcome = indexed.search(scratch.root(), &query).unwrap();
    assert_eq!(scan_outcome.hits.total_hits, 0);
    assert_outcomes_equal(&scan_outcome, &indexed_outcome);
}

#[test]
fn indexed_reuses_snapshot_within_one_revision() {
    // Same stub rev across a filesystem mutation: the indexed engine
    // serves the cached snapshot (stale by design), the scan engine
    // sees fresh bytes. Deterministic cache pin without timing.
    let repo = ContractRepo::build();
    let scan = scan_engine();
    let indexed = indexed_engine();
    let query = FinderQuery::new("cacheprobe_xyz", 10).unwrap();
    assert_eq!(scan.search(repo.root(), &query).unwrap().hits.total_hits, 0);
    assert_eq!(
        indexed.search(repo.root(), &query).unwrap().hits.total_hits,
        0
    );
    std::fs::write(
        repo.root().as_path().join("src").join("main.rs"),
        "fn main() {\n    println!(\"cacheprobe_xyz\");\n}\n",
    )
    .unwrap();
    assert_eq!(
        scan.search(repo.root(), &query).unwrap().hits.total_hits,
        1,
        "scan must see the mutated file"
    );
    assert_eq!(
        indexed.search(repo.root(), &query).unwrap().hits.total_hits,
        0,
        "indexed must serve the cached snapshot within one revision"
    );
}

#[test]
fn indexed_rebuilds_on_revision_change() {
    let repo = ContractRepo::build();
    let revision = Arc::new(MutableRevision {
        rev: Mutex::new("r1".to_string()),
        dirty: Mutex::new(false),
    });
    let indexed = IndexedFinder::new(revision.clone());
    let query = FinderQuery::new("revprobe_xyz", 10).unwrap();
    assert_eq!(
        indexed.search(repo.root(), &query).unwrap().hits.total_hits,
        0
    );
    std::fs::write(
        repo.root().as_path().join("src").join("main.rs"),
        "fn main() {\n    println!(\"revprobe_xyz\");\n}\n",
    )
    .unwrap();
    *revision.rev.lock().unwrap() = "r2".to_string();
    assert_eq!(
        indexed.search(repo.root(), &query).unwrap().hits.total_hits,
        1,
        "revision change must rebuild the snapshot"
    );
}

#[test]
fn indexed_bypasses_cache_when_dirty() {
    let repo = ContractRepo::build();
    let revision = Arc::new(MutableRevision {
        rev: Mutex::new("r1".to_string()),
        dirty: Mutex::new(true),
    });
    let indexed = IndexedFinder::new(revision.clone());
    let query = FinderQuery::new("dirtyprobe_xyz", 10).unwrap();
    assert_eq!(
        indexed.search(repo.root(), &query).unwrap().hits.total_hits,
        0
    );
    std::fs::write(
        repo.root().as_path().join("src").join("main.rs"),
        "fn main() {\n    println!(\"dirtyprobe_xyz\");\n}\n",
    )
    .unwrap();
    let outcome = indexed.search(repo.root(), &query).unwrap();
    assert_eq!(
        outcome.hits.total_hits, 1,
        "dirty worktrees must bypass the snapshot"
    );
    assert_eq!(
        outcome.backend.name, "finder-local-indexed",
        "fallback outcomes carry the serving engine's identity"
    );
}

#[test]
fn indexed_matches_scan_across_full_page_walk() {
    // Every page of a multi-page listing must match, not just the
    // first page-turn.
    let repo = ContractRepo::build();
    let scan = scan_engine();
    let indexed = indexed_engine();
    let mut scan_token = None;
    let mut indexed_token = None;
    let mut pages = 0;
    loop {
        let scan_query = FinderQuery::new(NEEDLE_ALPHA, 1)
            .unwrap()
            .replace_continuation(scan_token);
        let indexed_query = FinderQuery::new(NEEDLE_ALPHA, 1)
            .unwrap()
            .replace_continuation(indexed_token);
        let scan_page = scan.search(repo.root(), &scan_query).unwrap();
        let indexed_page = indexed.search(repo.root(), &indexed_query).unwrap();
        assert_outcomes_equal(&scan_page, &indexed_page);
        pages += 1;
        assert!(pages <= 10, "page walk must terminate");
        match (
            scan_page.hits.continuation.clone(),
            indexed_page.hits.continuation.clone(),
        ) {
            (Some(left), Some(right)) => {
                assert_eq!(left.as_str(), right.as_str());
                scan_token = Some(left);
                indexed_token = Some(right);
            }
            (None, None) => break,
            differ => panic!("continuation presence must match, got {differ:?}"),
        }
    }
    assert!(pages >= 2, "fixture must yield several pages, got {pages}");
}

#[cfg(unix)]
#[test]
fn indexed_falls_back_to_scan_when_build_fails_out_of_scope() {
    // An unreadable file outside the query scope breaks the snapshot
    // build but not a scoped scan: the indexed engine must degrade,
    // not fail.
    use std::os::unix::fs::PermissionsExt;
    let scratch = ScratchRepo::with_files(&[
        ("src/ok.rs", b"fallback_needle_xyz\n" as &[u8]),
        ("secret/blocked.rs", b"fallback_needle_xyz\n" as &[u8]),
    ]);
    std::fs::set_permissions(
        scratch.root().as_path().join("secret/blocked.rs"),
        std::fs::Permissions::from_mode(0o0),
    )
    .unwrap();
    let scan = scan_engine();
    let indexed = indexed_engine();
    let query = FinderQuery::new("fallback_needle_xyz", 10)
        .unwrap()
        .with_scope(PathBuf::from("src"))
        .unwrap();
    let scan_outcome = scan.search(scratch.root(), &query).unwrap();
    let indexed_outcome = indexed.search(scratch.root(), &query).unwrap();
    assert_eq!(scan_outcome.hits.total_hits, 1);
    assert_outcomes_equal(&scan_outcome, &indexed_outcome);
    assert_eq!(indexed_outcome.backend.name, "finder-local-indexed");
}

#[test]
fn indexed_falls_back_to_scan_past_size_cap() {
    // A zero-byte cap forces the scan fallback: results stay correct.
    let repo = ContractRepo::build();
    let scan = scan_engine();
    let indexed = IndexedFinder::new(Arc::new(StubRevision)).with_max_snapshot_bytes(0);
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).unwrap();
    let scan_outcome = scan.search(repo.root(), &query).unwrap();
    let indexed_outcome = indexed.search(repo.root(), &query).unwrap();
    assert_outcomes_equal(&scan_outcome, &indexed_outcome);
    assert!(
        indexed_outcome.hits.total_hits > 0,
        "fallback must still find fixture needles"
    );
}
