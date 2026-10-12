//! `FinderService` behavior: revision-bound paging, fail-closed
//! reopen, and the content-filter stage.
//!
//! Stub ports keep these tests in-memory (no filesystem, no Git):
//! the adapters prove real I/O against the contract suite, this file
//! proves the service's composition promises — service tokens bound
//! to query+revision, stale detection on moved revisions and
//! drifted bytes, and filter application that never breaks handle
//! binding.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use app_services::{ContentFilter, FinderService};
use repo_finder::{
    content_hash, BackendId, BoundedHits, ContinuationToken, EvidenceId, EvidenceParts,
    FinderError, FinderQuery, Hit, HitReason, HitSpan, MatchField, RepoProvenance, RepoReadPort,
    RepoRevisionPort, RepoRoot, RepoSearchPort, Revision, SearchOutcome, SpanContent,
};

struct StubSearch {
    matches: Vec<(PathBuf, u32, String)>,
}

impl RepoSearchPort for StubSearch {
    fn backend(&self) -> BackendId {
        BackendId::new("stub", "0").unwrap()
    }

    fn search(&self, root: &RepoRoot, query: &FinderQuery) -> Result<SearchOutcome, FinderError> {
        let offset = match query.continuation() {
            None => 0,
            Some(token) => token
                .as_str()
                .strip_prefix("off:")
                .and_then(|digits| digits.parse::<usize>().ok())
                .ok_or_else(|| FinderError::InvalidContinuation {
                    reason: "stub: continuation must be `off:<n>`".to_string(),
                })?,
        };
        let total = self.matches.len();
        let revision = Revision::new("r1").unwrap();
        let mut hits = Vec::new();
        for (path, line_no, line) in self.matches.iter().skip(offset).take(query.limit()) {
            let span = HitSpan::new(*line_no, *line_no).unwrap();
            let parts = EvidenceParts::new(revision.clone(), path.clone(), span).unwrap();
            let hash = content_hash(std::slice::from_ref(line));
            hits.push(
                Hit::new(
                    EvidenceId::mint(&parts, &hash).unwrap(),
                    path.clone(),
                    span,
                    line.clone(),
                    HitReason {
                        matched_terms: vec![query.text().to_string()],
                        field: MatchField::Content,
                        rank_score: 1,
                    },
                )
                .unwrap(),
            );
        }
        let continuation = if offset + hits.len() < total {
            Some(ContinuationToken::new(format!("off:{}", offset + hits.len())).unwrap())
        } else {
            None
        };
        Ok(SearchOutcome {
            provenance: RepoProvenance {
                root: root.clone(),
                revision,
                dirty: false,
            },
            backend: self.backend(),
            hits: BoundedHits {
                total_hits: total,
                hits,
                skipped: Vec::new(),
                continuation,
            },
        })
    }
}

struct StubRead {
    files: Mutex<HashMap<PathBuf, Vec<String>>>,
}

impl RepoReadPort for StubRead {
    fn read_span(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
    ) -> Result<SpanContent, FinderError> {
        let files = self.files.lock().unwrap();
        let all = files
            .get(&evidence.path)
            .ok_or_else(|| FinderError::UnknownEvidence {
                detail: "stub: no such file".to_string(),
            })?;
        let start = evidence.span.start_line as usize;
        let end = evidence.span.end_line as usize;
        if start < 1 || end > all.len() {
            return Err(FinderError::UnknownEvidence {
                detail: "stub: span outside file".to_string(),
            });
        }
        let lines = all[start - 1..end].to_vec();
        Ok(SpanContent {
            content_hash: content_hash(&lines),
            lines,
            provenance: RepoProvenance {
                root: root.clone(),
                revision: Revision::new("r1").unwrap(),
                dirty: false,
            },
        })
    }

