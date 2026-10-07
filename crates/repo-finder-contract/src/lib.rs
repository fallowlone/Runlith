//! Shared `repo-finder` port contract suite (TASK-0147, G9).
//!
//! The canonical repository retrieval seam is [`repo_finder::port`]
//! (ADR-0010): bounded search, exact span reopen, and revision
//! binding behind replaceable engines. Every adapter implementing
//! the seam — the lexical engine today, a symbol index or vector
//! engine tomorrow — must satisfy this same contract.
//!
//! Engine implementations consume this contract; they do not define
//! it. Each engine crate invokes [`run_finder_search_contract`] (and,
//! for revision backends, [`run_finder_revision_contract`]) from its
//! own test target against a hermetic [`ContractRepo`] fixture —
//! never against the working repository, never over a network.
//!
//! What the contract asserts (only behavior the seam promises):
//!
//! ```text
//! bounded   → hits within limit, total never understated,
//!             continuation exactly when hits remain, skips explicit
//! determin. → same query twice yields the identical hit sequence
//! ignore    → ignore files honored (no hits under ignored paths)
//! confine   → no hit outside the root; unknown terms yield zero hits
//! scope     → an explicit scope restricts every hit to it
//! skips     → reported skips are well-formed; binaries never surface
//!             as content hits; escaping symlinks never surface at all
//! explain   → every hit carries matched terms in its excerpt (or path)
//!             plus a coherent field; handle and path agree
//! reopen    → exact current bytes plus content hash and binding;
//!             missing files are UnknownEvidence, never invented bytes
//! window    → verified core plus adjacent context from one read;
//!             core hash binds the core bytes, context never exceeds
//!             the request and clamps at file edges; over-bound
//!             context is InvalidInput, missing files UnknownEvidence
//! revision  → non-repositories are NotARepository; bindings on a real
//!             repository are stable, non-blank, and bounded
//! ```
//!
//! Deliberately NOT in the contract (not promised by the seam):
//!
//! ```text
//! ranking   → scores are adapter-local; only determinism and the
//!             matched-terms explanation are pinned, never the formula
//! counts    → exact hit counts are engine behavior; the suite pins
//!             bounds and properties, never a golden hit list
//! excerpt   → styling is adapter-local within the generous bound
//! skip-full → whether a traversal reports every skipped file is
//!             engine thoroughness (pinned per adapter); the suite pins
//!             well-formedness of reported skips and the never-surface
//!             rules (binaries, escaping symlinks)
//! redaction → the seam passes bytes; secret filtering is a
//!             service-stage seam, not asserted here
//! staleness → drift detection compares handle against outcome and
//!             lives in the service; the suite pins honest binding
//! latency   → no timing promise; the TASK-0147 benchmark measures it
//! ```
//!
//! Adding a second engine validates mechanically:
//!
//! ```text
//! finder-symbols implements RepoSearchPort (+ RepoReadPort)
//!   → its tests dev-depend on this crate
//!   → they build the engine against the fixture
//!   → they call run_finder_search_contract
//!   → composition root selects the implementation
//! ```
//!
//! No service, IPC, or CLI change is involved.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use repo_finder::{
    content_hash, EvidenceId, EvidenceParts, FinderError, FinderQuery, HitSpan, MatchField,
    RepoReadPort, RepoRevisionPort, RepoRoot, RepoSearchPort, Revision, SearchOutcome, SkipReason,
};

/// Generous contract guardrail for excerpts (chars). Adapters stay
/// tighter; the contract catches dumps, not exact limits.
pub const MAX_CONTRACT_EXCERPT_CHARS: usize = 4096;

/// Generous contract guardrail for failure detail (chars).
pub const MAX_CONTRACT_DETAIL_CHARS: usize = 2048;

/// Generous contract guardrail for evidence identifiers (bytes).
pub const MAX_CONTRACT_ID_BYTES: usize = 2048;

/// Fixture term with text hits inside the root (plus one ignored and
/// one binary occurrence that must never surface as content hits).
pub const NEEDLE_ALPHA: &str = "needle_alpha_xyz";

/// Fixture term living only outside the root (must yield zero hits).
pub const NEEDLE_BETA: &str = "needle_beta_xyz";

/// Hermetic fixture repository. Built fresh per contract run under
/// the system temp dir; removed on drop. Never the working repo.
pub struct ContractRepo {
    base: PathBuf,
    root: RepoRoot,
    has_symlink: bool,
}

