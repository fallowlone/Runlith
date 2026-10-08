//! `finder-local` engine behavior: recall, ignore honoring, skip
//! explicitness, paging, and reopen semantics.
//!
//! The contract suite pins seam honesty (an empty engine passes it);
//! this file pins that the engine actually finds, actually honors
//! ignore files, and actually reports what it skipped. Fixture and
//! needles come from the shared contract crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use finder_local::LocalFinder;
use repo_finder::{
    content_hash, ContinuationToken, EvidenceId, EvidenceParts, FinderError, FinderQuery, HitSpan,
    MatchField, RepoProvenance, RepoReadPort, RepoRevisionPort, RepoRoot, RepoSearchPort, Revision,
    SkipReason,
};
use repo_finder_contract::{ContractRepo, NEEDLE_ALPHA};

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

fn engine() -> LocalFinder {
    LocalFinder::new(Arc::new(StubRevision))
}

#[test]
fn finds_fixture_term_in_text_files() {
    let repo = ContractRepo::build();
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).unwrap();
    let outcome = engine().search(repo.root(), &query).unwrap();
    let paths: Vec<_> = outcome
        .hits
        .hits
        .iter()
        .map(|hit| hit.path.to_string_lossy().to_string())
        .collect();
    assert!(
        paths.iter().any(|path| path == "src/main.rs"),
        "must find the main.rs occurrence, got {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path == "notes.md"),
        "must find the notes.md occurrence, got {paths:?}"
    );
}

#[test]
fn honors_gitignore_without_a_git_repo() {
    // The fixture has a .gitignore but is not a Git repository: the
    // engine must honor ignore files unconditionally (anti-#4477).
    let repo = ContractRepo::build();
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).unwrap();
    let outcome = engine().search(repo.root(), &query).unwrap();
    for hit in &outcome.hits.hits {
        assert!(
            !hit.path.starts_with("ignored"),
            "ignored paths must stay hidden, got {}",
            hit.path.display()
        );
    }
}

#[test]
fn reports_binary_skipped_explicitly() {
    let repo = ContractRepo::build();
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).unwrap();
    let outcome = engine().search(repo.root(), &query).unwrap();
    assert!(
        outcome.hits.skipped.iter().any(|skipped| {
            skipped.path == Path::new("bin/blob") && matches!(skipped.reason, SkipReason::Binary)
        }),
        "binary match must surface as an explicit Binary skip, got {:?}",
        outcome.hits.skipped
    );
}

#[test]
fn never_follows_symlinks_reports_denied() {
    let repo = ContractRepo::build();
    let link = repo.root().as_path().join("link_outside.rs");
    if !link.is_symlink() {
        return;
    }
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).unwrap();
    let outcome = engine().search(repo.root(), &query).unwrap();
    assert!(
        outcome.hits.skipped.iter().any(|skipped| {
            skipped.path == Path::new("link_outside.rs")
                && matches!(
                    &skipped.reason,
                    SkipReason::Denied { rule } if rule == "symlink"
                )
        }),
        "symlink must surface as an explicit Denied skip, got {:?}",
        outcome.hits.skipped
    );
}

#[test]
fn pages_with_continuation_to_full_list() {
    let repo = ContractRepo::build();
    let full = engine()
        .search(repo.root(), &FinderQuery::new(NEEDLE_ALPHA, 50).unwrap())
        .unwrap();
    assert!(full.hits.total_hits > 1, "fixture must offer pages");
    let mut seen: Vec<String> = Vec::new();
    let mut continuation: Option<ContinuationToken> = None;
    loop {
        let mut query = FinderQuery::new(NEEDLE_ALPHA, 1).unwrap();
        if let Some(token) = continuation {
            query = query.with_continuation(token);
        }
        let page = engine().search(repo.root(), &query).unwrap();
        assert_eq!(page.hits.total_hits, full.hits.total_hits);
        seen.extend(
            page.hits
                .hits
                .iter()
                .map(|hit| hit.evidence_id.as_str().to_string()),
        );
        match page.hits.continuation {
            Some(token) => continuation = Some(token),
            None => break,
        }
    }
    let expected: Vec<String> = full
        .hits
        .hits
        .iter()
        .map(|hit| hit.evidence_id.as_str().to_string())
        .collect();
    assert_eq!(seen, expected, "paged walk must reproduce the full list");
}

#[test]
fn rejects_malformed_continuation() {
    let repo = ContractRepo::build();
    let query = FinderQuery::new(NEEDLE_ALPHA, 10)
        .unwrap()
        .with_continuation(ContinuationToken::new("garbage").unwrap());
    let error = engine().search(repo.root(), &query).unwrap_err();
    assert!(
        matches!(error, FinderError::InvalidContinuation { .. }),
        "malformed token must be InvalidContinuation, got {error:?}"
    );
}