    fn read_window(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
        before: u32,
        after: u32,
    ) -> Result<repo_finder::WindowContent, FinderError> {
        if before > repo_finder::MAX_WINDOW_CONTEXT || after > repo_finder::MAX_WINDOW_CONTEXT {
            return Err(FinderError::InvalidInput(
                "stub: context exceeds bound".to_string(),
            ));
        }
        let core = self.read_span(root, evidence)?;
        let files = self.files.lock().unwrap();
        let all = files.get(&evidence.path).expect("stub: span read it");
        let start = evidence.span.start_line as usize;
        let end = evidence.span.end_line as usize;
        let from = start.saturating_sub(before as usize + 1);
        let upto = (end + after as usize).min(all.len());
        Ok(repo_finder::WindowContent {
            before: all[from..start - 1].to_vec(),
            after: all[end..upto].to_vec(),
            core,
        })
    }
}

struct StubRevision {
    rev: Mutex<String>,
}

impl RepoRevisionPort for StubRevision {
    fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
        Ok(RepoProvenance {
            root: root.clone(),
            revision: Revision::new(self.rev.lock().unwrap().clone()).unwrap(),
            dirty: false,
        })
    }
}

fn harness() -> (FinderService, Arc<StubRevision>, Arc<StubRead>) {
    let matches = vec![
        (PathBuf::from("a.rs"), 1, "needle one".to_string()),
        (PathBuf::from("b.rs"), 2, "needle two".to_string()),
        (PathBuf::from("c.rs"), 1, "needle three".to_string()),
    ];
    let search: Arc<dyn RepoSearchPort> = Arc::new(StubSearch { matches });
    let mut files = HashMap::new();
    files.insert(PathBuf::from("a.rs"), vec!["needle one".to_string()]);
    files.insert(
        PathBuf::from("b.rs"),
        vec!["pad".to_string(), "needle two".to_string()],
    );
    files.insert(PathBuf::from("c.rs"), vec!["needle three".to_string()]);
    let read = Arc::new(StubRead {
        files: Mutex::new(files),
    });
    let revision = Arc::new(StubRevision {
        rev: Mutex::new("r1".to_string()),
    });
    let read_port: Arc<dyn RepoReadPort> = read.clone();
    let rev_port: Arc<dyn RepoRevisionPort> = revision.clone();
    (
        FinderService::new(search, read_port, rev_port),
        revision,
        read,
    )
}

fn root() -> RepoRoot {
    RepoRoot::new("/stub").unwrap()
}

#[test]
fn query_pages_with_service_tokens_to_full_list() {
    let (service, _, _) = harness();
    let full = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    assert_eq!(full.hits.len(), 3);
    assert!(full.continuation.is_none());
    let mut seen: Vec<String> = Vec::new();
    let mut continuation: Option<ContinuationToken> = None;
    loop {
        let mut query = FinderQuery::new("needle", 1).unwrap();
        if let Some(token) = continuation {
            assert!(
                token.as_str().starts_with("rfc1."),
                "service tokens are self-describing, got {:?}",
                token.as_str()
            );
            query = query.with_continuation(token);
        }
        let page = service.query(&root(), &query).unwrap();
        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.total_hits, 3);
        seen.extend(
            page.hits
                .iter()
                .map(|hit| hit.evidence_id.as_str().to_string()),
        );
        match page.continuation {
            Some(token) => continuation = Some(token),
            None => break,
        }
    }
    let expected: Vec<String> = full
        .hits
        .iter()
        .map(|hit| hit.evidence_id.as_str().to_string())
        .collect();
    assert_eq!(seen, expected);
}

#[test]
fn query_rejects_foreign_token() {
    let (service, _, _) = harness();
    let first = service
        .query(&root(), &FinderQuery::new("needle", 1).unwrap())
        .unwrap();
    let token = first.continuation.unwrap();
    let query = FinderQuery::new("other", 1)
        .unwrap()
        .with_continuation(token);
    let error = service.query(&root(), &query).unwrap_err();
    assert!(
        matches!(error, FinderError::InvalidContinuation { .. }),
        "foreign token must be InvalidContinuation, got {error:?}"
    );
}

#[test]
fn query_rejects_malformed_token() {
    let (service, _, _) = harness();
    let query = FinderQuery::new("needle", 1)
        .unwrap()
        .with_continuation(ContinuationToken::new("garbage").unwrap());
    let error = service.query(&root(), &query).unwrap_err();
    assert!(
        matches!(error, FinderError::InvalidContinuation { .. }),
        "malformed token must be InvalidContinuation, got {error:?}"
    );
}

