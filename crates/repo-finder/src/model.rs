//! Finder domain values: validated identities, bounded hits, canonical problems.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

/// Generous bound for query text (chars).
pub const MAX_QUERY_TEXT_CHARS: usize = 1024;
/// Hard cap for one query page.
pub const MAX_QUERY_LIMIT: usize = 200;
/// Generous bound for evidence identifiers (bytes).
pub const MAX_EVIDENCE_ID_BYTES: usize = 2048;
/// Generous bound for hit excerpts (chars). Adapters stay tighter.
pub const MAX_EXCERPT_CHARS: usize = 4096;
/// Generous bound for failure detail (chars).
pub const MAX_DETAIL_CHARS: usize = 2048;
/// Hard cap for one reopened span (lines).
pub const MAX_SPAN_LINES: u32 = 2000;
/// Hard cap for context lines on each side of a window reopen.
pub const MAX_WINDOW_CONTEXT: u32 = 200;
/// Generous bound for evidence paths (bytes).
pub const MAX_PATH_BYTES: usize = 1024;
/// Generous bound for revision strings (chars).
pub const MAX_REVISION_CHARS: usize = 128;
/// Generous bound for backend identity fields (chars).
pub const MAX_BACKEND_ID_CHARS: usize = 128;
/// Generous bound for continuation tokens (bytes).
pub const MAX_CONTINUATION_BYTES: usize = 1024;

/// Canonical finder failure. Small seam-local taxonomy: every adapter
/// maps its backend failures onto these variants, never onto strings
/// the service would have to re-parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinderError {
    /// Blank, over-long, or otherwise malformed input value.
    InvalidInput(String),
    /// The root is not a repository the backend can bind.
    NotARepository { root: PathBuf },
    /// Revision resolution failed (bounded detail, never a dump).
    RevisionUnresolvable { detail: String },
    /// The evidence revision moved under a reopened handle.
    Stale {
        expected_rev: String,
        actual_rev: String,
    },
    /// A well-formed handle the backend cannot resolve. Fail-closed:
    /// never synthesized content, never a neighboring file.
    UnknownEvidence { detail: String },
    /// The query itself is unusable.
    InvalidQuery { reason: String },
    /// The continuation token is malformed, foreign, or stale.
    InvalidContinuation { reason: String },
    /// A path escaped (or would escape) repository scope.
    ScopeEscape { detail: String },
    /// Backend failure with bounded detail.
    BackendFailure { detail: String },
}

impl fmt::Display for FinderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(detail) => write!(f, "invalid input: {detail}"),
            Self::NotARepository { root } => {
                write!(f, "not a repository: {}", root.display())
            }
            Self::RevisionUnresolvable { detail } => {
                write!(f, "revision unresolvable: {detail}")
            }
            Self::Stale {
                expected_rev,
                actual_rev,
            } => write!(
                f,
                "stale evidence: expected {expected_rev}, now {actual_rev}"
            ),
            Self::UnknownEvidence { detail } => write!(f, "unknown evidence: {detail}"),
            Self::InvalidQuery { reason } => write!(f, "invalid query: {reason}"),
            Self::InvalidContinuation { reason } => {
                write!(f, "invalid continuation: {reason}")
            }
            Self::ScopeEscape { detail } => write!(f, "scope escape: {detail}"),
            Self::BackendFailure { detail } => write!(f, "backend failure: {detail}"),
        }
    }
}

impl std::error::Error for FinderError {}

/// Validated absolute repository root. Adapters resolve every read
/// against this root; confinement is structural, not advisory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot(PathBuf);