impl ContractRepo {
    /// Builds the fixture layout:
    ///
    /// ```text
    /// <base>/repo/src/main.rs      fn main, NEEDLE_ALPHA line 2
    /// <base>/repo/src/lib.rs       helper, no needle
    /// <base>/repo/notes.md         NEEDLE_ALPHA line 3
    /// <base>/repo/.gitignore       ignored/
    /// <base>/repo/ignored/secret.rs NEEDLE_ALPHA (must stay hidden)
    /// <base>/repo/bin/blob         NEEDLE_ALPHA bytes + NUL (binary)
    /// <base>/repo/link_outside.rs  symlink → ../outside.rs (unix)
    /// <base>/outside.rs            NEEDLE_BETA (outside the root)
    /// ```
    pub fn build() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "rf-contract-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let root_dir = base.join("repo");
        for dir in [
            &root_dir,
            &root_dir.join("src"),
            &root_dir.join("ignored"),
            &root_dir.join("bin"),
        ] {
            std::fs::create_dir_all(dir).expect("fixture dirs must build");
        }
        std::fs::write(
            root_dir.join("src/main.rs"),
            format!("fn main() {{\n    println!(\"{NEEDLE_ALPHA}\");\n}}\n"),
        )
        .expect("fixture main.rs must build");
        std::fs::write(
            root_dir.join("src/lib.rs"),
            "pub fn helper() -> u32 {\n    42\n}\n",
        )
        .expect("fixture lib.rs must build");
        std::fs::write(
            root_dir.join("notes.md"),
            format!("# notes\n\n{NEEDLE_ALPHA} appears here too.\n"),
        )
        .expect("fixture notes.md must build");
        std::fs::write(root_dir.join(".gitignore"), "ignored/\n")
            .expect("fixture .gitignore must build");
        std::fs::write(
            root_dir.join("ignored/secret.rs"),
            format!("// {NEEDLE_ALPHA} must stay hidden\n"),
        )
        .expect("fixture secret.rs must build");
        let mut blob = vec![0u8, 1u8];
        blob.extend_from_slice(NEEDLE_ALPHA.as_bytes());
        blob.extend_from_slice(&[0xffu8, 0xfeu8]);
        std::fs::write(root_dir.join("bin/blob"), blob).expect("fixture blob must build");
        std::fs::write(
            base.join("outside.rs"),
            format!("// {NEEDLE_BETA} lives outside the root\n"),
        )
        .expect("fixture outside.rs must build");
        let has_symlink = Self::link_outside(&root_dir);
        let root = RepoRoot::new(root_dir).expect("fixture root is absolute");
        Self {
            base,
            root,
            has_symlink,
        }
    }

    /// The search root adapters are bound to.
    pub fn root(&self) -> &RepoRoot {
        &self.root
    }

    /// The fixture base (parent of the root; holds outside-only files).
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Whether the escaping-symlink fixture was created (unix only).
    pub fn has_symlink(&self) -> bool {
        self.has_symlink
    }

    /// A fresh empty directory for non-repository checks.
    pub fn plain_dir(&self) -> PathBuf {
        let dir = self.base.join("plain");
        std::fs::create_dir_all(&dir).expect("plain dir must build");
        dir
    }

    #[cfg(unix)]
    fn link_outside(root_dir: &Path) -> bool {
        std::os::unix::fs::symlink("../outside.rs", root_dir.join("link_outside.rs")).is_ok()
    }

    #[cfg(not(unix))]
    fn link_outside(_root_dir: &Path) -> bool {
        false
    }
}

impl Drop for ContractRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Runs the full search/read contract against one engine.
pub fn run_finder_search_contract(search: &dyn RepoSearchPort, read: &dyn RepoReadPort) {
    let repo = ContractRepo::build();
    search_is_bounded_and_explicit(search, &repo);
    search_is_deterministic(search, &repo);
    search_honors_ignore_files(search, &repo);
    search_confines_to_root(search, &repo);
    search_honors_scope(search, &repo);
    search_reports_skips_well_formed(search, &repo);
    search_explains_every_hit(search, &repo);
    search_keeps_handle_and_path_consistent(search, &repo);
    search_never_surfaces_escaping_symlinks(search, &repo);
    read_span_returns_exact_bytes(read, &repo);
    read_span_maps_missing_to_unknown(read, &repo);
    read_window_returns_verified_core_with_context(read, &repo);
    read_window_maps_missing_to_unknown(read, &repo);
    read_window_rejects_over_bound_context(read, &repo);
}

/// Runs the revision contract. `repo` is a real repository built by
/// the adapter under test (its own fixture owns it); the
/// non-repository check always runs against a fresh plain dir.
pub fn run_finder_revision_contract(rev: &dyn RepoRevisionPort, repo: Option<&RepoRoot>) {
    let fixture = ContractRepo::build();
    revision_rejects_non_repository(rev, &fixture);
    if let Some(repo) = repo {
        revision_is_stable_and_bounded(rev, repo);
    }
}

/// One page stays within its limit, the total never understates
/// the page, and a remainder always carries a way to continue.
/// A total beyond the page without a continuation token is the
/// silent truncation the seam forbids.
pub fn search_is_bounded_and_explicit(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let limit = 1;
    let query = FinderQuery::new(NEEDLE_ALPHA, limit).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("bounded probe must succeed");
    assert!(
        outcome.hits.hits.len() <= limit,
        "hits must stay within the query limit, got {} for limit {limit}",
        outcome.hits.hits.len()
    );
    assert!(
        outcome.hits.total_hits >= outcome.hits.hits.len(),
        "total must never understate the returned page"
    );
    match (
        outcome.hits.total_hits > outcome.hits.hits.len(),
        &outcome.hits.continuation,
    ) {
        (true, None) => panic!(
            "total {} beyond {} returned without a continuation token",
            outcome.hits.total_hits,
            outcome.hits.hits.len()
        ),
        (false, Some(_)) => panic!("continuation token without a remainder"),
        _ => {}
    }
    assert!(
        !outcome.backend.name.trim().is_empty(),
        "backend name must be non-blank"
    );
    assert!(
        !outcome.backend.version.trim().is_empty(),
        "backend version must be non-blank"
    );
    assert_eq!(
        outcome.provenance.root,
        *repo.root(),
        "outcome must bind the queried root"
    );
    for hit in &outcome.hits.hits {
        assert!(
            hit.evidence_id.as_str().len() <= MAX_CONTRACT_ID_BYTES,
            "evidence id must be a bounded handle"
        );
        assert!(!hit.excerpt.trim().is_empty(), "excerpt must be non-blank");
        assert!(
            hit.excerpt.chars().count() <= MAX_CONTRACT_EXCERPT_CHARS,
            "excerpt must be bounded"
        );
    }
}

/// The same query twice yields the identical hit sequence. Ranking
/// formulas stay adapter-local; reproducibility does not.
pub fn search_is_deterministic(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).expect("contract query must build");
    let first = search
        .search(repo.root(), &query)
        .expect("determinism probe must succeed");
    let second = search
        .search(repo.root(), &query)
        .expect("determinism probe must succeed");
    let ids = |outcome: &SearchOutcome| {
        outcome
            .hits
            .hits
            .iter()
            .map(|hit| hit.evidence_id.as_str().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&first),
        ids(&second),
        "same query must yield the identical hit sequence"
    );
}

