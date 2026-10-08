//! Lexical repository search engine (TASK-0147, G9).
//!
//! [`LocalFinder`] implements the `repo-finder` seam over the local
//! filesystem: literal case-sensitive substring matching, ignore
//! files honored, symlinks never followed, binaries never
//! content-matched, every page bounded with an explicit continuation.
//! Revision binding comes from an injected [`RepoRevisionPort`]
//! (production: the `git-cli` backend); this crate never shells out
//! to Git and never guesses a revision.
//!
//! Ranking (documented, deterministic, total order): score desc,
//! then the external-usage file tiebreak, then path asc, then line
//! asc. Filename hits score 1000+ (always above content); content
//! hits score matched-term count plus symbol signals (+2 whole
//! token, +4 Rust definition line). Scores are adapter-local; the
//! matched terms are the explanation.
//!
//! Continuation tokens are plain offsets (`off:<n>`) into the total
//! ordered match list. They are NOT bound to a query or revision at
//! this layer: binding tokens to query+revision is the service's job
//! (a token replayed against another query pages that query's list).
//!
//! [`IndexedFinder`](index::IndexedFinder) serves the same seam from
//! a revision-bound in-memory snapshot: identical outcomes without
//! the per-query filesystem walk.

pub mod index;

pub use index::IndexedFinder;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use repo_finder::{
    content_hash, BackendId, BoundedHits, ContinuationToken, EvidenceId, EvidenceParts,
    FinderError, FinderQuery, Hit, HitReason, HitSpan, MatchField, RepoProvenance, RepoReadPort,
    RepoRevisionPort, RepoRoot, RepoSearchPort, SearchOutcome, SkipReason, SkippedSource,
    SpanContent, WindowContent, MAX_WINDOW_CONTEXT,
};

/// Default cap for a searchable file (bytes). Larger files are
/// reported [`SkipReason::TooLarge`](repo_finder::SkipReason), never
/// half-searched.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1_048_576;

/// Default cap for a hit excerpt's content portion (chars). Longer
/// lines are cut with an explicit marker (at most 64 chars more);
/// the full line stays reopenable and the handle binds the full line.
pub const DEFAULT_MAX_EXCERPT_CHARS: usize = 1000;

/// Marker budget added past [`DEFAULT_MAX_EXCERPT_CHARS`].
const MARKER_BUDGET_CHARS: usize = 64;

/// Lexical engine. Construct with a revision backend; bounds are
/// tunable for tests via [`LocalFinder::with_bounds`].
pub struct LocalFinder {
    revision: Arc<dyn RepoRevisionPort>,
    max_file_bytes: u64,
    max_excerpt_chars: usize,
}

impl LocalFinder {
    pub fn new(revision: Arc<dyn RepoRevisionPort>) -> Self {
        Self {
            revision,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_excerpt_chars: DEFAULT_MAX_EXCERPT_CHARS,
        }
    }

    pub fn with_bounds(mut self, max_file_bytes: u64, max_excerpt_chars: usize) -> Self {
        self.max_file_bytes = max_file_bytes;
        // Clamped so built excerpts always satisfy the seam bound.
        self.max_excerpt_chars = max_excerpt_chars
            .min(repo_finder::MAX_EXCERPT_CHARS.saturating_sub(MARKER_BUDGET_CHARS));
        self
    }

    fn backend_id() -> BackendId {
        BackendId::new("finder-local", "0.1.0").expect("static backend id must build")
    }