impl RepoRoot {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, FinderError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(FinderError::InvalidInput(
                "repository root must not be blank".to_string(),
            ));
        }
        if !path.is_absolute() {
            return Err(FinderError::InvalidInput(
                "repository root must be absolute".to_string(),
            ));
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Opaque validated revision string (a VCS revision at the adapter's
/// discretion; git SHAs qualify). Compared for equality only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision(String);

impl Revision {
    pub fn new(value: impl Into<String>) -> Result<Self, FinderError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(FinderError::InvalidInput(
                "revision must not be blank".to_string(),
            ));
        }
        if value.chars().count() > MAX_REVISION_CHARS {
            return Err(FinderError::InvalidInput(
                "revision exceeds bound".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Revision binding carried by every outcome: what revision the
/// evidence was selected against, and whether the tree was dirty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoProvenance {
    pub root: RepoRoot,
    pub revision: Revision,
    pub dirty: bool,
}

/// Opaque paging handle minted by the service. The port treats it as
/// an opaque validated string; interpretation is service-side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationToken(String);

impl ContinuationToken {
    pub fn new(value: impl Into<String>) -> Result<Self, FinderError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(FinderError::InvalidInput(
                "continuation token must not be blank".to_string(),
            ));
        }
        if value.len() > MAX_CONTINUATION_BYTES {
            return Err(FinderError::InvalidInput(
                "continuation token exceeds bound".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Adapter identity for the response envelope: which engine produced
/// the selection, and in which version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendId {
    pub name: String,
    pub version: String,
}

impl BackendId {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Result<Self, FinderError> {
        let (name, version) = (name.into(), version.into());
        if name.trim().is_empty() || version.trim().is_empty() {
            return Err(FinderError::InvalidInput(
                "backend name and version must not be blank".to_string(),
            ));
        }
        if name.chars().count() > MAX_BACKEND_ID_CHARS
            || version.chars().count() > MAX_BACKEND_ID_CHARS
        {
            return Err(FinderError::InvalidInput(
                "backend identity exceeds bound".to_string(),
            ));
        }
        Ok(Self { name, version })
    }
}

/// Bounded search request: non-blank text, a capped page size, an
/// optional confined scope, and an optional paging handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinderQuery {
    text: String,
    limit: usize,
    scope: Option<PathBuf>,
    continuation: Option<ContinuationToken>,
}

impl FinderQuery {
    pub fn new(text: impl Into<String>, limit: usize) -> Result<Self, FinderError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(FinderError::InvalidQuery {
                reason: "query text must not be blank".to_string(),
            });
        }
        if text.chars().count() > MAX_QUERY_TEXT_CHARS {
            return Err(FinderError::InvalidQuery {
                reason: "query text exceeds bound".to_string(),
            });
        }
        if limit == 0 || limit > MAX_QUERY_LIMIT {
            return Err(FinderError::InvalidQuery {
                reason: format!("limit must be 1..={MAX_QUERY_LIMIT}"),
            });
        }
        Ok(Self {
            text,
            limit,
            scope: None,
            continuation: None,
        })
    }

    pub fn with_scope(mut self, scope: impl Into<PathBuf>) -> Result<Self, FinderError> {
        let scope = scope.into();
        validate_relative_confined(&scope)?;
        self.scope = Some(scope);
        Ok(self)
    }

    pub fn with_continuation(mut self, token: ContinuationToken) -> Self {
        self.continuation = Some(token);
        self
    }

    /// Swaps the paging handle without revalidation: the query was
    /// validated at construction, and tokens are opaque to it. The
    /// service uses this to translate between service and adapter
    /// tokens on an already-valid query.
    pub fn replace_continuation(mut self, token: Option<ContinuationToken>) -> Self {
        self.continuation = token;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn scope(&self) -> Option<&Path> {
        self.scope.as_deref()
    }

    pub fn continuation(&self) -> Option<&ContinuationToken> {
        self.continuation.as_ref()
    }
}

/// Rejects anything that is not a confined relative path: blank,
/// absolute, `..`, leading `.`, prefixes, and root dir. Shared by
/// query scopes and evidence paths: confinement is checked once, at
/// construction, in one place. Interior `.` segments normalize away
/// under `Path::components` and are accepted as the confined path
/// they denote.
fn validate_relative_confined(path: &Path) -> Result<(), FinderError> {
    if path.as_os_str().is_empty() {
        return Err(FinderError::ScopeEscape {
            detail: "path must not be blank".to_string(),
        });
    }
    if path.is_absolute() {
        return Err(FinderError::ScopeEscape {
            detail: "absolute path escapes repository scope".to_string(),
        });
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::ParentDir => {
                return Err(FinderError::ScopeEscape {
                    detail: "`..` escapes repository scope".to_string(),
                });
            }
            Component::CurDir => {
                return Err(FinderError::ScopeEscape {
                    detail: "interior `.` is not allowed".to_string(),
                });
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(FinderError::ScopeEscape {
                    detail: "path escapes repository scope".to_string(),
                });
            }
        }
    }
    Ok(())
}

/// One-based inclusive line span. Bounded: a reopened span is
/// evidence, never a file dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitSpan {
    pub start_line: u32,
    pub end_line: u32,
}

impl HitSpan {
    pub fn new(start_line: u32, end_line: u32) -> Result<Self, FinderError> {
        if start_line == 0 {
            return Err(FinderError::InvalidInput(
                "span start must be one-based".to_string(),
            ));
        }
        if end_line < start_line {
            return Err(FinderError::InvalidInput(
                "span end precedes start".to_string(),
            ));
        }
        if end_line - start_line + 1 > MAX_SPAN_LINES {
            return Err(FinderError::InvalidInput("span exceeds bound".to_string()));
        }
        Ok(Self {
            start_line,
            end_line,
        })
    }
}

/// The structured parts of a reopenable evidence handle: which
/// revision, which confined relative path, which span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceParts {
    pub revision: Revision,
    pub path: PathBuf,
    pub span: HitSpan,
}

impl EvidenceParts {
    pub fn new(revision: Revision, path: PathBuf, span: HitSpan) -> Result<Self, FinderError> {
        validate_relative_confined(&path)?;
        if path.as_os_str().len() > MAX_PATH_BYTES {
            return Err(FinderError::InvalidInput(
                "evidence path exceeds bound".to_string(),
            ));
        }
        Ok(Self {
            revision,
            path,
            span,
        })
    }
}

/// Stable content binding for a span: lowercase hex SHA-256 over the
/// lines joined with `\n` (no trailing separator). Same bytes in,
/// same hash out, across processes and platforms — the staleness
/// check compares this, never line counts.
pub fn content_hash(lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            hasher.update(b"\n");
        }
        hasher.update(line.as_bytes());
    }
    hex_encode(&hasher.finalize())
}