/// Scratch repository for edge shapes the shared fixture does not
/// carry (long lines, tiny bounds, dir symlinks).
struct ScratchRepo {
    dir: PathBuf,
    root: RepoRoot,
}

impl ScratchRepo {
    fn with_files(files: &[(&str, &[u8])]) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rf-scratch-{}-{}",
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

#[test]
fn excerpt_marks_long_lines_and_handle_binds_full_line() {
    let long_line = format!("needle {}", "x".repeat(2000));
    let scratch = ScratchRepo::with_files(&[("long.rs", long_line.as_bytes())]);
    let finder = LocalFinder::new(Arc::new(StubRevision)).with_bounds(1_048_576, 100);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = finder.search(scratch.root(), &query).unwrap();
    assert_eq!(outcome.hits.hits.len(), 1);
    let hit = &outcome.hits.hits[0];
    assert!(
        hit.excerpt.contains("reopen for the full line"),
        "long excerpt must carry its marker, got {:?}",
        hit.excerpt
    );
    assert!(
        hit.excerpt.chars().count() <= 100 + 64,
        "marked excerpt must stay within content bound + marker"
    );
    // The handle binds the FULL line: reopen verifies under the
    // stored (hash16) handle hash.
    let (parts, hash) = EvidenceId::parse(hit.evidence_id.as_str()).unwrap();
    let reopened = finder.read_span(scratch.root(), &parts).unwrap();
    assert_eq!(reopened.lines, vec![long_line]);
    assert!(
        EvidenceId::hash_matches(&hash, &reopened.content_hash),
        "reopened bytes must verify against the handle hash"
    );
}

#[test]
fn reports_too_large_explicitly() {
    let repo = ContractRepo::build();
    let finder = LocalFinder::new(Arc::new(StubRevision)).with_bounds(10, 1000);
    let query = FinderQuery::new(NEEDLE_ALPHA, 50).unwrap();
    let outcome = finder.search(repo.root(), &query).unwrap();
    assert!(
        outcome.hits.hits.is_empty(),
        "tiny bound must suppress content hits"
    );
    assert!(
        outcome.hits.skipped.iter().any(|skipped| matches!(
            skipped.reason,
            SkipReason::TooLarge { bytes } if bytes > 10
        )),
        "oversized files must surface as TooLarge skips, got {:?}",
        outcome.hits.skipped
    );
}

#[test]
fn ranks_filename_hits_first() {
    let scratch = ScratchRepo::with_files(&[
        ("needle.txt", b"no match here\n"),
        ("other.rs", b"holds needle content\n"),
    ]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    assert_eq!(outcome.hits.hits.len(), 2);
    assert_eq!(outcome.hits.hits[0].path, Path::new("needle.txt"));
    assert_eq!(outcome.hits.hits[0].reason.field, MatchField::Path);
    assert_eq!(outcome.hits.hits[1].reason.field, MatchField::Content);
}

#[test]
fn read_span_returns_exact_lines() {
    let repo = ContractRepo::build();
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("notes.md"),
        HitSpan::new(3, 3).unwrap(),
    )
    .unwrap();
    let content = engine().read_span(repo.root(), &parts).unwrap();
    assert_eq!(
        content.lines,
        vec![format!("{NEEDLE_ALPHA} appears here too.")]
    );
    assert_eq!(content.content_hash, content_hash(&content.lines));
}

#[test]
fn read_missing_is_unknown() {
    let repo = ContractRepo::build();
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("missing.rs"),
        HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let error = engine().read_span(repo.root(), &parts).unwrap_err();
    assert!(
        matches!(error, FinderError::UnknownEvidence { .. }),
        "missing file must be UnknownEvidence, got {error:?}"
    );
}

#[test]
fn read_symlink_is_not_followed() {
    let repo = ContractRepo::build();
    let link = repo.root().as_path().join("link_outside.rs");
    if !link.is_symlink() {
        return;
    }
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("link_outside.rs"),
        HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let error = engine().read_span(repo.root(), &parts).unwrap_err();
    match error {
        FinderError::UnknownEvidence { detail } => assert!(
            detail.contains("symlink"),
            "symlink reopen must say why, got {detail:?}"
        ),
        other => panic!("symlink reopen must fail closed, got {other:?}"),
    }
}

#[test]
fn read_through_dir_symlink_is_scope_escape() {
    let outside = ScratchRepo::with_files(&[("secret.txt", b"secret\n")]);
    let scratch = ScratchRepo::with_files(&[("inner.txt", b"x\n")]);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside.dir, scratch.dir.join("sub")).unwrap();
    #[cfg(not(unix))]
    return;
    // `sub/secret.txt` is a well-typed confined path whose
    // resolution escapes the root through the dir symlink.
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("sub/secret.txt"),
        HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let error = engine().read_span(scratch.root(), &parts).unwrap_err();
    assert!(
        matches!(error, FinderError::ScopeEscape { .. }),
        "escaping resolution must be ScopeEscape, got {error:?}"
    );
}