#[test]
fn query_detects_rev_move_on_paging() {
    let (service, revision, _) = harness();
    let first = service
        .query(&root(), &FinderQuery::new("needle", 1).unwrap())
        .unwrap();
    let token = first.continuation.unwrap();
    *revision.rev.lock().unwrap() = "r2".to_string();
    let query = FinderQuery::new("needle", 1)
        .unwrap()
        .with_continuation(token);
    let error = service.query(&root(), &query).unwrap_err();
    match error {
        FinderError::Stale {
            expected_rev,
            actual_rev,
        } => {
            assert_eq!(expected_rev, "r1");
            assert_eq!(actual_rev, "r2");
        }
        other => panic!("rev move on paging must be Stale, got {other:?}"),
    }
}

#[test]
fn query_is_deterministic() {
    let (service, _, _) = harness();
    let query = FinderQuery::new("needle", 1).unwrap();
    let first = service.query(&root(), &query).unwrap();
    let second = service.query(&root(), &query).unwrap();
    let ids = |hits: &[Hit]| {
        hits.iter()
            .map(|hit| hit.evidence_id.as_str().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&first.hits), ids(&second.hits));
    assert_eq!(
        first.continuation.as_ref().map(|token| token.as_str()),
        second.continuation.as_ref().map(|token| token.as_str()),
        "service tokens must be deterministic"
    );
}

#[test]
fn show_reopens_handle() {
    let (service, _, _) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[1].evidence_id.clone();
    let span = service.show(&root(), &id).unwrap();
    assert_eq!(span.lines, vec!["needle two".to_string()]);
    let (_, hash) = EvidenceId::parse(id.as_str()).unwrap();
    assert_eq!(span.content_hash, hash);
    assert_eq!(span.provenance.revision.as_str(), "r1");
}

#[test]
fn show_rejects_moved_rev() {
    let (service, revision, _) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[0].evidence_id.clone();
    *revision.rev.lock().unwrap() = "r2".to_string();
    let error = service.show(&root(), &id).unwrap_err();
    match error {
        FinderError::Stale {
            expected_rev,
            actual_rev,
        } => {
            assert_eq!(expected_rev, "r1");
            assert_eq!(actual_rev, "r2");
        }
        other => panic!("moved rev must be Stale, got {other:?}"),
    }
}

#[test]
fn show_rejects_drifted_content() {
    let (service, _, read) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[0].evidence_id.clone();
    read.files
        .lock()
        .unwrap()
        .insert(PathBuf::from("a.rs"), vec!["changed".to_string()]);
    let error = service.show(&root(), &id).unwrap_err();
    match error {
        FinderError::Stale {
            expected_rev,
            actual_rev,
        } => {
            // In-revision drift: equal revs signal drifted bytes.
            assert_eq!(expected_rev, "r1");
            assert_eq!(actual_rev, "r1");
        }
        other => panic!("drifted content must be Stale, got {other:?}"),
    }
}

#[test]
fn show_propagates_unknown() {
    let (service, _, _) = harness();
    let parts = EvidenceParts::new(
        Revision::new("r1").unwrap(),
        PathBuf::from("missing.rs"),
        repo_finder::HitSpan::new(1, 1).unwrap(),
    )
    .unwrap();
    let hash = content_hash(&["nothing".to_string()]);
    let id = EvidenceId::mint(&parts, &hash).unwrap();
    let error = service.show(&root(), &id).unwrap_err();
    assert!(
        matches!(error, FinderError::UnknownEvidence { .. }),
        "missing file must stay UnknownEvidence, got {error:?}"
    );
}

struct RedactFilter;

impl ContentFilter for RedactFilter {
    fn filter_excerpt(&self, _path: &Path, excerpt: String) -> String {
        excerpt.replace("secret", "[redacted]")
    }

    fn filter_lines(&self, _path: &Path, lines: Vec<String>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| line.replace("secret", "[redacted]"))
            .collect()
    }
}