/// Self-describing reopenable handle:
/// `rf1.<rev>.<start>.<end>.<hash>.<hexpath>`. Opaque to callers —
/// only [`EvidenceId::parse`] reads it — and fail-closed: anything
/// malformed is [`FinderError::InvalidInput`], never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceId(String);

impl EvidenceId {
    pub fn mint(parts: &EvidenceParts, hash: &str) -> Result<Self, FinderError> {
        let hash = hash.to_ascii_lowercase();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(FinderError::InvalidInput(
                "content hash must be 64 hex chars".to_string(),
            ));
        }
        let revision = parts.revision.as_str();
        if revision.is_empty()
            || !revision
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(FinderError::InvalidInput(
                "revision is not evidence-safe".to_string(),
            ));
        }
        let path = parts
            .path
            .to_str()
            .ok_or_else(|| FinderError::InvalidInput("evidence path must be UTF-8".to_string()))?;
        let id = format!(
            "rf1.{revision}.{}.{}.{hash}.{}",
            parts.span.start_line,
            parts.span.end_line,
            hex_encode(path.as_bytes())
        );
        if id.len() > MAX_EVIDENCE_ID_BYTES {
            return Err(FinderError::InvalidInput(
                "evidence id exceeds bound".to_string(),
            ));
        }
        Ok(Self(id))
    }

    pub fn parse(text: &str) -> Result<(EvidenceParts, String), FinderError> {
        fn malformed(what: &str) -> FinderError {
            FinderError::InvalidInput(format!("malformed evidence id: {what}"))
        }
        let rest = text
            .strip_prefix("rf1.")
            .ok_or_else(|| malformed("prefix"))?;
        let segments: Vec<&str> = rest.split('.').collect();
        if segments.len() != 5 {
            return Err(malformed("segments"));
        }
        if segments[0].is_empty()
            || !segments[0]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(malformed("revision"));
        }
        let revision = Revision::new(segments[0]).map_err(|_| malformed("revision"))?;
        let start: u32 = segments[1].parse().map_err(|_| malformed("span start"))?;
        let end: u32 = segments[2].parse().map_err(|_| malformed("span end"))?;
        let span = HitSpan::new(start, end).map_err(|_| malformed("span"))?;
        let hash = segments[3].to_ascii_lowercase();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(malformed("hash"));
        }
        let path_bytes = hex_decode(segments[4]).ok_or_else(|| malformed("path"))?;
        let path_text = String::from_utf8(path_bytes).map_err(|_| malformed("path"))?;
        let path = PathBuf::from(path_text);
        validate_relative_confined(&path).map_err(|_| malformed("path"))?;
        let parts = EvidenceParts::new(revision, path, span).map_err(|_| malformed("parts"))?;
        Ok((parts, hash))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Lowercase hex encoding for handle and token segments.
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Lowercase hex decoding for handle and token segments.
/// Rejects odd lengths and non-hex bytes fail-closed (`None`).
pub fn hex_decode(text: &str) -> Option<Vec<u8>> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push(nibble(pair[0])? << 4 | nibble(pair[1])?);
    }
    Some(out)
}

