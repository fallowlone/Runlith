//! Finder IPC dispatch (TASK-0147, G9): `FinderCoreRequest` in,
//! `FinderCoreResponse` out. Pure wire mapping plus validation over
//! [`FinderService`]: server-local paths become [`RepoRoot`]s,
//! paging handles round-trip opaquely, and every [`FinderError`]
//! becomes a typed error code. No I/O beyond the service call; the
//! serve loop runs this on a blocking thread because scans are
//! seconds-long filesystem work.

use app_services::{FinderPage, FinderService, FinderSpan};
use ipc_contracts::proto::{
    finder_core_request, finder_core_response, FinderCoreRequest, FinderCoreResponse,
    FinderErrorResponse, FinderHit, FinderPageResponse, FinderQueryRequest, FinderShowRequest,
    FinderSkipped, FinderSpanResponse,
};
use repo_finder::{
    ContinuationToken, EvidenceId, FinderError, FinderQuery, MatchField, RepoRoot, SkipReason,
};

/// Server default page size when the client sends limit 0.
pub const DEFAULT_QUERY_LIMIT: u32 = 20;

/// Dispatches one finder request against the composed service.
pub fn dispatch(service: &FinderService, request: FinderCoreRequest) -> FinderCoreResponse {
    match request.payload {
        Some(finder_core_request::Payload::Query(query)) => dispatch_query(service, query),
        Some(finder_core_request::Payload::Show(show)) => dispatch_show(service, show),
        None => error_response("invalid_input", "missing finder payload", "", ""),
    }
}

/// Typed unavailable shape for daemons without a finder composition.
pub fn unavailable() -> FinderCoreResponse {
    error_response("unavailable", "finder composition is not available", "", "")
}

/// Typed failure shape when the blocking dispatch task itself fails
/// (panic or cancellation): the call failed, no result exists.
pub fn failed() -> FinderCoreResponse {
    error_response("backend_failure", "finder task failed", "", "")
}

fn dispatch_query(service: &FinderService, query: FinderQueryRequest) -> FinderCoreResponse {
    let root = match RepoRoot::new(query.repository_path) {
        Ok(root) => root,
        Err(_) => {
            return error_response(
                "invalid_input",
                "repository_path must be a non-blank absolute path",
                "",
                "",
            );
        }
    };
    let limit = if query.limit == 0 {
        DEFAULT_QUERY_LIMIT
    } else {
        query.limit
    };
    let built = match FinderQuery::new(query.text, limit as usize) {
        Ok(built) => built,
        Err(error) => return map_error(&error),
    };
    let built = if query.scope.is_empty() {
        built
    } else {
        match built.with_scope(query.scope) {
            Ok(built) => built,
            Err(_) => {
                return error_response("invalid_query", "scope escapes repository scope", "", "");
            }
        }
    };
    let built = if query.continuation.is_empty() {
        built
    } else {
        match ContinuationToken::new(query.continuation) {
            Ok(token) => built.with_continuation(token),
            Err(_) => {
                return error_response(
                    "invalid_continuation",
                    "malformed continuation token",
                    "",
                    "",
                );
            }
        }
    };
    match service.query(&root, &built) {
        Ok(page) => page_response(page),
        Err(error) => map_error(&error),
    }
}

fn dispatch_show(service: &FinderService, show: FinderShowRequest) -> FinderCoreResponse {
    let root = match RepoRoot::new(show.repository_path) {
        Ok(root) => root,
        Err(_) => {
            return error_response(
                "invalid_input",
                "repository_path must be a non-blank absolute path",
                "",
                "",
            );
        }
    };
    let (parts, hash) = match EvidenceId::parse(&show.evidence_id) {
        Ok(parsed) => parsed,
        Err(_) => {
            return error_response("invalid_input", "malformed evidence id", "", "");
        }
    };
    // Rebuilds the owned handle from validated parts: infallible in
    // practice (parse already enforced every mint rule), defensive
    // error instead of a panic if the rules ever drift apart.
    let id = match EvidenceId::mint(&parts, &hash) {
        Ok(id) => id,
        Err(_) => {
            return error_response("invalid_input", "malformed evidence id", "", "");
        }
    };
    match service.show_with_context(&root, &id, show.context_before, show.context_after) {
        Ok(span) => span_response(span),
        Err(error) => map_error(&error),
    }
}

