//! Snapshot index behind the repo-finder seam (TASK-0174, G9).
//!
//! [`IndexedFinder`] serves clean revisions from an in-memory
//! snapshot: per-file lines plus a byte-trigram Bloom that prefilters
//! files before the exact `contains` verify. Recall is exact by
//! construction — a missing trigram proves absence, a hit only
//! admits to verification, short terms skip the prefilter, and
//! ranking/paging/minting reuse the scan path's shared code — so
//! indexed outcomes equal scan outcomes field-for-field. Dirty
//! worktrees and over-cap snapshots delegate to the scan path (same
//! outputs, current cost).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use repo_finder::{
    BackendId, FinderError, FinderQuery, RepoProvenance, RepoReadPort, RepoRevisionPort, RepoRoot,
    RepoSearchPort, SearchOutcome, SkippedSource,
};

use crate::{collect_candidates, reject_unrepresentable, IngestedFile, LocalFinder};

/// Default cap for one cached snapshot (ingested file bytes, not
/// index overhead). Past it the engine serves the scan path: same
/// outputs, no unbounded memory.
pub const DEFAULT_MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

/// Max cached `(root, revision)` snapshots. Past it the cache evicts
/// everything — a perf-only event, outputs never change.
const MAX_CACHED_SNAPSHOTS: usize = 8;

/// One snapshotted file: searchable lines plus the byte-trigram
/// Bloom over content lines AND the file name (filename-only
/// matches must survive the prefilter).
struct SnapshotFile {
    path: PathBuf,
    lines: Vec<String>,
    bloom: TrigramBloom,
}

/// One build-time skip in walk order: `None` replays always
/// (unrepresentable paths report even outside scope, like scan),
/// `Some(path)` replays only inside the query scope.
struct OrderedSkip {
    path: Option<PathBuf>,
    skip: SkippedSource,
}

/// Replays build-time skips for one query: globals always,
/// path-bound only inside scope. Walk order preserved.
fn replay_skips(skips: &[OrderedSkip], scope: Option<&std::path::Path>) -> Vec<SkippedSource> {
    skips
        .iter()
        .filter(|entry| match &entry.path {
            None => true,
            Some(path) => scope.is_none_or(|scope| path.starts_with(scope)),
        })
        .map(|entry| entry.skip.clone())
        .collect()
}

/// One bound snapshot: walk-order files, walk-order skips
/// (replayed per query, scope-filtered like the scan path), and the
/// ingested byte weight the size cap meters.
struct Snapshot {
    files: Vec<SnapshotFile>,
    skips: Vec<OrderedSkip>,
    bytes: u64,
}

/// Indexed retrieval engine. Construct with a revision backend like
/// [`LocalFinder`]; bounds mirror it via
/// [`IndexedFinder::with_bounds`]. Snapshot caching is interior
/// (`RwLock`): concurrent queries share one snapshot per root and
/// revision, and a poisoned lock degrades to the scan path rather
/// than failing.
pub struct IndexedFinder {
    scan: LocalFinder,
    revision: Arc<dyn RepoRevisionPort>,
    max_snapshot_bytes: u64,
    cache: RwLock<HashMap<(PathBuf, String), Snapshot>>,
}