/// Where a hit matched: the path itself, or file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchField {
    Path,
    Content,
}

/// Why a hit was selected: the matched terms, the field, and the
/// adapter's rank score. Scores are adapter-local and comparable
/// only within one outcome; the terms are the explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitReason {
    pub matched_terms: Vec<String>,
    pub field: MatchField,
    pub rank_score: u32,
}

/// One selected hit: a reopenable handle plus the bounded excerpt
/// that justifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub evidence_id: EvidenceId,
    pub path: PathBuf,
    pub span: HitSpan,
    pub excerpt: String,
    pub reason: HitReason,
}

impl Hit {
    pub fn new(
        evidence_id: EvidenceId,
        path: PathBuf,
        span: HitSpan,
        excerpt: String,
        reason: HitReason,
    ) -> Result<Self, FinderError> {
        validate_relative_confined(&path)?;
        if path.as_os_str().len() > MAX_PATH_BYTES {
            return Err(FinderError::InvalidInput(
                "hit path exceeds bound".to_string(),
            ));
        }
        if excerpt.chars().count() > MAX_EXCERPT_CHARS {
            return Err(FinderError::InvalidInput(
                "excerpt exceeds bound".to_string(),
            ));
        }
        Ok(Self {
            evidence_id,
            path,
            span,
            excerpt,
            reason,
        })
    }
}

/// Why a source was skipped instead of searched. Skips are reported,
/// never silent: the caller sees what the selection did not cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// Binary content is never content-matched.
    Binary,
    /// The file exceeds the adapter's search bound.
    TooLarge { bytes: u64 },
    /// The file could not be read (bounded detail).
    Inaccessible { detail: String },
    /// A confinement rule excluded the path (names the rule).
    Denied { rule: String },
}

/// A source the selection did not cover, with its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSource {
    pub path: PathBuf,
    pub reason: SkipReason,
}

/// One bounded page of hits. Invariants the seam keeps: `hits` never
/// exceeds the query limit, `total_hits` never understates it, and a
/// continuation token exists exactly when hits remain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedHits {
    pub hits: Vec<Hit>,
    pub total_hits: usize,
    pub skipped: Vec<SkippedSource>,
    pub continuation: Option<ContinuationToken>,
}

/// A search outcome: what revision the selection binds to, which
/// engine produced it, and the bounded page itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    pub provenance: RepoProvenance,
    pub backend: BackendId,
    pub hits: BoundedHits,
}

/// Exact current bytes of a reopened span, with their content hash
/// and revision binding. The caller compares the hash against the
/// handle it holds; any drift is stale evidence, never a silent fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanContent {
    pub lines: Vec<String>,
    pub content_hash: String,
    pub provenance: RepoProvenance,
}

