//! Repository finder service (TASK-0147, G9).
//!
//! [`FinderService`] is the trusted composition over the finder
//! seam: it binds search pages to revisions, rebinds adapter paging
//! tokens to query+revision, detects stale evidence fail-closed, and
//! applies the [`ContentFilter`] stage. Stateless: every handle and
//! token is self-describing, so any daemon process can serve any
//! page or reopen.
//!
//! [`ContentFilter`] is the INV-008 seam. Phase B ships
//! [`PassthroughFilter`]; a redacting implementation plugs in
//! without touching selection, paging, or staleness.

use std::path::Path;
use std::sync::Arc;

use repo_finder::{
    BackendId, ContinuationToken, EvidenceId, EvidenceParts, FinderError, FinderQuery, Hit,
    RepoProvenance, RepoReadPort, RepoRevisionPort, RepoRoot, RepoSearchPort, SkippedSource,
};

/// Content filtering stage over finder output. Receives the source
/// path plus raw bytes, returns display bytes. Implementations must
/// be total (never fail, never panic): filtering degrades to
/// redaction, never to errors.
pub trait ContentFilter: Send + Sync {
    fn filter_excerpt(&self, path: &Path, excerpt: String) -> String;
    fn filter_lines(&self, path: &Path, lines: Vec<String>) -> Vec<String>;
}

/// Identity filter: bytes pass through unchanged.
pub struct PassthroughFilter;

impl ContentFilter for PassthroughFilter {
    fn filter_excerpt(&self, _path: &Path, excerpt: String) -> String {
        excerpt
    }

    fn filter_lines(&self, _path: &Path, lines: Vec<String>) -> Vec<String> {
        lines
    }
}

/// One service-bound search page: the query it answers, the revision
/// it binds, and hits with a service continuation token.
#[derive(Debug)]
pub struct FinderPage {
    pub query_text: String,
    pub provenance: RepoProvenance,
    pub backend: BackendId,
    pub hits: Vec<Hit>,
    pub total_hits: usize,
    pub skipped: Vec<SkippedSource>,
    pub continuation: Option<ContinuationToken>,
}

/// One reopened span: the handle it resolves, filtered display
/// lines, and the content hash binding the PRE-filter bytes (the
/// handle compares against raw bytes; filtering is display-only).
/// `lines` is the full display window (context + core); the window
/// bounds name it, the handle names the verified core inside it. A
/// zero-context span is exactly the core: window equals the span.
#[derive(Debug)]
pub struct FinderSpan {
    pub parts: EvidenceParts,
    pub lines: Vec<String>,
    pub window_start: u32,
    pub window_end: u32,
    pub content_hash: String,
    pub provenance: RepoProvenance,
}

/// Trusted finder composition. Construct with the three seam
/// backends; [`FinderService::with_filter`] swaps the filter stage.
pub struct FinderService {
    search: Arc<dyn RepoSearchPort>,
    read: Arc<dyn RepoReadPort>,
    revision: Arc<dyn RepoRevisionPort>,
    filter: Arc<dyn ContentFilter>,
}

impl FinderService {
    pub fn new(
        search: Arc<dyn RepoSearchPort>,
        read: Arc<dyn RepoReadPort>,
        revision: Arc<dyn RepoRevisionPort>,
    ) -> Self {
        Self {
            search,
            read,
            revision,
            filter: Arc::new(PassthroughFilter),
        }
    }

    pub fn with_filter(mut self, filter: Arc<dyn ContentFilter>) -> Self {
        self.filter = filter;
        self
    }

    /// One bound page for `query` against `root`. Resolves the
    /// revision first (the service is the binding authority: page
    /// provenance and token binding come from this resolution, not
    /// from the adapter's copy), translates any service token back
    /// to the adapter token it wraps, and filters display excerpts.
    pub fn query(&self, root: &RepoRoot, query: &FinderQuery) -> Result<FinderPage, FinderError> {
        let provenance = self.revision.resolve(root)?;
        let adapter_query = match query.continuation() {
            None => query.clone().replace_continuation(None),
            Some(token) => {
                let adapter = open_service_token(
                    token,
                    query.text(),
                    query.scope(),
                    query.limit(),
                    provenance.revision.as_str(),
                )?;
                query.clone().replace_continuation(Some(adapter))
            }
        };
        let outcome = self.search.search(root, &adapter_query)?;
        let mut hits = outcome.hits.hits;
        for hit in &mut hits {
            let excerpt = std::mem::take(&mut hit.excerpt);
            hit.excerpt = self.filter.filter_excerpt(&hit.path, excerpt);
        }
        let continuation = match outcome.hits.continuation {
            None => None,
            Some(adapter) => Some(mint_service_token(
                query.text(),
                query.scope(),
                query.limit(),
                provenance.revision.as_str(),
                &adapter,
            )?),
        };
        Ok(FinderPage {
            query_text: query.text().to_string(),
            provenance,
            backend: outcome.backend,
            hits,
            total_hits: outcome.hits.total_hits,
            skipped: outcome.hits.skipped,
            continuation,
        })
    }