    /// Ignore-aware traversal (ripgrep semantics, hermetic):
    /// repo-local `.gitignore`/`.ignore` honored unconditionally
    /// (`require_git(false)` — the seam promises ignore honoring
    /// without a Git repository). Global excludes, parent-dir rules,
    /// and `.git/info/exclude` are deliberately NOT honored, so two
    /// machines index the same root identically. Hidden files are
    /// skipped; symlinks are yielded but never followed.
    fn walk(&self, root: &Path, out: &mut Vec<PathBuf>) -> Result<(), FinderError> {
        let mut builder = ignore::WalkBuilder::new(root);
        builder
            .hidden(true)
            .git_ignore(true)
            .ignore(true)
            .git_global(false)
            .git_exclude(false)
            .require_git(false)
            .follow_links(false)
            .parents(false)
            .sort_by_file_path(std::cmp::Ord::cmp);
        for entry in builder.build() {
            let entry = entry.map_err(|error| FinderError::BackendFailure {
                detail: bound_detail(&error.to_string()),
            })?;
            let path = entry.path();
            if path == root {
                continue;
            }
            if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                continue;
            }
            // Files and symlinks are collected; symlinks are reported
            // as denied skips at match time, never followed.
            // (Sockets, fifos, devices never arrive here: the walker
            // only yields files, dirs, and symlinks.)
            out.push(strip(root, path)?);
        }
        Ok(())
    }

    fn excerpt(&self, line: &str) -> String {
        if line.chars().count() <= self.max_excerpt_chars {
            return line.to_string();
        }
        let keep = self.max_excerpt_chars.saturating_sub(MARKER_BUDGET_CHARS);
        let truncated: String = line.chars().take(keep).collect();
        let dropped = line.chars().count() - truncated.chars().count();
        format!("{truncated}…[+{dropped} chars, reopen for the full line]")
    }

    /// Reads and classifies one walked file. Shared by the scan path
    /// and the snapshot builder so both see identical bytes, skips,
    /// and failures. Scope filtering stays caller-side (query-time);
    /// callers run [`reject_unrepresentable`] first so invalid paths
    /// report skips without touching the filesystem.
    fn ingest_file(&self, root_path: &Path, relative: &Path) -> Result<IngestedFile, FinderError> {
        let full = root_path.join(relative);
        if full.is_symlink() {
            return Ok(IngestedFile::Skipped(SkippedSource {
                path: relative.to_path_buf(),
                reason: SkipReason::Denied {
                    rule: "symlink".to_string(),
                },
            }));
        }
        let len = full
            .symlink_metadata()
            .map_err(|error| FinderError::BackendFailure {
                detail: bound_detail(&error.to_string()),
            })?
            .len();
        if len > self.max_file_bytes {
            return Ok(IngestedFile::Skipped(SkippedSource {
                path: relative.to_path_buf(),
                reason: SkipReason::TooLarge { bytes: len },
            }));
        }
        let bytes = std::fs::read(&full).map_err(|error| FinderError::BackendFailure {
            detail: bound_detail(&error.to_string()),
        })?;
        if bytes.contains(&0) {
            return Ok(IngestedFile::Skipped(SkippedSource {
                path: relative.to_path_buf(),
                reason: SkipReason::Binary,
            }));
        }
        let text = String::from_utf8_lossy(&bytes);
        Ok(IngestedFile::Content {
            lines: text.lines().map(str::to_string).collect(),
            bytes: bytes.len() as u64,
        })
    }

    /// Ranks candidates, pages them, and mints evidence handles.
    /// Shared by the scan path and the indexed path so both emit
    /// identical pages for identical candidate sets.
    fn finish_search(
        &self,
        provenance: RepoProvenance,
        candidates: Vec<Candidate>,
        skipped: Vec<SkippedSource>,
        query: &FinderQuery,
    ) -> Result<SearchOutcome, FinderError> {
        // Defining files per matched term: the Rust files holding a
        // definition line for the term. A content candidate outside
        // every matched term's defining files outranks a same-file
        // mention at EQUAL score only — the defining file is already
        // represented by the definition itself, so its further
        // mentions (imports, re-exports, recursion) yield to external
        // usages. Content-only: filename hits keep pure path order
        // among themselves. Deterministic per query and corpus.
        let mut defining_files: std::collections::HashMap<String, Vec<PathBuf>> =
            std::collections::HashMap::new();
        for candidate in &candidates {
            if candidate.field != MatchField::Content || !candidate.is_rust() {
                continue;
            }
            for term in &candidate.terms {
                if defines_symbol(&candidate.line, term) {
                    defining_files
                        .entry(term.clone())
                        .or_default()
                        .push(candidate.path.clone());
                }
            }
        }
        let is_external = |candidate: &Candidate| -> bool {
            candidate.field == MatchField::Content
                && candidate.terms.iter().all(|term| {
                    defining_files
                        .get(term.as_str())
                        .is_none_or(|files| !files.contains(&candidate.path))
                })
        };
        // Scores and file preference are precomputed once: the
        // comparator below must stay a pure total order without
        // re-tokenizing on every comparison.
        let mut ranked: Vec<(u32, bool, Candidate)> = candidates
            .into_iter()
            .map(|candidate| {
                let score = candidate.score();
                let external = is_external(&candidate);
                (score, external, candidate)
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.path.cmp(&b.2.path))
                .then_with(|| a.2.line_no.cmp(&b.2.line_no))
        });
        let candidates: Vec<Candidate> = ranked
            .into_iter()
            .map(|(_, _, candidate)| candidate)
            .collect();
        let total_hits = candidates.len();
        let offset = parse_offset(query.continuation())?;
        let page: Vec<Candidate> = candidates
            .into_iter()
            .skip(offset)
            .take(query.limit())
            .collect();
        let continuation = if offset + page.len() < total_hits {
            Some(
                ContinuationToken::new(format!("off:{}", offset + page.len()))
                    .expect("offset token must build: bounded digits under a short prefix"),
            )
        } else {
            None
        };
        let mut hits = Vec::with_capacity(page.len());
        for candidate in page {
            let span = HitSpan::new(candidate.line_no, candidate.line_no)
                .expect("candidate spans must build: one-based lines within a read file");
            let parts =
                EvidenceParts::new(provenance.revision.clone(), candidate.path.clone(), span)
                    .expect("candidate parts must build: confined walk-relative paths");
            // The handle binds the FULL line; the excerpt may carry a
            // truncation marker. Reopen compares against full bytes.
            let hash = content_hash(std::slice::from_ref(&candidate.line));
            // The revision arrives from an injected port: an unsafe
            // value is a backend failure, never a panic.
            let id = EvidenceId::mint(&parts, &hash).map_err(|_| FinderError::BackendFailure {
                detail: "revision is not evidence-safe".to_string(),
            })?;
            let score = candidate.score();
            hits.push(
                Hit::new(
                    id,
                    candidate.path,
                    span,
                    self.excerpt(&candidate.line),
                    HitReason {
                        matched_terms: candidate.terms,
                        field: candidate.field,
                        rank_score: score,
                    },
                )
                .expect("candidate hit must build: bounded excerpt, confined path"),
            );
        }
        Ok(SearchOutcome {
            provenance,
            backend: Self::backend_id(),
            hits: BoundedHits {
                total_hits,
                hits,
                skipped,
                continuation,
            },
        })
    }
}