impl IndexedFinder {
    pub fn new(revision: Arc<dyn RepoRevisionPort>) -> Self {
        Self {
            scan: LocalFinder::new(revision.clone()),
            revision,
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_bounds(mut self, max_file_bytes: u64, max_excerpt_chars: usize) -> Self {
        self.scan =
            LocalFinder::new(self.revision.clone()).with_bounds(max_file_bytes, max_excerpt_chars);
        self
    }

    pub fn with_max_snapshot_bytes(mut self, max_snapshot_bytes: u64) -> Self {
        self.max_snapshot_bytes = max_snapshot_bytes;
        self
    }

    fn backend_id() -> BackendId {
        BackendId::new("finder-local-indexed", "0.1.0").expect("static backend id must build")
    }

    /// Builds a snapshot with the exact scan ingest path: same walk,
    /// same per-file bytes, same skips, same failures.
    fn build_snapshot(&self, root_path: &std::path::Path) -> Result<Snapshot, FinderError> {
        let mut walked = Vec::new();
        self.scan.walk(root_path, &mut walked)?;
        let mut files = Vec::new();
        let mut skips = Vec::new();
        let mut bytes = 0u64;
        for relative in walked {
            if let Some(skip) = reject_unrepresentable(&relative) {
                skips.push(OrderedSkip { path: None, skip });
                continue;
            }
            match self.scan.ingest_file(root_path, &relative)? {
                IngestedFile::Skipped(skip) => skips.push(OrderedSkip {
                    path: Some(relative),
                    skip,
                }),
                IngestedFile::Content {
                    lines,
                    bytes: weight,
                } => {
                    bytes += weight;
                    let mut bloom = TrigramBloom::new(weight as usize);
                    for line in &lines {
                        for window in line.as_bytes().windows(3) {
                            bloom.insert(trigram_key(window));
                        }
                    }
                    // The file name feeds the same Bloom: a term
                    // matching only the name is still provably
                    // present, so filename hits survive. Sound: only
                    // UTF-8 paths reach here (rejected above), so raw
                    // bytes equal the lossy match bytes.
                    if let Some(name) = relative.file_name() {
                        for window in name.as_encoded_bytes().windows(3) {
                            bloom.insert(trigram_key(window));
                        }
                    }
                    files.push(SnapshotFile {
                        path: relative,
                        lines,
                        bloom,
                    });
                }
            }
        }
        Ok(Snapshot {
            files,
            skips,
            bytes,
        })
    }

    /// Scan delegation with this engine's identity stamp: the
    /// outcome was served by the indexed engine (which chose not to
    /// use its snapshot), so it carries the indexed backend id.
    fn fallback_search(
        &self,
        root: &RepoRoot,
        query: &FinderQuery,
    ) -> Result<SearchOutcome, FinderError> {
        let mut outcome = self.scan.search(root, query)?;
        outcome.backend = Self::backend_id();
        Ok(outcome)
    }

    /// Serves one page from a snapshot: scope filter, trigram
    /// prefilter, exact verify via the shared collector, then the
    /// shared rank/page/mint tail. Skips replay scope-filtered,
    /// exactly like the scan path reports them.
    fn query_snapshot(
        &self,
        provenance: RepoProvenance,
        snapshot: &Snapshot,
        query: &FinderQuery,
    ) -> Result<SearchOutcome, FinderError> {
        let terms: Vec<&str> = query.text().split_whitespace().collect();
        let mut candidates = Vec::new();
        for file in &snapshot.files {
            if let Some(scope) = query.scope() {
                if !file.path.starts_with(scope) {
                    continue;
                }
            }
            if !file_may_match(&file.bloom, &terms) {
                continue;
            }
            candidates.extend(collect_candidates(&file.path, &file.lines, &terms));
        }
        let skipped = replay_skips(&snapshot.skips, query.scope());
        let mut outcome = self
            .scan
            .finish_search(provenance, candidates, skipped, query)?;
        outcome.backend = Self::backend_id();
        Ok(outcome)
    }
}

impl RepoSearchPort for IndexedFinder {
    fn backend(&self) -> BackendId {
        Self::backend_id()
    }

    fn search(&self, root: &RepoRoot, query: &FinderQuery) -> Result<SearchOutcome, FinderError> {
        let provenance = self.revision.resolve(root)?;
        let root_path = root.as_path();
        if root_path.symlink_metadata().is_err() {
            return Err(FinderError::NotARepository {
                root: root_path.to_path_buf(),
            });
        }
        // Dirty worktrees bypass the cache: the snapshot is only
        // valid for the exact committed state it was built from.
        if provenance.dirty {
            return self.fallback_search(root, query);
        }
        let key = (
            root_path.to_path_buf(),
            provenance.revision.as_str().to_string(),
        );
        if let Some(cached) = self.cache.read().ok().and_then(|guard| {
            guard
                .get(&key)
                .map(|snapshot| self.query_snapshot(provenance.clone(), snapshot, query))
        }) {
            return cached;
        }
        // A build failure (file vanished mid-walk, unreadable file)
        // falls back to scan like the size cap does: the scan only
        // touches in-scope files, so out-of-scope breakage must not
        // fail the query.
        let snapshot = match self.build_snapshot(root_path) {
            Ok(snapshot) => snapshot,
            Err(_) => return self.fallback_search(root, query),
        };
        if snapshot.bytes > self.max_snapshot_bytes {
            return self.fallback_search(root, query);
        }
        let outcome = self.query_snapshot(provenance, &snapshot, query)?;
        if let Ok(mut guard) = self.cache.write() {
            if guard.len() >= MAX_CACHED_SNAPSHOTS {
                guard.clear();
            }
            guard.insert(key, snapshot);
        }
        Ok(outcome)
    }
}

impl RepoReadPort for IndexedFinder {
    fn read_span(
        &self,
        root: &RepoRoot,
        evidence: &repo_finder::EvidenceParts,
    ) -> Result<repo_finder::SpanContent, FinderError> {
        self.scan.read_span(root, evidence)
    }

    fn read_window(
        &self,
        root: &RepoRoot,
        evidence: &repo_finder::EvidenceParts,
        before: u32,
        after: u32,
    ) -> Result<repo_finder::WindowContent, FinderError> {
        self.scan.read_window(root, evidence, before, after)
    }
}

/// True when the file may hold a line matching some term. Short
/// terms (< 3 bytes) constrain nothing: if any term is short, no
/// file is excludable and every file passes. Otherwise a file
/// passes when some term's every trigram is in the Bloom — a
/// necessary byte condition for `contains`, never a sufficient one;
/// the collector verifies exactly, correcting Bloom false positives.
fn file_may_match(bloom: &TrigramBloom, terms: &[&str]) -> bool {
    if terms.iter().any(|term| term.len() < 3) {
        return true;
    }
    terms.iter().any(|term| {
        term.as_bytes()
            .windows(3)
            .all(|window| bloom.may_contain(trigram_key(window)))
    })
}

/// Bloom filter over a file's byte trigrams: absence is certain
/// (the file cannot match), presence only admits to exact
/// verification. Sized ~8 bits per file byte; two hashes keep build
/// and query cheap while the verify step corrects false positives.
struct TrigramBloom {
    bits: Vec<u64>,
    mask: u32,
}

impl TrigramBloom {
    fn new(file_bytes: usize) -> Self {
        let wanted = file_bytes.saturating_mul(8).clamp(1024, 1 << 23);
        let bits_count = wanted.next_power_of_two();
        Self {
            bits: vec![0u64; bits_count / 64],
            mask: (bits_count - 1) as u32,
        }
    }

    fn insert(&mut self, trigram: u32) {
        for bit in self.positions(trigram) {
            self.bits[bit as usize / 64] |= 1u64 << (bit % 64);
        }
    }

    fn may_contain(&self, trigram: u32) -> bool {
        self.positions(trigram)
            .iter()
            .all(|bit| self.bits[*bit as usize / 64] & (1u64 << (bit % 64)) != 0)
    }

    fn positions(&self, trigram: u32) -> [u32; 2] {
        [
            trigram.wrapping_mul(0x9E37_79B1) & self.mask,
            trigram.wrapping_mul(0x85EB_CA77) & self.mask,
        ]
    }
}

/// Packs three bytes into one `u32` key (little-endian).
fn trigram_key(window: &[u8]) -> u32 {
    window[0] as u32 | ((window[1] as u32) << 8) | ((window[2] as u32) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use repo_finder::SkipReason;
    use std::path::Path;

    fn skip_for(path: &str) -> SkippedSource {
        SkippedSource {
            path: PathBuf::from(path),
            reason: SkipReason::Binary,
        }
    }

    #[test]
    fn replay_preserves_walk_order_and_scope() {
        let skips = vec![
            OrderedSkip {
                path: None,
                skip: skip_for("<unrepresentable-path>"),
            },
            OrderedSkip {
                path: Some(PathBuf::from("src/big.bin")),
                skip: skip_for("src/big.bin"),
            },
            OrderedSkip {
                path: Some(PathBuf::from("docs/huge.bin")),
                skip: skip_for("docs/huge.bin"),
            },
            OrderedSkip {
                path: None,
                skip: skip_for("<unrepresentable-path>"),
            },
        ];
        let unscoped = replay_skips(&skips, None);
        assert_eq!(unscoped.len(), 4);
        let scoped = replay_skips(&skips, Some(Path::new("src")));
        let paths: Vec<&str> = scoped
            .iter()
            .map(|skip| skip.path.to_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                "<unrepresentable-path>",
                "src/big.bin",
                "<unrepresentable-path>"
            ],
            "globals replay in walk order around the in-scope skip"
        );
    }
}