/// Ignore files are honored: the fixture ignores `ignored/`, so no
/// hit may come from under it however the term matches there.
pub fn search_honors_ignore_files(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("ignore probe must succeed");
    for hit in &outcome.hits.hits {
        assert!(
            !hit.path.starts_with("ignored"),
            "ignore files must be honored, got hit at {}",
            hit.path.display()
        );
    }
}

/// The root confines the search: a term living only outside yields
/// zero hits, and every returned hit names a file that exists under
/// the root. (Path escape itself is blocked by `Hit::new`, so the
/// seam pins the observable behavior: zero outside hits, no phantoms.)
pub fn search_confines_to_root(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_BETA, 50).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("confinement probe must succeed");
    assert!(
        outcome.hits.hits.is_empty(),
        "outside-only term must yield zero hits, got {}",
        outcome.hits.hits.len()
    );
    assert_eq!(
        outcome.hits.total_hits, 0,
        "outside-only term must total zero hits"
    );
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("confinement probe must succeed");
    for hit in &outcome.hits.hits {
        let full = repo.root().as_path().join(&hit.path);
        assert!(
            full.symlink_metadata().is_ok(),
            "hit file must exist under the root: {}",
            hit.path.display()
        );
    }
}

/// An explicit scope restricts every hit to it.
pub fn search_honors_scope(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_ALPHA, 50)
        .expect("contract query must build")
        .with_scope("src")
        .expect("contract scope must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("scope probe must succeed");
    for hit in &outcome.hits.hits {
        assert!(
            hit.path.starts_with("src"),
            "scoped search must stay in scope, got {}",
            hit.path.display()
        );
    }
}

/// Reported skips are well-formed, and two never-surface rules hold:
/// binaries never surface as content hits, and the fixture blob —
/// which does contain the term bytes — proves the negative.
pub fn search_reports_skips_well_formed(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("skips probe must succeed");
    for hit in &outcome.hits.hits {
        assert_ne!(
            hit.path,
            Path::new("bin/blob"),
            "binary files must never surface as content hits"
        );
    }
    for skipped in &outcome.hits.skipped {
        assert!(
            !skipped.path.as_os_str().is_empty() && !skipped.path.is_absolute(),
            "skipped path must be a confined relative path"
        );
        match &skipped.reason {
            SkipReason::Binary => {}
            SkipReason::TooLarge { bytes } => {
                assert!(*bytes > 0, "too-large size must be positive");
            }
            SkipReason::Inaccessible { detail } => {
                assert!(!detail.trim().is_empty(), "skip detail must be non-blank");
                assert!(
                    detail.chars().count() <= MAX_CONTRACT_DETAIL_CHARS,
                    "skip detail must be bounded"
                );
            }
            SkipReason::Denied { rule } => {
                assert!(!rule.trim().is_empty(), "deny rule must name its rule");
                assert!(
                    rule.chars().count() <= MAX_CONTRACT_DETAIL_CHARS,
                    "deny rule must be bounded"
                );
            }
        }
    }
}

/// Every hit carries its explanation: non-blank matched terms, each
/// of which appears in the excerpt — or, for path matches, in the
/// path. The terms are the audit trail of the selection.
pub fn search_explains_every_hit(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("explanation probe must succeed");
    for hit in &outcome.hits.hits {
        assert!(
            !hit.reason.matched_terms.is_empty(),
            "every hit must carry matched terms"
        );
        let excerpt = hit.excerpt.to_lowercase();
        let path = hit.path.to_string_lossy().to_lowercase();
        for term in &hit.reason.matched_terms {
            assert!(!term.trim().is_empty(), "matched terms must be non-blank");
            let term = term.to_lowercase();
            let in_excerpt = excerpt.contains(&term);
            let in_path = hit.reason.field == MatchField::Path && path.contains(&term);
            assert!(
                in_excerpt || in_path,
                "matched term {term:?} must appear in the excerpt or path"
            );
        }
    }
}

/// The handle and the hit agree: the evidence id parses, and its
/// parts name the same path and span the hit claims. A reopened
/// handle must address the evidence it was minted for.
pub fn search_keeps_handle_and_path_consistent(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).expect("contract query must build");
    let outcome = search
        .search(repo.root(), &query)
        .expect("consistency probe must succeed");
    for hit in &outcome.hits.hits {
        let (parts, _) = EvidenceId::parse(hit.evidence_id.as_str())
            .expect("evidence id must parse back to its parts");
        assert_eq!(parts.path, hit.path, "handle path must match the hit path");
        assert_eq!(parts.span, hit.span, "handle span must match the hit span");
    }
}

/// Escaping symlinks never surface as hits. Unix-only fixture: where
/// the platform cannot create the symlink, the check passes
/// vacuously and says so.
pub fn search_never_surfaces_escaping_symlinks(search: &dyn RepoSearchPort, repo: &ContractRepo) {
    if !repo.has_symlink() {
        return;
    }
    for term in [NEEDLE_ALPHA, NEEDLE_BETA] {
        let query = FinderQuery::new(term, 50).expect("contract query must build");
        let outcome = search
            .search(repo.root(), &query)
            .expect("symlink probe must succeed");
        for hit in &outcome.hits.hits {
            assert_ne!(
                hit.path,
                Path::new("link_outside.rs"),
                "escaping symlinks must never surface as hits"
            );
        }
    }
}