/// One ingested file: searchable lines plus their byte weight, or an
/// explicit skip recording why the file stays out.
enum IngestedFile {
    Content { lines: Vec<String>, bytes: u64 },
    Skipped(SkippedSource),
}

/// Pure path-validity gate (no I/O): non-UTF-8 or overlong walked
/// paths report an explicit skip instead of reaching the filesystem.
fn reject_unrepresentable(relative: &Path) -> Option<SkippedSource> {
    if relative.to_str().is_none() || relative.as_os_str().len() > repo_finder::MAX_PATH_BYTES {
        Some(SkippedSource {
            path: PathBuf::from("<unrepresentable-path>"),
            reason: SkipReason::Denied {
                rule: "non-utf8-or-overlong-path".to_string(),
            },
        })
    } else {
        None
    }
}

/// Collects path-hit and content candidates for `terms` over one
/// file's lines. Shared by scan and index so both rank identical
/// candidate sets.
fn collect_candidates(relative: &Path, lines: &[String], terms: &[&str]) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let file_name = relative
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let name_terms: Vec<String> = terms
        .iter()
        .filter(|term| file_name.contains(*term))
        .map(|term| term.to_string())
        .collect();
    if !name_terms.is_empty() {
        if let Some(first) = lines.first() {
            candidates.push(Candidate {
                path: relative.to_path_buf(),
                line_no: 1,
                line: first.clone(),
                terms: name_terms,
                field: MatchField::Path,
            });
        }
    }
    for (index, line) in lines.iter().enumerate() {
        let matched: Vec<String> = terms
            .iter()
            .filter(|term| line.contains(*term))
            .map(|term| term.to_string())
            .collect();
        if !matched.is_empty() {
            candidates.push(Candidate {
                path: relative.to_path_buf(),
                line_no: index as u32 + 1,
                line: line.clone(),
                terms: matched,
                field: MatchField::Content,
            });
        }
    }
    candidates
}

/// One ordered match before paging.
struct Candidate {
    path: PathBuf,
    line_no: u32,
    line: String,
    terms: Vec<String>,
    field: MatchField,
}