#[test]
fn show_with_context_returns_window() {
    // b.rs is ["pad", "needle two"]; the handle spans line 2 only,
    // one context line before widens the display to the full file.
    let (service, _, _) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[1].evidence_id.clone();
    let span = service.show_with_context(&root(), &id, 1, 0).unwrap();
    assert_eq!(
        span.lines,
        vec!["pad".to_string(), "needle two".to_string()]
    );
    assert_eq!(span.window_start, 1);
    assert_eq!(span.window_end, 2);
    assert_eq!(span.parts.span.start_line, 2);
    assert_eq!(span.parts.span.end_line, 2);
    // The hash still binds the raw CORE bytes, never the window.
    let (_, hash) = EvidenceId::parse(id.as_str()).unwrap();
    assert_eq!(span.content_hash, hash);
    assert!(
        EvidenceId::hash_matches(
            &span.content_hash,
            &content_hash(&["needle two".to_string()])
        ),
        "stored handle hash must verify against the raw core bytes"
    );
}

#[test]
fn show_with_context_window_follows_returned_lines() {
    // Mutation pin: the window must reflect the lines actually
    // returned (clamped at EOF), never the request. b.rs line 2 is
    // the last line; asking 5 after must still end the window at 2.
    let (service, _, _) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[1].evidence_id.clone();
    let span = service.show_with_context(&root(), &id, 0, 5).unwrap();
    assert_eq!(span.lines, vec!["needle two".to_string()]);
    assert_eq!(span.window_start, 2);
    assert_eq!(span.window_end, 2);
}

#[test]
fn show_delegates_to_zero_window() {
    let (service, _, _) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[1].evidence_id.clone();
    let plain = service.show(&root(), &id).unwrap();
    let windowed = service.show_with_context(&root(), &id, 0, 0).unwrap();
    assert_eq!(plain.lines, windowed.lines);
    assert_eq!(plain.window_start, plain.parts.span.start_line);
    assert_eq!(plain.window_end, plain.parts.span.end_line);
}

#[test]
fn show_with_context_rejects_over_bound() {
    use repo_finder::MAX_WINDOW_CONTEXT;
    let (service, _, _) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[0].evidence_id.clone();
    for (before, after) in [(MAX_WINDOW_CONTEXT + 1, 0), (0, MAX_WINDOW_CONTEXT + 1)] {
        let error = service
            .show_with_context(&root(), &id, before, after)
            .unwrap_err();
        assert!(
            matches!(error, FinderError::InvalidInput(_)),
            "over-bound context must be InvalidInput, got {error:?}"
        );
    }
}

#[test]
fn show_with_context_still_detects_drift() {
    let (service, _, read) = harness();
    let page = service
        .query(&root(), &FinderQuery::new("needle", 50).unwrap())
        .unwrap();
    let id = page.hits[1].evidence_id.clone();
    read.files.lock().unwrap().insert(
        PathBuf::from("b.rs"),
        vec!["pad".to_string(), "changed".to_string()],
    );
    let error = service.show_with_context(&root(), &id, 1, 0).unwrap_err();
    match error {
        FinderError::Stale {
            expected_rev,
            actual_rev,
        } => {
            assert_eq!(expected_rev, "r1");
            assert_eq!(actual_rev, "r1");
        }
        other => panic!("drifted core must be Stale, got {other:?}"),
    }
}

#[test]
fn show_with_context_filters_whole_window() {
    let search: Arc<dyn RepoSearchPort> = Arc::new(StubSearch {
        matches: vec![(PathBuf::from("s.rs"), 2, "needle".to_string())],
    });
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("s.rs"),
        vec!["secret pad".to_string(), "needle".to_string()],
    );
    let read: Arc<dyn RepoReadPort> = Arc::new(StubRead {
        files: Mutex::new(files),
    });
    let revision: Arc<dyn RepoRevisionPort> = Arc::new(StubRevision {
        rev: Mutex::new("r1".to_string()),
    });
    let service = FinderService::new(search, read, revision).with_filter(Arc::new(RedactFilter));
    let page = service
        .query(&root(), &FinderQuery::new("needle", 10).unwrap())
        .unwrap();
    let id = page.hits[0].evidence_id.clone();
    let span = service.show_with_context(&root(), &id, 1, 0).unwrap();
    assert_eq!(
        span.lines,
        vec!["[redacted] pad".to_string(), "needle".to_string()]
    );
    let (_, hash) = EvidenceId::parse(id.as_str()).unwrap();
    assert_eq!(span.content_hash, hash);
}