    /// Reopens the span `id` names: exactly the bound bytes, no
    /// context. Equivalent to [`FinderService::show_with_context`]
    /// with zero context on both sides.
    pub fn show(&self, root: &RepoRoot, id: &EvidenceId) -> Result<FinderSpan, FinderError> {
        self.show_with_context(root, id, 0, 0)
    }

    /// Reopens the span `id` names plus up to `before` preceding and
    /// `after` following lines. Fails closed with
    /// [`FinderError::Stale`] when the revision moved under the
    /// handle, and equally when the bytes drifted under the same
    /// revision (equal revs in the error signal in-revision drift).
    /// The reported hash binds the pre-filter CORE bytes; filtering
    /// is display-only over the whole window. Context beyond
    /// [`repo_finder::MAX_WINDOW_CONTEXT`] per side is
    /// [`FinderError::InvalidInput`], rejected before any I/O.
    pub fn show_with_context(
        &self,
        root: &RepoRoot,
        id: &EvidenceId,
        before: u32,
        after: u32,
    ) -> Result<FinderSpan, FinderError> {
        let (parts, hash) = EvidenceId::parse(id.as_str())?;
        if before > repo_finder::MAX_WINDOW_CONTEXT || after > repo_finder::MAX_WINDOW_CONTEXT {
            return Err(FinderError::InvalidInput(
                "context exceeds bound".to_string(),
            ));
        }
        let provenance = self.revision.resolve(root)?;
        if provenance.revision != parts.revision {
            return Err(FinderError::Stale {
                expected_rev: parts.revision.as_str().to_string(),
                actual_rev: provenance.revision.as_str().to_string(),
            });
        }
        let window = self.read.read_window(root, &parts, before, after)?;
        if !repo_finder::EvidenceId::hash_matches(&hash, &window.core.content_hash) {
            return Err(FinderError::Stale {
                expected_rev: parts.revision.as_str().to_string(),
                actual_rev: provenance.revision.as_str().to_string(),
            });
        }
        // The window reflects the lines actually returned (clamped at
        // file edges), never the request; saturating math keeps a
        // lying engine from producing an invalid bound.
        let window_start = parts
            .span
            .start_line
            .saturating_sub(window.before.len() as u32)
            .max(1);
        let window_end = parts
            .span
            .end_line
            .saturating_add(window.after.len() as u32);
        let mut display = window.before;
        display.extend(window.core.lines);
        display.extend(window.after);
        let lines = self.filter.filter_lines(&parts.path, display);
        Ok(FinderSpan {
            parts,
            lines,
            window_start,
            window_end,
            content_hash: hash,
            provenance,
        })
    }
}

/// Service paging token: `rfc1.<query16>.<revhex>.<tokhex>` — the
/// adapter token wrapped with the query fingerprint and revision it
/// was minted for, so a token replayed against another query or
/// revision fails closed instead of paging the wrong list.
fn mint_service_token(
    text: &str,
    scope: Option<&std::path::Path>,
    limit: usize,
    rev: &str,
    adapter: &ContinuationToken,
) -> Result<ContinuationToken, FinderError> {
    let token = format!(
        "rfc1.{}.{}.{}",
        query_fingerprint(text, scope, limit),
        repo_finder::hex_encode(rev.as_bytes()),
        repo_finder::hex_encode(adapter.as_str().as_bytes())
    );
    ContinuationToken::new(token).map_err(|_| FinderError::BackendFailure {
        detail: "service token exceeds bound".to_string(),
    })
}

fn open_service_token(
    token: &ContinuationToken,
    text: &str,
    scope: Option<&std::path::Path>,
    limit: usize,
    rev: &str,
) -> Result<ContinuationToken, FinderError> {
    fn invalid() -> FinderError {
        FinderError::InvalidContinuation {
            reason: "service token must be `rfc1.<query16>.<revhex>.<tokhex>`".to_string(),
        }
    }
    let rest = token.as_str().strip_prefix("rfc1.").ok_or_else(invalid)?;
    let segments: Vec<&str> = rest.split('.').collect();
    if segments.len() != 3 {
        return Err(invalid());
    }
    if segments[0] != query_fingerprint(text, scope, limit) {
        return Err(invalid());
    }
    let token_rev = String::from_utf8(repo_finder::hex_decode(segments[1]).ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    if token_rev != rev {
        return Err(FinderError::Stale {
            expected_rev: token_rev,
            actual_rev: rev.to_string(),
        });
    }
    let adapter = String::from_utf8(repo_finder::hex_decode(segments[2]).ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    ContinuationToken::new(adapter).map_err(|_| invalid())
}

/// Query fingerprint binding a token to its query: 16 hex chars of
/// SHA-256 over NUL-separated text, scope, and limit. NUL separators
/// (paths cannot contain NUL) keep segment boundaries unambiguous.
fn query_fingerprint(text: &str, scope: Option<&std::path::Path>, limit: usize) -> String {
    let scope = scope
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    repo_finder::content_hash(&[format!("{text}\0{scope}\0{limit}")])[..16].to_string()
}
