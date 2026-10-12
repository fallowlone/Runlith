//! The finder retrieval seam: search, span reopen, revision binding.
//!
//! Three narrow traits, one seam promise each. Engines are
//! replaceable behind this seam (ADR-0010): the phase-B lexical
//! engine, a future symbol index, and a future vector engine all
//! implement the same traits, and every one of them passes the same
//! `repo-finder-contract` conformance runner.
//!
//! What the seam promises (pinned by the contract suite):
//!
//! ```text
//! search  → bounded page: hits within limit, total never understated,
//!           continuation exactly when hits remain, skips explicit with
//!           reasons, ignore files honored, root confined, deterministic
//!           order, every hit explained (terms + field)
//! reopen  → exact current bytes of the span plus their content hash and
//!           revision binding; missing files are UnknownEvidence, never
//!           synthesized or neighboring content
//! window  → the verified span plus adjacent context from one read;
//!           core hash binds the core bytes, context clamps at edges
//! resolve → stable revision binding for the root, or NotARepository
//! ```
//!
//! Deliberately NOT in the contract (not promised by the seam):
//!
//! ```text
//! ranking    → scores are adapter-local; only determinism and the
//!              matched-terms explanation are pinned, never the formula
//! excerpt    → styling is adapter-local within the generous bound
//! redaction  → the seam passes bytes; secret filtering is a
//!              service-stage seam (`ContentFilter`), not shown here
//! freshness  → staleness detection compares handle against outcome and
//!              lives in the service; the seam only binds honestly
//! performance → no latency promise; the TASK-0147 benchmark measures it
//! ```

use crate::model::{
    BackendId, EvidenceParts, FinderError, FinderQuery, RepoProvenance, RepoRoot, SearchOutcome,
    SpanContent, WindowContent,
};

/// Bounded lexical-or-better search over one repository root.
pub trait RepoSearchPort: Send + Sync {
    /// Stable engine identity for the response envelope.
    fn backend(&self) -> BackendId;

    /// One bounded page against `root`. Binds the outcome to the
    /// current revision; honors `query` limit, scope, and
    /// continuation; never leaks paths outside `root`.
    fn search(&self, root: &RepoRoot, query: &FinderQuery) -> Result<SearchOutcome, FinderError>;
}

/// Exact span reopen behind an evidence handle.
pub trait RepoReadPort: Send + Sync {
    /// Current bytes of the span named by `evidence`, with their
    /// content hash and revision binding. Missing or unreadable files
    /// are [`FinderError::UnknownEvidence`]; the port never invents
    /// bytes and never substitutes a neighboring file.
    fn read_span(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
    ) -> Result<SpanContent, FinderError>;

    /// The span named by `evidence` plus up to `before` preceding
    /// and `after` following lines, from one read. The core carries
    /// the same hash and binding as [`RepoReadPort::read_span`];
    /// context clamps at file edges and never exceeds the request.
    /// Context beyond [`crate::model::MAX_WINDOW_CONTEXT`] per side
    /// is [`FinderError::InvalidInput`]; missing or unreadable files
    /// are [`FinderError::UnknownEvidence`].
    fn read_window(
        &self,
        root: &RepoRoot,
        evidence: &EvidenceParts,
        before: u32,
        after: u32,
    ) -> Result<WindowContent, FinderError>;
}

/// Revision binding for a repository root.
pub trait RepoRevisionPort: Send + Sync {
    /// The revision (and dirtiness) `root` is currently at. Roots the
    /// backend cannot bind are [`FinderError::NotARepository`].
    fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError>;
}