#[test]
fn read_window_returns_core_with_clamped_context() {
    let repo = ContractRepo::build();
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("notes.md"),
        HitSpan::new(3, 3).unwrap(),
    )
    .unwrap();
    let window = engine().read_window(repo.root(), &parts, 2, 5).unwrap();
    assert_eq!(
        window.core.lines,
        vec![format!("{NEEDLE_ALPHA} appears here too.")]
    );
    assert_eq!(window.core.content_hash, content_hash(&window.core.lines));
    assert_eq!(window.before, vec!["# notes".to_string(), String::new()]);
    assert!(
        window.after.is_empty(),
        "trailing context must clamp at EOF, got {:?}",
        window.after
    );
    assert_eq!(window.core.provenance.root, *repo.root());
}

#[test]
fn read_window_clamps_at_file_start() {
    let repo = ContractRepo::build();
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("notes.md"),
        HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let window = engine().read_window(repo.root(), &parts, 5, 0).unwrap();
    assert_eq!(window.core.lines, vec!["# notes".to_string()]);
    assert!(
        window.before.is_empty(),
        "leading context must clamp at BOF, got {:?}",
        window.before
    );
}

#[test]
fn read_window_rejects_over_bound_context() {
    use repo_finder::MAX_WINDOW_CONTEXT;
    let repo = ContractRepo::build();
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("notes.md"),
        HitSpan::new(3, 3).unwrap(),
    )
    .unwrap();
    for (before, after) in [(MAX_WINDOW_CONTEXT + 1, 0), (0, MAX_WINDOW_CONTEXT + 1)] {
        let error = engine()
            .read_window(repo.root(), &parts, before, after)
            .unwrap_err();
        assert!(
            matches!(error, FinderError::InvalidInput(_)),
            "over-bound context must be InvalidInput, got {error:?}"
        );
    }
}

#[test]
fn read_window_missing_is_unknown() {
    let repo = ContractRepo::build();
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("missing.rs"),
        HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let error = engine().read_window(repo.root(), &parts, 2, 2).unwrap_err();
    assert!(
        matches!(error, FinderError::UnknownEvidence { .. }),
        "missing file must be UnknownEvidence, got {error:?}"
    );
}