impl Candidate {
    /// Rust source files only: prose in other languages must not
    /// earn Rust definition signals.
    fn is_rust(&self) -> bool {
        self.path.extension().is_some_and(|ext| ext == "rs")
    }

    fn score(&self) -> u32 {
        match self.field {
            MatchField::Path => 1000 + self.terms.len() as u32,
            MatchField::Content => {
                let base = self.terms.len() as u32;
                let tokens: Vec<&str> = split_symbol_tokens(&self.line).collect();
                let whole = self
                    .terms
                    .iter()
                    .any(|term| tokens.contains(&term.as_str()));
                let defined = self.is_rust()
                    && self
                        .terms
                        .iter()
                        .any(|term| defines_symbol(&self.line, term));
                base + u32::from(whole) * 2 + u32::from(defined) * 4
            }
        }
    }
}

/// Strips a leading whole word (`unsafe` but never `unsafe_x`).
/// Returns the remainder, or `None` when the word is absent.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let after = text.strip_prefix(word)?;
    if after.is_empty() || after.starts_with(|c: char| !c.is_alphanumeric() && c != '_') {
        Some(after)
    } else {
        None
    }
}

/// Skips a balanced `[...]` group at the start of `text` (nested
/// groups count). Returns the remainder after the closing bracket.
fn skip_balanced(text: &str, open: char, close: char) -> Option<&str> {
    let mut depth = 0u32;
    for (index, c) in text.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(&text[index + 1..]);
            }
        }
    }
    None
}

/// Symbol tokens: maximal runs of identifier characters. Splits
/// `foo::bar`, `x.y`, and `f(x)` into comparable tokens.
fn split_symbol_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
}

/// True when the line defines `term`: a Rust item or binding keyword
/// whose first symbol token is the term. Leading `pub`/`pub(...)`,
/// same-line `#[...]` attributes, and `fn` qualifiers (`unsafe`,
/// `async`, `const`, `extern "..."`) are skipped; `let other = term`
/// does NOT count — only the bound name earns the boost. Callers
/// gate on Rust source files; this stays a pure line predicate.
fn defines_symbol(line: &str, term: &str) -> bool {
    const KEYWORDS: [&str; 11] = [
        "fn",
        "struct",
        "enum",
        "trait",
        "type",
        "const",
        "static",
        "mod",
        "let",
        "macro_rules",
        "macro",
    ];
    // One visibility qualifier at most (`pub`, `pub(...)`, `pub ` —
    // never `public`); a second `pub` is not valid Rust.
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        if after.starts_with('(') {
            let mut depth = 0u32;
            let mut end = None;
            for (index, c) in after.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(index + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            match end {
                Some(end) => rest = after[end..].trim_start(),
                None => return false,
            }
        } else if after.starts_with(char::is_whitespace) {
            rest = after.trim_start();
        } else if after.is_empty() {
            return false;
        }
    }
    // Same-line attributes (`#[inline] fn`) and `fn` qualifiers
    // (`pub unsafe fn`, `async fn`, `const fn`, `extern "C" fn`):
    // strip in a loop, since they compose. `const` strips only
    // before `fn` — a bare `const NAME` is the keyword itself.
    loop {
        let before = rest;
        if rest.starts_with("#[") {
            match skip_balanced(rest, '[', ']') {
                Some(after) => rest = after.trim_start(),
                None => return false,
            }
        }
        for qualifier in ["unsafe", "async", "extern"] {
            if let Some(after) = strip_word(rest, qualifier) {
                rest = after.trim_start();
                // `extern "C"`: skip the ABI literal too.
                if qualifier == "extern" && rest.starts_with('"') {
                    if let Some(end) = rest[1..].find('"') {
                        rest = rest[1 + end + 1..].trim_start();
                    } else {
                        return false;
                    }
                }
            }
        }
        if let Some(after) = strip_word(rest, "const") {
            if after.trim_start().starts_with("fn")
                && after.trim_start()[2..].starts_with(|c: char| c.is_whitespace() || c == '(')
            {
                rest = after.trim_start();
            }
        }
        if rest == before {
            break;
        }
    }
    for keyword in KEYWORDS {
        let after = match rest.strip_prefix(keyword) {
            Some(after)
                if after.starts_with(char::is_whitespace)
                    || after.starts_with('(')
                    || after.starts_with('!') =>
            {
                after
            }
            _ => continue,
        };
        // `macro_rules! name` / `macro name`: skip the `!`.
        let after = after.trim_start_matches('!').trim_start();
        let mut tokens = split_symbol_tokens(after);
        if tokens.next() == Some(term) {
            return true;
        }
    }
    false
}