/// A reopened span plus its adjacent context, from one read. The
/// `core` is the verified span (hash-bound, staleness-checked); the
/// context lines are display-adjacent to it — same read, clamped at
/// file edges, never exceeding the request. A zero-context window
/// is exactly the span reopen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowContent {
    pub core: SpanContent,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_failure_kind() {
        assert!(FinderError::InvalidInput("x".to_string())
            .to_string()
            .starts_with("invalid input"));
        assert!(FinderError::Stale {
            expected_rev: "a".to_string(),
            actual_rev: "b".to_string(),
        }
        .to_string()
        .starts_with("stale evidence"));
        assert!(FinderError::ScopeEscape {
            detail: "x".to_string()
        }
        .to_string()
        .starts_with("scope escape"));
    }

    #[test]
    fn repo_root_rejects_blank() {
        assert!(RepoRoot::new("").is_err());
    }

    #[test]
    fn repo_root_rejects_relative() {
        assert!(RepoRoot::new("repo/sub").is_err());
    }

    #[test]
    fn repo_root_accepts_absolute() {
        assert!(RepoRoot::new("/repo/sub").is_ok());
    }

    #[test]
    fn revision_rejects_blank() {
        assert!(Revision::new("  ").is_err());
    }

    #[test]
    fn revision_rejects_over_long() {
        assert!(Revision::new("a".repeat(MAX_REVISION_CHARS + 1)).is_err());
    }

    #[test]
    fn revision_accepts_sha() {
        assert!(Revision::new("40754ae40754ae40754ae40754ae40754ae40754ae").is_ok());
    }

    #[test]
    fn query_rejects_blank_text() {
        assert!(FinderQuery::new("  ", 10).is_err());
    }

    #[test]
    fn query_rejects_over_long_text() {
        assert!(FinderQuery::new("a".repeat(MAX_QUERY_TEXT_CHARS + 1), 10).is_err());
    }

    #[test]
    fn query_rejects_zero_limit() {
        assert!(FinderQuery::new("needle", 0).is_err());
    }

    #[test]
    fn query_rejects_over_limit() {
        assert!(FinderQuery::new("needle", MAX_QUERY_LIMIT + 1).is_err());
    }

    #[test]
    fn query_accepts_plain() {
        let query = FinderQuery::new("needle", 10).unwrap();
        assert_eq!(query.text(), "needle");
        assert_eq!(query.limit(), 10);
        assert!(query.scope().is_none());
        assert!(query.continuation().is_none());
    }

    #[test]
    fn query_scope_rejects_absolute() {
        let query = FinderQuery::new("needle", 10).unwrap();
        assert!(query.with_scope("/etc").is_err());
    }

    #[test]
    fn query_scope_rejects_parent_dir() {
        let query = FinderQuery::new("needle", 10).unwrap();
        assert!(query.with_scope("../evil").is_err());
    }

    #[test]
    fn query_scope_rejects_leading_dot() {
        let query = FinderQuery::new("needle", 10).unwrap();
        assert!(query.with_scope("./src").is_err());
    }

    #[test]
    fn query_scope_accepts_interior_dot_as_confined() {
        // `components()` normalizes interior `.` away; `src/./main.rs`
        // denotes the confined `src/main.rs` and is accepted as such.
        let query = FinderQuery::new("needle", 10)
            .unwrap()
            .with_scope("src/./main.rs")
            .unwrap();
        assert_eq!(query.scope(), Some(Path::new("src/./main.rs")));
    }

    #[test]
    fn query_scope_accepts_subdir() {
        let query = FinderQuery::new("needle", 10)
            .unwrap()
            .with_scope("src/sub")
            .unwrap();
        assert_eq!(query.scope(), Some(Path::new("src/sub")));
    }

    #[test]
    fn continuation_rejects_blank() {
        assert!(ContinuationToken::new("").is_err());
    }

    #[test]
    fn continuation_rejects_over_long() {
        assert!(ContinuationToken::new("a".repeat(MAX_CONTINUATION_BYTES + 1)).is_err());
    }

    #[test]
    fn continuation_accepts_opaque() {
        assert!(ContinuationToken::new("rfc1.opaque").is_ok());
    }

    #[test]
    fn hex_roundtrips_bytes() {
        assert_eq!(hex_decode(""), Some(Vec::new()));
        assert_eq!(hex_decode("00ff"), Some(vec![0u8, 255u8]));
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_encode(&[0u8, 255u8]), "00ff");
    }

    #[test]
    fn replace_continuation_swaps_token() {
        let query = FinderQuery::new("needle", 10)
            .unwrap()
            .with_continuation(ContinuationToken::new("a").unwrap())
            .replace_continuation(Some(ContinuationToken::new("b").unwrap()));
        assert_eq!(query.continuation().unwrap().as_str(), "b");
        let query = query.replace_continuation(None);
        assert!(query.continuation().is_none());
        assert_eq!(query.text(), "needle");
    }

    #[test]
    fn backend_rejects_blank_name() {
        assert!(BackendId::new("", "1").is_err());
    }

    #[test]
    fn backend_rejects_blank_version() {
        assert!(BackendId::new("local", "").is_err());
    }

    #[test]
    fn backend_accepts_identity() {
        let id = BackendId::new("finder-local", "0.1.0").unwrap();
        assert_eq!(id.name, "finder-local");
        assert_eq!(id.version, "0.1.0");
    }

    #[test]
    fn span_rejects_zero_start() {
        assert!(HitSpan::new(0, 3).is_err());
    }

    #[test]
    fn span_rejects_end_before_start() {
        assert!(HitSpan::new(5, 4).is_err());
    }

    #[test]
    fn span_rejects_over_long_span() {
        assert!(HitSpan::new(1, MAX_SPAN_LINES + 1).is_err());
    }

    #[test]
    fn span_accepts_single_line() {
        let span = HitSpan::new(7, 7).unwrap();
        assert_eq!(span.start_line, 7);
        assert_eq!(span.end_line, 7);
    }

    fn sample_parts() -> EvidenceParts {
        EvidenceParts::new(
            Revision::new("abc123").unwrap(),
            PathBuf::from("src/main.rs"),
            HitSpan::new(3, 5).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn parts_rejects_escaping_path() {
        assert!(EvidenceParts::new(
            Revision::new("abc123").unwrap(),
            PathBuf::from("../evil.rs"),
            HitSpan::new(1, 1).unwrap(),
        )
        .is_err());
    }

    #[test]
    fn parts_accepts_relative() {
        assert_eq!(sample_parts().path, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn content_hash_pins_known_vector() {
        // SHA-256("a"); pins algorithm and canonical join at once.
        assert_eq!(
            content_hash(&["a".to_string()]),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
    }

    #[test]
    fn content_hash_separates_lines() {
        assert_ne!(
            content_hash(&["a".to_string(), "b".to_string()]),
            content_hash(&["ab".to_string()])
        );
        assert_eq!(
            content_hash(&["a".to_string(), "b".to_string()]),
            content_hash(&["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn evidence_id_roundtrips_parts() {
        let parts = sample_parts();
        let hash = content_hash(&["l3".to_string(), "l4".to_string(), "l5".to_string()]);
        let id = EvidenceId::mint(&parts, &hash).unwrap();
        let (parsed, parsed_hash) = EvidenceId::parse(id.as_str()).unwrap();
        assert_eq!(parsed, parts);
        assert_eq!(parsed_hash, hash);
    }

    #[test]
    fn evidence_id_rejects_wrong_prefix() {
        assert!(EvidenceId::parse("xx1.abc.1.2.00.ff").is_err());
    }

    #[test]
    fn evidence_id_rejects_tampered_segments() {
        let parts = sample_parts();
        let hash = content_hash(&["l3".to_string()]);
        let id = EvidenceId::mint(&parts, &hash).unwrap();
        let mut tampered = id.as_str().to_string();
        let flip = if tampered.ends_with('0') { '1' } else { '0' };
        tampered.pop();
        tampered.push(flip);
        // Either the id is rejected, or it parses to different parts —
        // never silently back to the original handle.
        match EvidenceId::parse(&tampered) {
            Err(_) => {}
            Ok((parsed, _)) => assert_ne!(parsed, parts),
        }
    }

    fn sample_hit(path: &str, excerpt: &str) -> Result<Hit, FinderError> {
        let parts = EvidenceParts::new(
            Revision::new("abc123").unwrap(),
            PathBuf::from(path),
            HitSpan::new(1, 1).unwrap(),
        )?;
        let hash = content_hash(&[excerpt.to_string()]);
        Hit::new(
            EvidenceId::mint(&parts, &hash).unwrap(),
            PathBuf::from(path),
            HitSpan::new(1, 1).unwrap(),
            excerpt.to_string(),
            HitReason {
                matched_terms: vec!["needle".to_string()],
                field: MatchField::Content,
                rank_score: 7,
            },
        )
    }

    fn sample_reason() -> HitReason {
        HitReason {
            matched_terms: vec!["needle".to_string()],
            field: MatchField::Content,
            rank_score: 7,
        }
    }

    #[test]
    fn hit_rejects_escaping_path() {
        // The handle itself is well-formed; only the hit path escapes,
        // so this failure belongs to `Hit::new`, not to the parts.
        let parts = EvidenceParts::new(
            Revision::new("abc123").unwrap(),
            PathBuf::from("src/a.rs"),
            HitSpan::new(1, 1).unwrap(),
        )
        .unwrap();
        let hash = content_hash(&["needle".to_string()]);
        let id = EvidenceId::mint(&parts, &hash).unwrap();
        assert!(Hit::new(
            id,
            PathBuf::from("../evil.rs"),
            HitSpan::new(1, 1).unwrap(),
            "needle".to_string(),
            sample_reason(),
        )
        .is_err());
    }

    #[test]
    fn hit_rejects_over_long_excerpt() {
        assert!(sample_hit("src/a.rs", &"x".repeat(MAX_EXCERPT_CHARS + 1)).is_err());
    }

    #[test]
    fn hit_accepts_bounded() {
        let hit = sample_hit("src/a.rs", "needle here").unwrap();
        assert_eq!(hit.path, PathBuf::from("src/a.rs"));
        assert_eq!(hit.reason.rank_score, 7);
    }

    #[test]
    fn evidence_id_mint_rejects_unsafe_revision() {
        let parts = EvidenceParts::new(
            Revision::new("release.1").unwrap(),
            PathBuf::from("src/main.rs"),
            HitSpan::new(1, 1).unwrap(),
        )
        .unwrap();
        assert!(EvidenceId::mint(&parts, &"0".repeat(64)).is_err());
    }
}