/// Reopen returns the exact current bytes of the span: the lines
/// match the file byte for byte, the reported hash binds those very
/// bytes, the binding echoes the queried root, and a second call is
/// identical. Anything else is invented evidence.
pub fn read_span_returns_exact_bytes(read: &dyn RepoReadPort, repo: &ContractRepo) {
    let parts = EvidenceParts::new(
        Revision::new("contract").expect("contract revision must build"),
        PathBuf::from("notes.md"),
        HitSpan::new(3, 3).expect("contract span must build"),
    )
    .expect("contract parts must build");
    let expected = format!("{NEEDLE_ALPHA} appears here too.");
    let first = read
        .read_span(repo.root(), &parts)
        .expect("reopen probe must succeed");
    assert_eq!(
        first.lines,
        vec![expected],
        "reopened bytes must match the file exactly"
    );
    assert_eq!(
        first.content_hash,
        content_hash(&first.lines),
        "reported hash must bind the returned bytes"
    );
    assert_eq!(
        first.provenance.root,
        *repo.root(),
        "reopen must bind the queried root"
    );
    let second = read
        .read_span(repo.root(), &parts)
        .expect("reopen probe must succeed");
    assert_eq!(second.lines, first.lines, "reopened bytes must be stable");
    assert_eq!(
        second.content_hash, first.content_hash,
        "reopened hash must be stable"
    );
}

/// Missing files fail canonically: [`FinderError::UnknownEvidence`],
/// never `Ok` with invented or neighboring content.
pub fn read_span_maps_missing_to_unknown(read: &dyn RepoReadPort, repo: &ContractRepo) {
    let parts = EvidenceParts::new(
        Revision::new("contract").expect("contract revision must build"),
        PathBuf::from("missing.rs"),
        HitSpan::new(1, 1).expect("contract span must build"),
    )
    .expect("contract parts must build");
    let error = read
        .read_span(repo.root(), &parts)
        .expect_err("missing files must fail");
    assert!(
        matches!(error, FinderError::UnknownEvidence { .. }),
        "missing files must be UnknownEvidence, got {error:?}"
    );
}

/// Window reopen returns a verified core plus adjacent context from
/// one read: the core lines match the file byte for byte, the core
/// hash binds those very bytes, the binding echoes the queried
/// root, context never exceeds the request, and context clamps at
/// file edges instead of inventing lines. The fixture probe asks
/// for more trailing context than the file holds.
pub fn read_window_returns_verified_core_with_context(
    read: &dyn RepoReadPort,
    repo: &ContractRepo,
) {
    let parts = EvidenceParts::new(
        Revision::new("contract").expect("contract revision must build"),
        PathBuf::from("notes.md"),
        HitSpan::new(3, 3).expect("contract span must build"),
    )
    .expect("contract parts must build");
    let window = read
        .read_window(repo.root(), &parts, 2, 5)
        .expect("window probe must succeed");
    let expected_core = format!("{NEEDLE_ALPHA} appears here too.");
    assert_eq!(
        window.core.lines,
        vec![expected_core],
        "window core must match the file exactly"
    );
    assert_eq!(
        window.core.content_hash,
        content_hash(&window.core.lines),
        "core hash must bind the core bytes"
    );
    assert_eq!(
        window.core.provenance.root,
        *repo.root(),
        "window must bind the queried root"
    );
    assert_eq!(
        window.before,
        vec!["# notes".to_string(), String::new()],
        "leading context must be the exact preceding lines"
    );
    assert!(
        window.before.len() <= 2,
        "leading context must never exceed the request"
    );
    assert!(
        window.after.is_empty(),
        "trailing context must clamp at EOF, got {:?}",
        window.after
    );
}

/// Context beyond the seam bound fails canonically:
/// [`FinderError::InvalidInput`], never a silent clamp and never a
/// window beyond the bound.
pub fn read_window_rejects_over_bound_context(read: &dyn RepoReadPort, repo: &ContractRepo) {
    let parts = EvidenceParts::new(
        Revision::new("contract").expect("contract revision must build"),
        PathBuf::from("notes.md"),
        HitSpan::new(3, 3).expect("contract span must build"),
    )
    .expect("contract parts must build");
    for (before, after) in [
        (repo_finder::MAX_WINDOW_CONTEXT + 1, 0),
        (0, repo_finder::MAX_WINDOW_CONTEXT + 1),
    ] {
        let error = read
            .read_window(repo.root(), &parts, before, after)
            .expect_err("over-bound context must fail");
        assert!(
            matches!(error, FinderError::InvalidInput(_)),
            "over-bound context must be InvalidInput, got {error:?}"
        );
    }
}

/// Window reopen on missing files fails canonically:
/// [`FinderError::UnknownEvidence`], never `Ok` with invented or
/// neighboring content.
pub fn read_window_maps_missing_to_unknown(read: &dyn RepoReadPort, repo: &ContractRepo) {
    let parts = EvidenceParts::new(
        Revision::new("contract").expect("contract revision must build"),
        PathBuf::from("missing.rs"),
        HitSpan::new(1, 1).expect("contract span must build"),
    )
    .expect("contract parts must build");
    let error = read
        .read_window(repo.root(), &parts, 2, 2)
        .expect_err("missing files must fail");
    assert!(
        matches!(error, FinderError::UnknownEvidence { .. }),
        "missing files must be UnknownEvidence, got {error:?}"
    );
}

/// Roots the backend cannot bind fail canonically:
/// [`FinderError::NotARepository`], never an invented binding.
pub fn revision_rejects_non_repository(rev: &dyn RepoRevisionPort, fixture: &ContractRepo) {
    let root = RepoRoot::new(fixture.plain_dir()).expect("plain dir is absolute");
    let error = rev.resolve(&root).expect_err("non-repositories must fail");
    assert!(
        matches!(error, FinderError::NotARepository { .. }),
        "non-repositories must be NotARepository, got {error:?}"
    );
}