impl RepoSearchPort for LocalFinder {
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
        let terms: Vec<&str> = query.text().split_whitespace().collect();
        let mut files = Vec::new();
        self.walk(root_path, &mut files)?;
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut skipped: Vec<SkippedSource> = Vec::new();
        for relative in files {
            if let Some(skip) = reject_unrepresentable(&relative) {
                skipped.push(skip);
                continue;
            }
            if let Some(scope) = query.scope() {
                if !relative.starts_with(scope) {
                    continue;
                }
            }
            match self.ingest_file(root_path, &relative)? {
                IngestedFile::Skipped(skip) => skipped.push(skip),
                IngestedFile::Content { lines, .. } => {
                    candidates.extend(collect_candidates(&relative, &lines, &terms));
                }
            }
        }
        self.finish_search(provenance, candidates, skipped, query)
    }
}

impl RepoReadPort for LocalFinder {
    fn read_span(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
    ) -> Result<SpanContent, FinderError> {
        let (all, provenance) = self.guarded_lines(root, evidence)?;
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
            provenance,
        })
    }

    fn read_window(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
        before: u32,
        after: u32,
    ) -> Result<WindowContent, FinderError> {
        if before > MAX_WINDOW_CONTEXT || after > MAX_WINDOW_CONTEXT {
            return Err(FinderError::InvalidInput(
                "context exceeds bound".to_string(),
            ));
        }
        // One guarded read, sliced twice: span and window share every
        // confinement check, so the window cannot widen a hole.
        let (all, provenance) = self.guarded_lines(root, evidence)?;
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
                provenance,
            },
            before: all[from..start - 1].to_vec(),
            after: all[end..upto].to_vec(),
        })
    }
}

impl LocalFinder {
    /// The guarded single read behind span and window reopen:
    /// revision binding plus every confinement check (symlink,
    /// file-type, canonical-scope), then all file lines. Both reopen
    /// shapes share this, so the window cannot widen a hole.
    fn guarded_lines(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
    ) -> Result<(Vec<String>, RepoProvenance), FinderError> {
        let provenance = self.revision.resolve(root)?;
        let canonical_root =
            root.as_path()
                .canonicalize()
                .map_err(|_| FinderError::NotARepository {
                    root: root.as_path().to_path_buf(),
                })?;
        let full = root.as_path().join(&evidence.path);
        let file_type = std::fs::symlink_metadata(&full)
            .map_err(|_| FinderError::UnknownEvidence {
                detail: "file cannot be read".to_string(),
            })?
            .file_type();
        if file_type.is_symlink() {
            return Err(FinderError::UnknownEvidence {
                detail: "symlinks are not followed".to_string(),
            });
        }
        if !file_type.is_file() {
            return Err(FinderError::UnknownEvidence {
                detail: "not a file".to_string(),
            });
        }
        let canonical = full
            .canonicalize()
            .map_err(|_| FinderError::UnknownEvidence {
                detail: "file cannot be resolved".to_string(),
            })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(FinderError::ScopeEscape {
                detail: "path escapes repository scope".to_string(),
            });
        }
        let bytes = std::fs::read(&canonical).map_err(|_| FinderError::UnknownEvidence {
            detail: "file cannot be read".to_string(),
        })?;
        let all: Vec<String> = String::from_utf8_lossy(&bytes)
            .lines()
            .map(str::to_string)
            .collect();
        Ok((all, provenance))
    }
}

fn strip(root: &Path, path: &Path) -> Result<PathBuf, FinderError> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| FinderError::BackendFailure {
            detail: "walked path escapes its root".to_string(),
        })
}

fn bound_detail(detail: &str) -> String {
    detail.chars().take(repo_finder::MAX_DETAIL_CHARS).collect()
}

fn parse_offset(token: Option<&ContinuationToken>) -> Result<usize, FinderError> {
    match token {
        None => Ok(0),
        Some(token) => token
            .as_str()
            .strip_prefix("off:")
            .and_then(|digits| digits.parse::<usize>().ok())
            .ok_or_else(|| FinderError::InvalidContinuation {
                reason: "continuation must be `off:<n>`".to_string(),
            }),
    }
}