#[test]
fn filter_is_applied_without_breaking_binding() {
    let matches = vec![(PathBuf::from("s.rs"), 1, "secret needle".to_string())];
    let search: Arc<dyn RepoSearchPort> = Arc::new(StubSearch { matches });
    let mut files = HashMap::new();
    files.insert(PathBuf::from("s.rs"), vec!["secret needle".to_string()]);
    let read: Arc<dyn RepoReadPort> = Arc::new(StubRead {
        files: Mutex::new(files),
    });
    let revision: Arc<dyn RepoRevisionPort> = Arc::new(StubRevision {
        rev: Mutex::new("r1".to_string()),
    });
    let service = FinderService::new(search, read, revision).with_filter(Arc::new(RedactFilter));
    let page = service
        .query(&root(), &FinderQuery::new("needle", 10).unwrap())
        .unwrap();
    assert_eq!(page.hits[0].excerpt, "[redacted] needle");
    let id = page.hits[0].evidence_id.clone();
    let span = service.show(&root(), &id).unwrap();
    assert_eq!(span.lines, vec!["[redacted] needle".to_string()]);
    // Binding still compares raw bytes: the handle hash matches.
    let (_, hash) = EvidenceId::parse(id.as_str()).unwrap();
    assert_eq!(span.content_hash, hash);
    assert!(
        EvidenceId::hash_matches(
            &span.content_hash,
            &content_hash(&["secret needle".to_string()])
        ),
        "stored handle hash must verify against the raw bytes"
    );
}

#[test]
fn redacting_filter_keeps_secrets_out_of_query_and_show() {
    // INV-008 regression pin (TASK-0174): the fail-closed default
    // filter (no explicit with_filter — this pins the default
    // itself) keeps secret bytes out of excerpts and windows, while
    // handle binding still verifies against raw bytes.
    let search: Arc<dyn RepoSearchPort> = Arc::new(StubSearch {
        matches: vec![
            (
                PathBuf::from("config.rs"),
                1,
                "password=hunter2".to_string(),
            ),
            (
                PathBuf::from("config.rs"),
                2,
                "token ghp_abcdefgh1234567890".to_string(),
            ),
        ],
    });
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("config.rs"),
        vec![
            "password=hunter2".to_string(),
            "token ghp_abcdefgh1234567890".to_string(),
        ],
    );
    let read: Arc<dyn RepoReadPort> = Arc::new(StubRead {
        files: Mutex::new(files),
    });
    let revision: Arc<dyn RepoRevisionPort> = Arc::new(StubRevision {
        rev: Mutex::new("r1".to_string()),
    });
    let service = FinderService::new(search, read, revision);
    let page = service
        .query(&root(), &FinderQuery::new("password", 10).unwrap())
        .unwrap();
    assert_eq!(page.hits.len(), 2);
    for hit in &page.hits {
        assert!(
            !hit.excerpt.contains("hunter2") && !hit.excerpt.contains("ghp_abcdefgh"),
            "excerpt must carry no secret bytes, got {:?}",
            hit.excerpt
        );
    }
    assert_eq!(page.hits[0].excerpt, "password=[redacted]");
    let id = page.hits[0].evidence_id.clone();
    let span = service.show_with_context(&root(), &id, 0, 1).unwrap();
    for line in &span.lines {
        assert!(
            !line.contains("hunter2") && !line.contains("ghp_abcdefgh"),
            "window must carry no secret bytes, got {line:?}"
        );
    }
    // Binding intact: the stored handle hash still verifies against
    // the raw (pre-filter) core bytes.
    assert!(EvidenceId::hash_matches(
        &span.content_hash,
        &content_hash(&["password=hunter2".to_string()])
    ));
}