/// Bindings on a real repository are stable, non-blank, bounded, and
/// echo the queried root.
pub fn revision_is_stable_and_bounded(rev: &dyn RepoRevisionPort, repo: &RepoRoot) {
    let first = rev.resolve(repo).expect("revision probe must succeed");
    let second = rev.resolve(repo).expect("revision probe must succeed");
    assert_eq!(
        first.revision, second.revision,
        "revision binding must be stable"
    );
    assert!(
        !first.revision.as_str().trim().is_empty(),
        "revision must be non-blank"
    );
    assert!(
        first.revision.as_str().chars().count() <= repo_finder::MAX_REVISION_CHARS,
        "revision must be bounded"
    );
    assert_eq!(first.root, *repo, "binding must echo the queried root");
}

#[cfg(test)]
mod tests {
    use super::*;
    use repo_finder::{
        content_hash, BackendId, BoundedHits, Hit, HitReason, RepoProvenance, SpanContent,
        WindowContent,
    };
    use std::sync::atomic::AtomicUsize;

    fn stub_backend() -> BackendId {
        BackendId::new("stub", "0").unwrap()
    }

    fn stub_provenance(root: &RepoRoot) -> RepoProvenance {
        RepoProvenance {
            root: root.clone(),
            revision: Revision::new("stubrev").unwrap(),
            dirty: false,
        }
    }

    fn stub_hit(path: &str, excerpt: &str) -> Hit {
        let span = HitSpan::new(1, 1).unwrap();
        let parts =
            EvidenceParts::new(Revision::new("stubrev").unwrap(), PathBuf::from(path), span)
                .unwrap();
        let hash = content_hash(&[excerpt.to_string()]);
        Hit::new(
            EvidenceId::mint(&parts, &hash).unwrap(),
            PathBuf::from(path),
            span,
            excerpt.to_string(),
            HitReason {
                matched_terms: vec![NEEDLE_ALPHA.to_string()],
                field: MatchField::Content,
                rank_score: 1,
            },
        )
        .unwrap()
    }