#[test]
fn read_window_symlink_is_not_followed() {
    // The window path shares the span guards: symlinks fail closed
    // with the same reason, never through a widened hole.
    let repo = ContractRepo::build();
    let link = repo.root().as_path().join("link_outside.rs");
    if !link.is_symlink() {
        return;
    }
    let parts = EvidenceParts::new(
        Revision::new("stubrev").unwrap(),
        PathBuf::from("link_outside.rs"),
        HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let error = engine().read_window(repo.root(), &parts, 2, 2).unwrap_err();
    match error {
        FinderError::UnknownEvidence { detail } => assert!(
            detail.contains("symlink"),
            "symlink window must say why, got {detail:?}"
        ),
        other => panic!("symlink window must fail closed, got {other:?}"),
    }
}

#[test]
fn symbol_definition_outranks_reference() {
    // Reference first in the file on purpose: ranking, not file
    // order, must put the definition first.
    let scratch =
        ScratchRepo::with_files(&[("lib.rs", b"call(needle);\nfn needle() {}\n" as &[u8])]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    assert_eq!(outcome.hits.hits.len(), 2);
    let first = &outcome.hits.hits[0];
    let second = &outcome.hits.hits[1];
    assert_eq!(
        (first.span.start_line, second.span.start_line),
        (2, 1),
        "definition line must outrank the reference line"
    );
    assert!(
        first.reason.rank_score > second.reason.rank_score,
        "definition score must exceed reference score"
    );
}

#[test]
fn whole_token_outranks_substring() {
    let scratch = ScratchRepo::with_files(&[("lib.rs", b"// xneedle\ncall(needle);\n" as &[u8])]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    assert_eq!(outcome.hits.hits.len(), 2);
    assert_eq!(
        (
            outcome.hits.hits[0].span.start_line,
            outcome.hits.hits[1].span.start_line
        ),
        (2, 1),
        "whole-token hit must outrank the substring hit"
    );
}

#[test]
fn external_call_outranks_same_file_mention_at_equal_score() {
    // Same score on both lines (one term, whole token); the usage
    // outside the defining file must come first.
    let scratch = ScratchRepo::with_files(&[
        ("def.rs", b"fn needle() {}\nlet x = needle(0);\n" as &[u8]),
        ("user.rs", b"call(needle);\n" as &[u8]),
    ]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    let order: Vec<(String, u32)> = outcome
        .hits
        .hits
        .iter()
        .map(|hit| (hit.path.to_string_lossy().to_string(), hit.span.start_line))
        .collect();
    assert_eq!(
        order,
        vec![
            ("def.rs".to_string(), 1),
            ("user.rs".to_string(), 1),
            ("def.rs".to_string(), 2),
        ],
        "definition first, then the external usage, then the same-file mention"
    );
}

#[test]
fn file_tiebreak_never_overrides_score() {
    // A two-term same-file line must stay above a one-term external
    // line: the file preference is a tiebreak, not a boost.
    let scratch = ScratchRepo::with_files(&[
        ("def.rs", b"fn needle() {}\nneedle alpha\n" as &[u8]),
        ("user.rs", b"call(needle);\n" as &[u8]),
    ]);
    let query = FinderQuery::new("needle alpha", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    let order: Vec<(String, u32)> = outcome
        .hits
        .hits
        .iter()
        .map(|hit| (hit.path.to_string_lossy().to_string(), hit.span.start_line))
        .collect();
    assert_eq!(
        order,
        vec![
            ("def.rs".to_string(), 1),
            ("def.rs".to_string(), 2),
            ("user.rs".to_string(), 1),
        ],
        "higher score wins regardless of file"
    );
}

#[test]
fn qualified_definitions_earn_boost() {
    let scratch = ScratchRepo::with_files(&[(
        "lib.rs",
        b"call(needle);\npub unsafe fn needle() {}\nasync fn needle() {}\nconst fn needle() {}\n"
            as &[u8],
    )]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    let lines: Vec<u32> = outcome
        .hits
        .hits
        .iter()
        .map(|hit| hit.span.start_line)
        .collect();
    assert_eq!(
        lines,
        vec![2, 3, 4, 1],
        "qualified definitions must outrank the plain reference"
    );
}

#[test]
fn attribute_prefix_and_const_item_keep_definition_boost() {
    let scratch = ScratchRepo::with_files(&[(
        "lib.rs",
        b"call(needle);\n#[inline] fn needle() {}\nconst needle: u32 = 1;\n" as &[u8],
    )]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    let lines: Vec<u32> = outcome
        .hits
        .hits
        .iter()
        .map(|hit| hit.span.start_line)
        .collect();
    assert_eq!(
        lines,
        vec![2, 3, 1],
        "attribute-prefixed fn and const item must both count as definitions"
    );
}

#[test]
fn definition_boost_ignores_non_rust_files() {
    let scratch = ScratchRepo::with_files(&[
        ("notes.md", b"let needle be free\n" as &[u8]),
        ("lib.rs", b"call(needle);\n" as &[u8]),
    ]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    let order: Vec<(String, u32)> = outcome
        .hits
        .hits
        .iter()
        .map(|hit| (hit.path.to_string_lossy().to_string(), hit.span.start_line))
        .collect();
    assert_eq!(
        order,
        vec![("lib.rs".to_string(), 1), ("notes.md".to_string(), 1),],
        "prose lines must not earn the Rust definition boost"
    );
}

#[test]
fn path_ties_keep_path_order() {
    // The file tiebreak is content-only: equal-score filename hits
    // must not reorder on content-definition grounds.
    let scratch = ScratchRepo::with_files(&[
        ("aneedle.rs", b"fn needle() {}\n" as &[u8]),
        ("zneedle.rs", b"x\n" as &[u8]),
    ]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    let paths: Vec<String> = outcome
        .hits
        .hits
        .iter()
        .filter(|hit| hit.reason.field == MatchField::Path)
        .map(|hit| hit.path.to_string_lossy().to_string())
        .collect();
    assert_eq!(
        paths,
        vec!["aneedle.rs".to_string(), "zneedle.rs".to_string()],
        "path-hit ties must stay in path order"
    );
}

#[test]
fn let_binding_boosts_only_the_defined_name() {
    let scratch =
        ScratchRepo::with_files(&[("lib.rs", b"let other = needle;\nlet needle = 1;\n" as &[u8])]);
    let query = FinderQuery::new("needle", 10).unwrap();
    let outcome = engine().search(scratch.root(), &query).unwrap();
    assert_eq!(outcome.hits.hits.len(), 2);
    assert_eq!(
        (
            outcome.hits.hits[0].span.start_line,
            outcome.hits.hits[1].span.start_line
        ),
        (2, 1),
        "only the binding that defines the term earns the definition boost"
    );
}