fn page_response(page: FinderPage) -> FinderCoreResponse {
    FinderCoreResponse {
        payload: Some(finder_core_response::Payload::Page(FinderPageResponse {
            query_text: page.query_text,
            revision: page.provenance.revision.as_str().to_owned(),
            dirty: page.provenance.dirty,
            backend_name: page.backend.name,
            backend_version: page.backend.version,
            hits: page.hits.iter().map(map_hit).collect(),
            total_hits: page.total_hits as u64,
            skipped: page.skipped.iter().map(map_skipped).collect(),
            continuation: page
                .continuation
                .map(|token| token.as_str().to_owned())
                .unwrap_or_default(),
        })),
    }
}

fn span_response(span: FinderSpan) -> FinderCoreResponse {
    FinderCoreResponse {
        payload: Some(finder_core_response::Payload::Span(FinderSpanResponse {
            path: span.parts.path.to_string_lossy().to_string(),
            start_line: span.parts.span.start_line,
            end_line: span.parts.span.end_line,
            lines: span.lines,
            content_hash: span.content_hash,
            revision: span.provenance.revision.as_str().to_owned(),
            dirty: span.provenance.dirty,
            window_start_line: span.window_start,
            window_end_line: span.window_end,
        })),
    }
}

fn map_hit(hit: &repo_finder::Hit) -> FinderHit {
    FinderHit {
        evidence_id: hit.evidence_id.as_str().to_owned(),
        path: hit.path.to_string_lossy().to_string(),
        start_line: hit.span.start_line,
        end_line: hit.span.end_line,
        excerpt: hit.excerpt.clone(),
        matched_terms: hit.reason.matched_terms.clone(),
        field: match hit.reason.field {
            MatchField::Path => "path".to_owned(),
            MatchField::Content => "content".to_owned(),
        },
        rank_score: hit.reason.rank_score,
    }
}

fn map_skipped(skipped: &repo_finder::SkippedSource) -> FinderSkipped {
    let (reason, detail) = match &skipped.reason {
        SkipReason::Binary => ("binary".to_owned(), String::new()),
        SkipReason::TooLarge { bytes } => ("too_large".to_owned(), bytes.to_string()),
        SkipReason::Inaccessible { detail } => ("inaccessible".to_owned(), detail.clone()),
        SkipReason::Denied { rule } => ("denied".to_owned(), rule.clone()),
    };
    FinderSkipped {
        path: skipped.path.to_string_lossy().to_string(),
        reason,
        detail,
    }
}

fn map_error(error: &FinderError) -> FinderCoreResponse {
    let (code, detail, expected_rev, actual_rev) = match error {
        FinderError::Stale {
            expected_rev,
            actual_rev,
        } => (
            "stale",
            if expected_rev == actual_rev {
                "evidence bytes drifted under revision".to_owned()
            } else {
                "evidence revision moved".to_owned()
            },
            expected_rev.clone(),
            actual_rev.clone(),
        ),
        FinderError::UnknownEvidence { detail } => (
            "unknown_evidence",
            detail.clone(),
            String::new(),
            String::new(),
        ),
        FinderError::InvalidQuery { reason } => (
            "invalid_query",
            reason.clone(),
            String::new(),
            String::new(),
        ),
        FinderError::InvalidContinuation { reason } => (
            "invalid_continuation",
            reason.clone(),
            String::new(),
            String::new(),
        ),
        FinderError::NotARepository { root } => (
            "not_a_repository",
            bound_detail(&root.to_string_lossy()),
            String::new(),
            String::new(),
        ),
        FinderError::RevisionUnresolvable { detail } => (
            "revision_unresolvable",
            detail.clone(),
            String::new(),
            String::new(),
        ),
        FinderError::ScopeEscape { detail } => {
            ("scope_escape", detail.clone(), String::new(), String::new())
        }
        FinderError::BackendFailure { detail } => (
            "backend_failure",
            detail.clone(),
            String::new(),
            String::new(),
        ),
        FinderError::InvalidInput(detail) => (
            "invalid_input",
            detail.clone(),
            String::new(),
            String::new(),
        ),
    };
    error_response(code, &detail, &expected_rev, &actual_rev)
}