    fn caught(f: impl FnOnce()) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err()
    }

    #[test]
    fn fixture_builds_expected_layout() {
        let repo = ContractRepo::build();
        let main = repo.root().as_path().join("src/main.rs");
        let body = std::fs::read_to_string(&main).unwrap();
        assert!(body.contains(NEEDLE_ALPHA));
        assert!(repo.root().as_path().join(".gitignore").exists());
        assert!(repo.root().as_path().join("bin/blob").exists());
        assert!(repo.base().join("outside.rs").exists());
        // Fixture cleans up after itself.
        let base = repo.base().to_path_buf();
        drop(repo);
        assert!(!base.exists());
    }

    struct UnboundedSearch;

    impl RepoSearchPort for UnboundedSearch {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            let mut hits = Vec::new();
            for index in 0..=query.limit() {
                hits.push(stub_hit(&format!("f{index}.rs"), NEEDLE_ALPHA));
            }
            Ok(SearchOutcome {
                provenance: stub_provenance(root),
                backend: stub_backend(),
                hits: BoundedHits {
                    total_hits: hits.len(),
                    hits,
                    skipped: Vec::new(),
                    continuation: None,
                },
            })
        }
    }

    #[test]
    fn suite_catches_unbounded_hits() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_is_bounded_and_explicit(&UnboundedSearch, &repo)),
            "suite must reject hits beyond the limit"
        );
    }

    struct SilentRemainderSearch;

    impl RepoSearchPort for SilentRemainderSearch {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            // Claims more hits than returned but offers no way to
            // continue: the silent truncation the seam forbids.
            Ok(SearchOutcome {
                provenance: stub_provenance(root),
                backend: stub_backend(),
                hits: BoundedHits {
                    total_hits: 5,
                    hits: vec![stub_hit("a.rs", NEEDLE_ALPHA)],
                    skipped: Vec::new(),
                    continuation: None,
                },
            })
        }
    }

    #[test]
    fn suite_catches_silent_remainder() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_is_bounded_and_explicit(&SilentRemainderSearch, &repo)),
            "suite must reject total beyond returned without continuation"
        );
    }

    struct NondeterministicSearch {
        calls: AtomicUsize,
    }

    impl RepoSearchPort for NondeterministicSearch {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            let calls = self.calls.fetch_add(1, Ordering::Relaxed);
            let mut hits = vec![
                stub_hit("a.rs", NEEDLE_ALPHA),
                stub_hit("b.rs", NEEDLE_ALPHA),
            ];
            if calls % 2 == 1 {
                hits.swap(0, 1);
            }
            Ok(SearchOutcome {
                provenance: stub_provenance(root),
                backend: stub_backend(),
                hits: BoundedHits {
                    total_hits: hits.len(),
                    hits,
                    skipped: Vec::new(),
                    continuation: None,
                },
            })
        }
    }

    #[test]
    fn suite_catches_nondeterministic_order() {
        let repo = ContractRepo::build();
        let search = NondeterministicSearch {
            calls: AtomicUsize::new(0),
        };
        assert!(
            caught(|| search_is_deterministic(&search, &repo)),
            "suite must reject order that changes between calls"
        );
    }

    fn violating_outcome(root: &RepoRoot, hits: Vec<Hit>) -> SearchOutcome {
        let total_hits = hits.len();
        SearchOutcome {
            provenance: stub_provenance(root),
            backend: stub_backend(),
            hits: BoundedHits {
                total_hits,
                hits,
                skipped: Vec::new(),
                continuation: None,
            },
        }
    }

    struct IgnoreViolator;

    impl RepoSearchPort for IgnoreViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            Ok(violating_outcome(
                root,
                vec![stub_hit("ignored/secret.rs", NEEDLE_ALPHA)],
            ))
        }
    }

    #[test]
    fn suite_catches_ignored_path_hit() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_honors_ignore_files(&IgnoreViolator, &repo)),
            "suite must reject hits under ignored paths"
        );
    }

    struct ConfineViolator;

    impl RepoSearchPort for ConfineViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            // NEEDLE_BETA lives outside the root; any hit for it is a
            // confinement failure (path escape itself is already
            // blocked by `Hit::new`, so the seam pins zero hits).
            Ok(violating_outcome(
                root,
                vec![stub_hit("src/main.rs", NEEDLE_BETA)],
            ))
        }
    }

    #[test]
    fn suite_catches_outside_term_hit() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_confines_to_root(&ConfineViolator, &repo)),
            "suite must reject hits for outside-only terms"
        );
    }

    struct ScopeViolator;

    impl RepoSearchPort for ScopeViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            Ok(violating_outcome(
                root,
                vec![stub_hit("notes.md", NEEDLE_ALPHA)],
            ))
        }
    }

    #[test]
    fn suite_catches_out_of_scope_hit() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_honors_scope(&ScopeViolator, &repo)),
            "suite must reject hits outside the query scope"
        );
    }

    struct BinaryHitViolator;

    impl RepoSearchPort for BinaryHitViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            Ok(violating_outcome(
                root,
                vec![stub_hit("bin/blob", NEEDLE_ALPHA)],
            ))
        }
    }

    #[test]
    fn suite_catches_binary_content_hit() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_reports_skips_well_formed(&BinaryHitViolator, &repo)),
            "suite must reject content hits into binary files"
        );
    }

    struct MalformedSkipViolator;

    impl RepoSearchPort for MalformedSkipViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            use repo_finder::{SkipReason, SkippedSource};
            Ok(SearchOutcome {
                provenance: stub_provenance(root),
                backend: stub_backend(),
                hits: BoundedHits {
                    total_hits: 0,
                    hits: Vec::new(),
                    skipped: vec![SkippedSource {
                        path: PathBuf::from("bin/blob"),
                        reason: SkipReason::Inaccessible {
                            detail: "x".repeat(MAX_CONTRACT_DETAIL_CHARS + 1),
                        },
                    }],
                    continuation: None,
                },
            })
        }
    }

    #[test]
    fn suite_catches_malformed_skip() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_reports_skips_well_formed(&MalformedSkipViolator, &repo)),
            "suite must reject unbounded skip detail"
        );
    }

    struct UnexplainedHitViolator;

    impl RepoSearchPort for UnexplainedHitViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            let span = HitSpan::new(1, 1).unwrap();
            let parts = EvidenceParts::new(
                Revision::new("stubrev").unwrap(),
                PathBuf::from("src/main.rs"),
                span,
            )
            .unwrap();
            let hash = content_hash(&[NEEDLE_ALPHA.to_string()]);
            let hit = Hit::new(
                EvidenceId::mint(&parts, &hash).unwrap(),
                PathBuf::from("src/main.rs"),
                span,
                NEEDLE_ALPHA.to_string(),
                HitReason {
                    matched_terms: Vec::new(),
                    field: MatchField::Content,
                    rank_score: 1,
                },
            )
            .unwrap();
            Ok(violating_outcome(root, vec![hit]))
        }
    }

    #[test]
    fn suite_catches_unexplained_hit() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_explains_every_hit(&UnexplainedHitViolator, &repo)),
            "suite must reject hits without matched terms"
        );
    }

    struct InconsistentHandleViolator;

    impl RepoSearchPort for InconsistentHandleViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            // Handle minted for b.rs, hit claims a.rs.
            let span = HitSpan::new(1, 1).unwrap();
            let parts = EvidenceParts::new(
                Revision::new("stubrev").unwrap(),
                PathBuf::from("b.rs"),
                span,
            )
            .unwrap();
            let hash = content_hash(&[NEEDLE_ALPHA.to_string()]);
            let hit = Hit::new(
                EvidenceId::mint(&parts, &hash).unwrap(),
                PathBuf::from("a.rs"),
                span,
                NEEDLE_ALPHA.to_string(),
                HitReason {
                    matched_terms: vec![NEEDLE_ALPHA.to_string()],
                    field: MatchField::Content,
                    rank_score: 1,
                },
            )
            .unwrap();
            Ok(violating_outcome(root, vec![hit]))
        }
    }

    #[test]
    fn suite_catches_inconsistent_handle() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| search_keeps_handle_and_path_consistent(&InconsistentHandleViolator, &repo)),
            "suite must reject handles that disagree with the hit"
        );
    }

    struct SymlinkSurfacingViolator;

    impl RepoSearchPort for SymlinkSurfacingViolator {
        fn backend(&self) -> BackendId {
            stub_backend()
        }

        fn search(
            &self,
            root: &RepoRoot,
            _query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            Ok(violating_outcome(
                root,
                vec![stub_hit("link_outside.rs", NEEDLE_BETA)],
            ))
        }
    }

    #[test]
    fn suite_catches_escaping_symlink_hit() {
        let repo = ContractRepo::build();
        if !repo.has_symlink() {
            return;
        }
        assert!(
            caught(|| search_never_surfaces_escaping_symlinks(&SymlinkSurfacingViolator, &repo)),
            "suite must reject hits through escaping symlinks"
        );
    }

    struct InventingRead;

    impl RepoReadPort for InventingRead {
        fn read_span(
            &self,
            root: &RepoRoot,
            _evidence: &EvidenceParts,
        ) -> Result<SpanContent, FinderError> {
            let lines = vec!["invented bytes".to_string()];
            Ok(SpanContent {
                content_hash: content_hash(&lines),
                lines,
                provenance: stub_provenance(root),
            })
        }

        fn read_window(
            &self,
            root: &RepoRoot,
            _evidence: &EvidenceParts,
            _before: u32,
            _after: u32,
        ) -> Result<WindowContent, FinderError> {
            let lines = vec!["invented core".to_string()];
            Ok(WindowContent {
                core: SpanContent {
                    content_hash: content_hash(&lines),
                    lines,
                    provenance: stub_provenance(root),
                },
                before: Vec::new(),
                after: Vec::new(),
            })
        }
    }

    #[test]
    fn suite_catches_invented_bytes() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| read_span_returns_exact_bytes(&InventingRead, &repo)),
            "suite must reject reopened bytes that differ from the file"
        );
    }

    #[test]
    fn suite_catches_invented_window_core() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| read_window_returns_verified_core_with_context(&InventingRead, &repo)),
            "suite must reject window cores that differ from the file"
        );
    }

    struct OverlongContextRead;

    impl RepoReadPort for OverlongContextRead {
        fn read_span(
            &self,
            root: &RepoRoot,
            _evidence: &EvidenceParts,
        ) -> Result<SpanContent, FinderError> {
            // This stub only exists to violate the window check; the
            // span path echoes the inventing double.
            InventingRead.read_span(root, _evidence)
        }

        fn read_window(
            &self,
            root: &RepoRoot,
            _evidence: &EvidenceParts,
            before: u32,
            _after: u32,
        ) -> Result<WindowContent, FinderError> {
            // Core is honest (matches the fixture probe); the
            // violation is context beyond the request.
            let core = vec![format!("{NEEDLE_ALPHA} appears here too.")];
            let extra = (0..=before)
                .map(|index| format!("extra {index}"))
                .collect::<Vec<_>>();
            Ok(WindowContent {
                core: SpanContent {
                    content_hash: content_hash(&core),
                    lines: core,
                    provenance: stub_provenance(root),
                },
                before: extra,
                after: Vec::new(),
            })
        }
    }

    #[test]
    fn suite_catches_overlong_context() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| read_window_returns_verified_core_with_context(&OverlongContextRead, &repo)),
            "suite must reject context beyond the request"
        );
    }

    struct OverBoundAcceptingRead;

    impl RepoReadPort for OverBoundAcceptingRead {
        fn read_span(
            &self,
            root: &RepoRoot,
            evidence: &EvidenceParts,
        ) -> Result<SpanContent, FinderError> {
            // Span path echoes the honest double; only the window
            // bound is violated here.
            HonestEngine.read_span(root, evidence)
        }

        fn read_window(
            &self,
            root: &RepoRoot,
            evidence: &EvidenceParts,
            before: u32,
            after: u32,
        ) -> Result<WindowContent, FinderError> {
            // Accepts any bound: clamps silently and returns Ok where
            // the seam demands a rejection. The suite must catch the
            // missing InvalidInput.
            HonestEngine.read_window(
                root,
                evidence,
                before.min(repo_finder::MAX_WINDOW_CONTEXT),
                after.min(repo_finder::MAX_WINDOW_CONTEXT),
            )
        }
    }

    #[test]
    fn suite_catches_over_bound_acceptance() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| read_window_rejects_over_bound_context(&OverBoundAcceptingRead, &repo)),
            "suite must reject windows beyond the context bound"
        );
    }

    struct MissingAsSuccessRead;

    impl RepoReadPort for MissingAsSuccessRead {
        fn read_span(
            &self,
            root: &RepoRoot,
            _evidence: &EvidenceParts,
        ) -> Result<SpanContent, FinderError> {
            let lines = vec!["neighboring content".to_string()];
            Ok(SpanContent {
                content_hash: content_hash(&lines),
                lines,
                provenance: stub_provenance(root),
            })
        }

        fn read_window(
            &self,
            root: &RepoRoot,
            _evidence: &EvidenceParts,
            _before: u32,
            _after: u32,
        ) -> Result<WindowContent, FinderError> {
            let lines = vec!["neighboring content".to_string()];
            Ok(WindowContent {
                core: SpanContent {
                    content_hash: content_hash(&lines),
                    lines,
                    provenance: stub_provenance(root),
                },
                before: Vec::new(),
                after: Vec::new(),
            })
        }
    }

    #[test]
    fn suite_catches_missing_as_success() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| read_span_maps_missing_to_unknown(&MissingAsSuccessRead, &repo)),
            "suite must reject Ok for missing files"
        );
    }

    #[test]
    fn suite_catches_window_missing_as_success() {
        let repo = ContractRepo::build();
        assert!(
            caught(|| read_window_maps_missing_to_unknown(&MissingAsSuccessRead, &repo)),
            "suite must reject Ok windows for missing files"
        );
    }

    struct NonRepoAcceptingRev;

    impl RepoRevisionPort for NonRepoAcceptingRev {
        fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
            Ok(stub_provenance(root))
        }
    }

    #[test]
    fn suite_catches_non_repo_success() {
        let fixture = ContractRepo::build();
        assert!(
            caught(|| revision_rejects_non_repository(&NonRepoAcceptingRev, &fixture)),
            "suite must reject Ok bindings for non-repositories"
        );
    }

    struct FlappingRev {
        calls: AtomicUsize,
    }

    impl RepoRevisionPort for FlappingRev {
        fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
            let calls = self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(RepoProvenance {
                root: root.clone(),
                revision: Revision::new(format!("rev{calls}")).unwrap(),
                dirty: false,
            })
        }
    }

    #[test]
    fn suite_catches_unstable_revision() {
        let fixture = ContractRepo::build();
        let rev = FlappingRev {
            calls: AtomicUsize::new(0),
        };
        assert!(
            caught(|| revision_is_stable_and_bounded(&rev, fixture.root())),
            "suite must reject bindings that change between calls"
        );
    }

    /// Positive control: an honest miniature engine over the fixture.
    /// Proves the suite can pass; guards against suite bugs that fail
    /// every implementation. Reads real files, honors the ignore
    /// file, skips binaries and symlinks explicitly, pages honestly.
    struct HonestEngine;

    impl HonestEngine {
        fn ignore_prefixes(root: &Path) -> Vec<String> {
            let body = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
            body.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(|line| line.trim_end_matches('/').to_string())
                .collect()
        }

        fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
            let entries = std::fs::read_dir(dir).unwrap();
            let mut entries: Vec<_> = entries.map(|entry| entry.unwrap().path()).collect();
            entries.sort();
            for path in entries {
                if path.is_dir() && !path.is_symlink() {
                    Self::walk(root, &path, out);
                } else if path.is_file() || path.is_symlink() {
                    out.push(path.strip_prefix(root).unwrap().to_path_buf());
                }
            }
        }
    }

    impl RepoSearchPort for HonestEngine {
        fn backend(&self) -> BackendId {
            BackendId::new("honest-stub", "0").unwrap()
        }

        fn search(
            &self,
            root: &RepoRoot,
            query: &FinderQuery,
        ) -> Result<SearchOutcome, FinderError> {
            use repo_finder::{ContinuationToken, SkipReason, SkippedSource};
            let root_path = root.as_path();
            let prefixes = Self::ignore_prefixes(root_path);
            let mut files = Vec::new();
            Self::walk(root_path, root_path, &mut files);
            let mut matches: Vec<(PathBuf, u32, String)> = Vec::new();
            let mut skipped = Vec::new();
            for relative in files {
                if prefixes.iter().any(|prefix| relative.starts_with(prefix)) {
                    continue;
                }
                let full = root_path.join(&relative);
                if full.is_symlink() {
                    skipped.push(SkippedSource {
                        path: relative,
                        reason: SkipReason::Denied {
                            rule: "symlink".to_string(),
                        },
                    });
                    continue;
                }
                if let Some(scope) = query.scope() {
                    if !relative.starts_with(scope) {
                        continue;
                    }
                }
                let bytes = std::fs::read(&full).unwrap();
                if bytes.contains(&0) {
                    skipped.push(SkippedSource {
                        path: relative,
                        reason: SkipReason::Binary,
                    });
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                for (index, line) in text.lines().enumerate() {
                    if line.contains(query.text()) {
                        matches.push((relative.clone(), index as u32 + 1, line.to_string()));
                    }
                }
            }
            matches.sort();
            let total_hits = matches.len();
            let offset = query
                .continuation()
                .map(|token| {
                    token
                        .as_str()
                        .strip_prefix("off:")
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let page: Vec<(PathBuf, u32, String)> = matches
                .into_iter()
                .skip(offset)
                .take(query.limit())
                .collect();
            let continuation = if offset + page.len() < total_hits {
                Some(ContinuationToken::new(format!("off:{}", offset + page.len())).unwrap())
            } else {
                None
            };
            let revision = Revision::new("stubrev").unwrap();
            let mut hits = Vec::new();
            for (path, line_no, line) in page {
                let span = HitSpan::new(line_no, line_no).unwrap();
                let parts = EvidenceParts::new(revision.clone(), path.clone(), span).unwrap();
                let hash = content_hash(std::slice::from_ref(&line));
                hits.push(
                    Hit::new(
                        EvidenceId::mint(&parts, &hash).unwrap(),
                        path,
                        span,
                        line,
                        HitReason {
                            matched_terms: vec![query.text().to_string()],
                            field: MatchField::Content,
                            rank_score: 1,
                        },
                    )
                    .unwrap(),
                );
            }
            Ok(SearchOutcome {
                provenance: stub_provenance(root),
                backend: self.backend(),
                hits: BoundedHits {
                    total_hits,
                    hits,
                    skipped,
                    continuation,
                },
            })
        }
    }

    impl RepoReadPort for HonestEngine {
        fn read_span(
            &self,
            root: &RepoRoot,
            evidence: &EvidenceParts,
        ) -> Result<SpanContent, FinderError> {
            let full = root.as_path().join(&evidence.path);
            let bytes = std::fs::read(&full).map_err(|_| FinderError::UnknownEvidence {
                detail: "file cannot be read".to_string(),
            })?;
            let text = String::from_utf8_lossy(&bytes);
            let all: Vec<String> = text.lines().map(str::to_string).collect();
            let start = evidence.span.start_line as usize;
            let end = evidence.span.end_line as usize;
            if start < 1 || end > all.len() {
                return Err(FinderError::UnknownEvidence {
                    detail: "span outside file".to_string(),
                });
            }
            let lines = all[start - 1..end].to_vec();
            Ok(SpanContent {
                content_hash: content_hash(&lines),
                lines,
                provenance: stub_provenance(root),
            })
        }

        fn read_window(
            &self,
            root: &RepoRoot,
            evidence: &EvidenceParts,
            before: u32,
            after: u32,
        ) -> Result<WindowContent, FinderError> {
            if before > repo_finder::MAX_WINDOW_CONTEXT || after > repo_finder::MAX_WINDOW_CONTEXT {
                return Err(FinderError::InvalidInput(
                    "context exceeds bound".to_string(),
                ));
            }
            // One read, sliced twice: the context is adjacent to the
            // verified core, never from a second look at the file.
            let full = root.as_path().join(&evidence.path);
            let bytes = std::fs::read(&full).map_err(|_| FinderError::UnknownEvidence {
                detail: "file cannot be read".to_string(),
            })?;
            let text = String::from_utf8_lossy(&bytes);
            let all: Vec<String> = text.lines().map(str::to_string).collect();
            let start = evidence.span.start_line as usize;
            let end = evidence.span.end_line as usize;
            if start < 1 || end > all.len() {
                return Err(FinderError::UnknownEvidence {
                    detail: "span outside file".to_string(),
                });
            }
            let core_lines = all[start - 1..end].to_vec();
            let from = start.saturating_sub(before as usize + 1);
            let upto = (end + after as usize).min(all.len());
            Ok(WindowContent {
                core: SpanContent {
                    content_hash: content_hash(&core_lines),
                    lines: core_lines,
                    provenance: stub_provenance(root),
                },
                before: all[from..start - 1].to_vec(),
                after: all[end..upto].to_vec(),
            })
        }
    }

    #[test]
    fn suite_passes_honest_engine() {
        run_finder_search_contract(&HonestEngine, &HonestEngine);
    }
}