fn error_response(
    code: &str,
    detail: &str,
    expected_rev: &str,
    actual_rev: &str,
) -> FinderCoreResponse {
    FinderCoreResponse {
        payload: Some(finder_core_response::Payload::Error(FinderErrorResponse {
            code: code.to_owned(),
            detail: detail.to_owned(),
            expected_rev: expected_rev.to_owned(),
            actual_rev: actual_rev.to_owned(),
        })),
    }
}

fn bound_detail(detail: &str) -> String {
    detail.chars().take(repo_finder::MAX_DETAIL_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use ipc_contracts::proto::{
        finder_core_request, finder_core_response, FinderQueryRequest, FinderShowRequest,
    };
    use repo_finder::{
        content_hash, EvidenceId, EvidenceParts, FinderError, HitSpan, RepoProvenance,
        RepoRevisionPort, RepoRoot, Revision,
    };
    use repo_finder_contract::{ContractRepo, NEEDLE_ALPHA};

    struct StubRevision;

    impl RepoRevisionPort for StubRevision {
        fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
            Ok(RepoProvenance {
                root: root.clone(),
                revision: Revision::new("testrev").unwrap(),
                dirty: false,
            })
        }
    }

    fn service() -> FinderService {
        let revision: Arc<dyn RepoRevisionPort> = Arc::new(StubRevision);
        let engine = Arc::new(finder_local::LocalFinder::new(revision.clone()));
        let search: Arc<dyn repo_finder::RepoSearchPort> = engine.clone();
        let read: Arc<dyn repo_finder::RepoReadPort> = engine;
        FinderService::new(search, read, revision)
    }

    fn query_request(repo: &ContractRepo, text: &str, limit: u32) -> FinderCoreRequest {
        FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Query(FinderQueryRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                text: text.to_owned(),
                scope: String::new(),
                limit,
                continuation: String::new(),
            })),
        }
    }

    fn page_of(response: &FinderCoreResponse) -> &ipc_contracts::proto::FinderPageResponse {
        match &response.payload {
            Some(finder_core_response::Payload::Page(page)) => page,
            other => panic!("expected page payload, got {other:?}"),
        }
    }

    fn error_of(response: &FinderCoreResponse) -> &ipc_contracts::proto::FinderErrorResponse {
        match &response.payload {
            Some(finder_core_response::Payload::Error(error)) => error,
            other => panic!("expected error payload, got {other:?}"),
        }
    }

    #[test]
    fn query_returns_mapped_page() {
        let repo = ContractRepo::build();
        let response = dispatch(&service(), query_request(&repo, NEEDLE_ALPHA, 10));
        let page = page_of(&response);
        assert!(!page.hits.is_empty(), "fixture term must map to hits");
        assert!(page.total_hits as usize >= page.hits.len());
        assert_eq!(page.backend_name, "finder-local");
        assert_eq!(page.revision, "testrev");
        assert!(!page.hits[0].evidence_id.is_empty());
        assert!(!page.hits[0].excerpt.is_empty());
        assert!(page.continuation.is_empty(), "small page has no remainder");
    }

    #[test]
    fn query_pages_through_continuation() {
        let repo = ContractRepo::build();
        let service = service();
        let first_response = dispatch(&service, query_request(&repo, NEEDLE_ALPHA, 1));
        let first = page_of(&first_response).clone();
        assert_eq!(first.hits.len(), 1);
        assert!(!first.continuation.is_empty());
        let mut request = query_request(&repo, NEEDLE_ALPHA, 1);
        match request.payload.as_mut().unwrap() {
            finder_core_request::Payload::Query(query) => {
                query.continuation = first.continuation.clone();
            }
            _ => unreachable!(),
        }
        let second_response = dispatch(&service, request);
        let second = page_of(&second_response).clone();
        assert_eq!(second.hits.len(), 1);
        assert_ne!(
            second.hits[0].evidence_id, first.hits[0].evidence_id,
            "second page must advance"
        );
    }

    #[test]
    fn query_rejects_relative_root() {
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Query(FinderQueryRequest {
                repository_path: "repo/sub".to_owned(),
                text: NEEDLE_ALPHA.to_owned(),
                scope: String::new(),
                limit: 10,
                continuation: String::new(),
            })),
        };
        let response = dispatch(&service(), request);
        let error = error_of(&response);
        assert_eq!(error.code, "invalid_input");
    }

    #[test]
    fn query_rejects_blank_text() {
        let repo = ContractRepo::build();
        let response = dispatch(&service(), query_request(&repo, "  ", 10));
        let error = error_of(&response);
        assert_eq!(error.code, "invalid_query");
    }

    #[test]
    fn query_applies_default_limit_when_zero() {
        let repo = ContractRepo::build();
        let response = dispatch(&service(), query_request(&repo, NEEDLE_ALPHA, 0));
        let page = page_of(&response);
        assert!(!page.hits.is_empty(), "limit 0 means server default");
    }

    #[test]
    fn show_reopens_span() {
        let repo = ContractRepo::build();
        let service = service();
        let page_response = dispatch(&service, query_request(&repo, NEEDLE_ALPHA, 10));
        let page = page_of(&page_response).clone();
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                evidence_id: page.hits[0].evidence_id.clone(),
                context_before: 0,
                context_after: 0,
            })),
        };
        let response = dispatch(&service, request);
        match &response.payload {
            Some(finder_core_response::Payload::Span(span)) => {
                assert!(!span.lines.is_empty());
                assert!(!span.content_hash.is_empty());
                assert_eq!(span.revision, "testrev");
            }
            other => panic!("expected span payload, got {other:?}"),
        }
    }

    #[test]
    fn show_rejects_garbage_id() {
        let repo = ContractRepo::build();
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                evidence_id: "garbage".to_owned(),
                context_before: 0,
                context_after: 0,
            })),
        };
        let response = dispatch(&service(), request);
        let error = error_of(&response);
        assert_eq!(error.code, "invalid_input");
    }

    #[test]
    fn show_maps_unknown_file() {
        let repo = ContractRepo::build();
        let parts = EvidenceParts::new(
            Revision::new("testrev").unwrap(),
            std::path::PathBuf::from("missing.rs"),
            HitSpan::new(1, 1).unwrap(),
        )
        .unwrap();
        let hash = content_hash(&["nothing".to_string()]);
        let id = EvidenceId::mint(&parts, &hash).unwrap();
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                evidence_id: id.as_str().to_owned(),
                context_before: 0,
                context_after: 0,
            })),
        };
        let response = dispatch(&service(), request);
        let error = error_of(&response);
        assert_eq!(error.code, "unknown_evidence");
    }

    #[test]
    fn error_codes_cover_the_seam_taxonomy() {
        let cases: Vec<(FinderError, &str)> = vec![
            (
                FinderError::Stale {
                    expected_rev: "a".to_owned(),
                    actual_rev: "b".to_owned(),
                },
                "stale",
            ),
            (
                FinderError::UnknownEvidence {
                    detail: "x".to_owned(),
                },
                "unknown_evidence",
            ),
            (
                FinderError::InvalidQuery {
                    reason: "x".to_owned(),
                },
                "invalid_query",
            ),
            (
                FinderError::InvalidContinuation {
                    reason: "x".to_owned(),
                },
                "invalid_continuation",
            ),
            (
                FinderError::NotARepository {
                    root: std::path::PathBuf::from("/repo"),
                },
                "not_a_repository",
            ),
            (
                FinderError::RevisionUnresolvable {
                    detail: "x".to_owned(),
                },
                "revision_unresolvable",
            ),
            (
                FinderError::ScopeEscape {
                    detail: "x".to_owned(),
                },
                "scope_escape",
            ),
            (
                FinderError::BackendFailure {
                    detail: "x".to_owned(),
                },
                "backend_failure",
            ),
            (FinderError::InvalidInput("x".to_owned()), "invalid_input"),
        ];
        for (error, code) in cases {
            assert_eq!(error_of(&map_error(&error)).code, code);
        }
        let stale_response = map_error(&FinderError::Stale {
            expected_rev: "a".to_owned(),
            actual_rev: "b".to_owned(),
        });
        let stale = error_of(&stale_response);
        assert_eq!(stale.expected_rev, "a");
        assert_eq!(stale.actual_rev, "b");
    }

    #[test]
    fn unavailable_and_failed_shapes_are_typed() {
        assert_eq!(error_of(&unavailable()).code, "unavailable");
        assert_eq!(error_of(&failed()).code, "backend_failure");
    }

    #[test]
    fn show_passes_context_to_window() {
        // notes.md is ["# notes", "", "<needle> ..."]; the hit spans
        // line 3, one context line before widens the window to 2-3.
        let repo = ContractRepo::build();
        let service = service();
        let page_response = dispatch(&service, query_request(&repo, NEEDLE_ALPHA, 10));
        let page = page_of(&page_response).clone();
        let notes = page
            .hits
            .iter()
            .find(|hit| hit.path == "notes.md")
            .expect("fixture must hit notes.md");
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                evidence_id: notes.evidence_id.clone(),
                context_before: 1,
                context_after: 0,
            })),
        };
        let response = dispatch(&service, request);
        match &response.payload {
            Some(finder_core_response::Payload::Span(span)) => {
                assert_eq!(span.start_line, 3);
                assert_eq!(span.end_line, 3);
                assert_eq!(span.window_start_line, 2);
                assert_eq!(span.window_end_line, 3);
                assert_eq!(span.lines.len(), 2);
                assert_eq!(span.lines[1], notes.excerpt);
            }
            other => panic!("expected span payload, got {other:?}"),
        }
    }

    #[test]
    fn show_rejects_over_bound_context() {
        let repo = ContractRepo::build();
        let service = service();
        let page_response = dispatch(&service, query_request(&repo, NEEDLE_ALPHA, 10));
        let page = page_of(&page_response).clone();
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                evidence_id: page.hits[0].evidence_id.clone(),
                context_before: repo_finder::MAX_WINDOW_CONTEXT + 1,
                context_after: 0,
            })),
        };
        let response = dispatch(&service, request);
        let error = error_of(&response);
        assert_eq!(error.code, "invalid_input");
    }

    #[test]
    fn show_without_context_reports_exact_window() {
        let repo = ContractRepo::build();
        let service = service();
        let page_response = dispatch(&service, query_request(&repo, NEEDLE_ALPHA, 10));
        let page = page_of(&page_response).clone();
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
                repository_path: repo.root().as_path().to_string_lossy().to_string(),
                evidence_id: page.hits[0].evidence_id.clone(),
                context_before: 0,
                context_after: 0,
            })),
        };
        let response = dispatch(&service, request);
        match &response.payload {
            Some(finder_core_response::Payload::Span(span)) => {
                assert_eq!(span.window_start_line, span.start_line);
                assert_eq!(span.window_end_line, span.end_line);
                assert_eq!(span.lines.len() as u32, span.end_line - span.start_line + 1);
            }
            other => panic!("expected span payload, got {other:?}"),
        }
    }
}
